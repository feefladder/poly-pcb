use derive_more::Display;
use exn::{OptionExt, ResultExt};
use log::{error, info};
use rusqlite::Connection;
use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    error::Error,
};
use three_d::*;

#[derive(Debug, Clone, PartialEq)]
pub struct Polyhedron {
    pub name: String,
    /// vertices in 3d space
    pub vertices: Vec<Vec3>,
    /// list of faces, with their vertices
    ///
    /// This is u32 because [`three_d::Indices`] is strongly typed and usize is u32 on wasm.
    pub faces: Vec<Vec<u32>>,
    /// per-face transforms
    /// ```
    /// // put glb on face `i`:
    /// glb.transform(poly.face_transforms[i])
    /// ```
    pub face_transforms: Vec<Mat4>,
    /// edge path
    ///
    /// This is the only really deterministic way of encoding a path through the
    /// pcbs, since a face can be visited more than once, not "concentrically"
    ///
    /// e.g. for square, counting counter-clockwise, 01 23 is allowed, but 03 12
    /// is not.
    ///
    /// also how is indexing, using two indices?
    ///
    /// yes, so basically, direction can be determined from previous edge
    ///
    /// ```text
    ///
    /// 0---1      1
    ///  \ / \    / \
    ///   2---?  0---2
    /// ```
    ///
    /// he lol, stokes's theorem thingies, so edges cancel out. aka edge -> will
    /// be <- edge on other polygon.
    ///
    /// 1. find controller
    /// 2. go left
    /// 3. keep going as-left-as-possible untill filled polyhedron
    ///
    /// and the main thingy is that seeing those things is easiest on the
    /// projection, so that's why want projection.
    ///
    /// also for assembly.
    ///
    /// And orientation of pcbs is done according to edge indices of the face normally.
    pub edge_path: Vec<(usize, usize)>,
}

/// An edge of a polyhedron
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Display, Clone)]
pub struct PolyError(String);
impl Error for PolyError {}

impl Polyhedron {
    pub fn iter_ngon(&self, n_sides: usize) -> impl Iterator<Item = usize> {
        self.faces
            .iter()
            .enumerate()
            .filter(move |(_, f)| f.len() == n_sides)
            .map(|(i, _)| i)
    }

    pub fn centroid(&self) -> Vec3 {
        self.vertices.iter().sum::<Vec3>() / self.vertices.len() as f32
    }

    pub fn load(conn: &Connection, longname: &str) -> exn::Result<Polyhedron, PolyError> {
        let poly_id: i64 = conn
            .query_row(
                "SELECT id FROM Polyhedron WHERE longname = ?",
                [longname],
                |row| row.get(0),
            )
            .or_raise(|| PolyError(format!("polyhedron {longname} not found")))?;

        let mut poly = Polyhedron {
            name: longname.to_owned(),
            vertices: Vec::new(),
            faces: Vec::new(),
            face_transforms: Vec::new(),
            edge_path: Vec::new(),
        };

        let mut stmt = conn
            .prepare(
                "
        SELECT id, x, y, z
        FROM Vertex
        WHERE poly = ?
        ORDER BY id
        ",
            )
            .or_raise(|| PolyError(format!("could not formulate vertex sql for {longname}")))?;

        poly.vertices = stmt
            .query_map([poly_id], |row| {
                Ok(vec3(
                    row.get::<_, f32>(1)?,
                    row.get::<_, f32>(2)?,
                    row.get::<_, f32>(3)?,
                ))
            })
            .or_raise(|| PolyError(format!("could not load polyhedron {longname}")))?
            .collect::<Result<Vec<_>, _>>()
            .or_raise(|| {
                PolyError(format!(
                    "could not decode vertex coordinates for {longname}"
                ))
            })?;

        let mut stmt = conn
            .prepare(
                "
        SELECT face, vertex, idx
        FROM Polygon
        WHERE poly = ?
        ORDER BY face, idx
        ",
            )
            .or_raise(|| PolyError(format!("could not load polyhedron {longname}")))?;

        poly.faces = stmt
            .query_map([poly_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .or_raise(|| PolyError(format!("Could not load polyhedron {longname}")))?
            .collect::<Result<Vec<_>, _>>()
            .or_raise(|| {
                PolyError(format!(
                    "Could not populate faces for polyhedron {longname}"
                ))
            })?
            .chunk_by(|(face_id_a, _, _), (face_id_b, _, _)| face_id_a == face_id_b)
            .map(|slice| {
                slice
                    .into_iter()
                    .map(|(_, vertex_id, _)| *vertex_id as u32)
                    .collect()
            })
            .collect();

        // check winding order
        // This is mainly needed for putting the rakes in the right place
        //
        // We'd like to have each rake on edge 0 or face[0]-face[1]
        // That means
        // 2
        //  \
        //   1--0
        // ordering
        //
        // to check, (1->0)x(1->2) should face towards us, or in the same
        // direction as face_centroid - poly.centroid()
        //
        // if that isn't the case, we reverse the face
        let poly_centroid = poly.centroid();
        for (face_idx, face) in poly.faces.iter_mut().enumerate() {
            poly.face_transforms
                .push(Polyhedron::face_transform(&face, &poly.vertices));
            let z = poly.face_transforms[face_idx].z.truncate();
            let face_centroid = poly.face_transforms[face_idx].w.truncate();
            // but the rake is on the pcb's bottom side, so normal points inwards
            if z.dot(face_centroid - poly_centroid) > 0.0 {
                face.reverse();
                // also re-create axes with updated faces
                poly.face_transforms[face_idx] = Polyhedron::face_transform(&face, &poly.vertices);
            }
        }
        // poly.make_path(0)?;
        Ok(poly)
    }

    /// Calculate a face's transform based on vertex coordinates
    ///
    /// If a face is numbered as
    /// ```text
    ///  2
    ///   \
    ///    1--0
    /// ```
    /// then this will produce a set of axes like
    /// ```text
    ///    y
    ///    |
    /// x<-z (into screen)
    /// ```
    /// to ensure the rake is on the `01` edge
    fn face_transform(face: &[u32], vertices: &[Vec3]) -> Mat4 {
        // x = 1->0
        let x = (vertices[face[0] as usize] - vertices[face[1] as usize]).normalize();
        // y = 1->2
        let mut y = vertices[face[2] as usize] - vertices[face[1] as usize];
        // now just orthonormalize y wrt x
        y = (y - x * y.dot(x)).normalize();
        // and have right-handed coordinate system
        let z = x.cross(y).normalize();
        let face_centroid =
            face.iter().map(|idx| vertices[*idx as usize]).sum::<Vec3>() / face.len() as f32;
        Mat4::from_cols(
            x.extend(0.0),
            y.extend(0.0),
            z.extend(0.0),
            face_centroid.extend(1.0),
        )
    }

    pub fn triangulate(&self) -> Vec<[u32; 3]> {
        let mut triangles = Vec::new();

        for face in &self.faces {
            if face.len() < 3 {
                continue;
            }

            for i in 1..face.len() - 1 {
                triangles.push([face[0], face[i], face[i + 1]]);
            }
        }

        triangles
    }

    pub fn cpu_mesh(&self) -> CpuMesh {
        CpuMesh {
            positions: Positions::F32(
                self.vertices
                    .iter()
                    .copied()
                    .map(|v| vec3(v[0], v[1], v[2]))
                    .collect(),
            ),
            indices: Indices::U32(self.triangulate().into_iter().flatten().collect()),
            ..Default::default()
        }
    }

    pub fn sphere(
        &self,
        context: &Context,
        material: PhysicalMaterial,
    ) -> exn::Result<Gm<Mesh, PhysicalMaterial>, PolyError> {
        let centroid = self.vertices.iter().sum::<Vec3>() / self.vertices.len() as f32;
        let avg_r = self
            .vertices
            .iter()
            .map(|v| (v - centroid).magnitude())
            .sum::<f32>()
            / self.vertices.len() as f32;
        let mut new_mesh = CpuMesh::sphere(8);
        let msg = PolyError("Could not create sphere for poly".to_string());
        new_mesh
            .transform(Mat4::from_scale(avg_r))
            .or_raise(|| msg.clone())?;
        new_mesh
            .transform(Mat4::from_translation(centroid))
            .or_raise(|| msg)?;
        let new_model = Gm::new(Mesh::new(context, &new_mesh), material);
        Ok(new_model)
    }

    pub fn find_path(&self, start_face_idx: usize) -> Option<Path> {
        let mut path = Vec::with_capacity(self.faces.len() + 5);
        let start_face = &self.faces[start_face_idx];

        if self.dfs(
            &mut path,
            PolygonVisit {
                face_idx: start_face_idx,
                edge: (start_face[0], start_face[1]),
            },
        ) {
            Some(path)
        } else {
            None
        }
    }

    /// Make a path, starting at the given face.
    ///
    /// If a path is found, will update self
    fn dfs(&self, path: &mut Path, visit: PolygonVisit) -> bool {
        path.push(visit);
        let face = &self.faces[visit.face_idx];
        // for dfs we want to go left first, then cycle around the polygon.
        //
        // since stack is fifo, we go counter-clockwise to have the left added last
        //
        // and we skip the last edge, since that's the one we're visiting from
        let face_edges = face
            .iter()
            .rev()
            .zip(face.iter().rev().cycle().skip(1))
            .map(|(start, end)| (*start, *end))
            .take(face.len() - 1);

        for edge in face_edges {
            if path.iter().any(|visit| edge == visit.edge) {
                continue;
            }
            // also add a preference for unvisited faces
            let n_face_idx = self
                .faces
                .iter()
                .position(|n_face| {
                    n_face
                        .iter()
                        .zip(n_face.iter().cycle().skip(1))
                        .position(|(start, end)| (*start, *end) == edge)
                        .is_some()
                })
                .unwrap();
            if path.iter().any(|visit| visit.face_idx == n_face_idx) {
                // get neighbour face id
                // Not sure why we need that though?
                // maybe to check if we are in between two edges
                //
                // so the 02 13 case
            }
            // At this point I'm seriously thinking that a recursive algorithm would be better...
        }
        false
    }
}

/// A path through faces
pub type Path = Vec<PolygonVisit>;

/// A visit event of a polygon
///
#[derive(Debug, Clone, Copy)]
pub struct PolygonVisit {
    face_idx: usize,
    edge: (u32, u32),
}

#[cfg(test)]
mod test {
    fn test_make_path_thawro() {
        let faces = vec![
            vec![0, 1, 2],
            vec![0, 1, 7, 9, 5],
            vec![0, 2, 4, 3],
            vec![0, 3, 5],
            vec![1, 2, 6],
            vec![1, 6, 7],
            vec![2, 4, 10, 15, 12, 6],
            vec![3, 4, 8],
            vec![3, 5, 11, 14, 8],
            vec![4, 8, 10],
            vec![5, 9, 11],
            vec![6, 7, 13, 12],
            vec![7, 9, 13],
            vec![8, 10, 14],
            vec![9, 11, 16, 17, 13],
            vec![10, 14, 16, 15],
            vec![11, 14, 16],
            vec![12, 13, 17],
            vec![12, 15, 17],
            vec![15, 16, 17], // this is good one?
        ];
    }
}

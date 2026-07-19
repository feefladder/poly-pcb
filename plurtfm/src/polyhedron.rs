use derive_more::Display;
use exn::{OptionExt, ResultExt};
use log::{info, warn};
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
    pub start: u32,
    pub end: u32,
}

impl Edge {
    fn rev(&self) -> Edge {
        Edge {
            start: self.end,
            end: self.start,
        }
    }
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

        poly.find_path(0);
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
        // x = 0->1
        let x = (vertices[face[1] as usize] - vertices[face[0] as usize]).normalize();
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

    pub fn find_path(&mut self, start_face_idx: usize) -> Option<Path> {
        let mut path = Vec::with_capacity(self.faces.len() + 5);
        let mut visited = vec![false; self.faces.len()];
        let start_face = &self.faces[start_face_idx];

        if self.dfs(
            &mut path,
            &mut visited,
            PolygonVisit {
                face_idx: start_face_idx,
                enter: (start_face[0], start_face[1]),
            },
        ) {
            info!("found path {path:?}");

            // update face transforms
            for visit in &path {
                self.face_transforms[visit.face_idx] =
                    Self::face_transform(&self.faces[visit.face_idx], &self.vertices);
            }
            Some(path)
        } else {
            warn!("did not find path for {}", self.name);
            None
        }
    }

    /// Get the nth edge of this face in clockwise direction
    /// let's say these are face indices:
    /// ```text
    /// 2
    ///  \
    ///   1--0
    /// ```
    /// then edge_from_face(0) will give (face[0], face[1])
    fn edge_from_face(&self, face_idx: usize, edge_n: usize) -> Edge {
        Edge {
            start: self.faces[face_idx][edge_n],
            end: self.faces[face_idx][(edge_n + 1) % self.faces[face_idx].len()],
        }
    }

    fn edge_n_on_face(&self, face_idx: usize, edge: (u32, u32)) -> usize {
        face.iter().position(|i| *i == visit.enter.1).unwrap()
    }

    fn other_face(&self, my_face: usize, flip_edge: usize) -> usize {
        42
    }

    fn face_edges(&self, face_idx: usize, start_idx: usize) -> impl Iterator<Item = (u32, u32)> {
        let face = &self.faces[face_idx];
        face.iter()
            .zip(face.iter().cycle().skip(1))
            .map(|(start, end)| (*start, *end))
            .skip(start_idx)
    }

    /// Make a path, starting at the given face.
    ///
    /// If a path is found, will update self
    fn dfs(&mut self, path: &mut Path, visited: &mut Vec<bool>, visit: PolygonVisit) -> bool {
        // So I mean, this works, but it's illegible. So what would be nice is
        // to have some face/edge-related functions on polyhedron....
        //
        // and store something edge-like, since idk.. they don't have _that_
        // much data and we need dihedral angle-thingies anyways in order to
        // tell people to not solder a led on sharp angles.
        //
        // Like yes, everything can be calculated, but trading memory for computation can do..
        //
        // So what is often done?
        // searching the path is kind of a necessary evil, since we want to have it ordered
        //
        // but the sad thing is this discrepancy between (face,id)<->(e0,e1) and
        // altogether I think the ordered-ness is very stokes and that's nice,
        // but it's kind of an unneeded extra requirement in an already rather
        // hard algorithm to also have to consider reversing edges?
        //
        // anyways, let's add a poly.edge(face_idx, id) or something?
        // it almost feels like having some silly type that is Face(Vec<u32>) just to be able to nicen the zip(skip) iterator hell?
        if !visited[visit.face_idx] {
            // rotate poly so we're entering on edge 0-1
            visited[visit.face_idx] = true;
            // winding direction is the same, so we only need to find the first
            let rotate_amount = self.faces[visit.face_idx]
                .iter()
                .position(|vidx| *vidx == visit.enter.0)
                .unwrap();
            self.faces[visit.face_idx].rotate_left(rotate_amount);
        }

        // success condition
        if visited.iter().all(|v| *v) {
            let f = &self.faces[visit.face_idx];
            path.push(visit.exit((f[1], f[2])));
            return true;
        }

        // current face
        let face = self.faces[visit.face_idx].to_owned();
        // for dfs we want to go left first, then cycle around the polygon.
        //
        // Also if we're revisiting the polygon, then only check next n edges
        let face_edges = self
            .face_edges(
                visit.face_idx,
                face.iter().position(|i| *i == visit.enter.1).unwrap(),
            )
            .collect::<Vec<_>>();
        let mut revisits = Vec::new();
        for edge in face_edges {
            let rev = (edge.1, edge.0);
            if path.iter().any(|crossing| crossing.enter == rev) {
                continue;
            }
            // check if we're visiting an already-crossed polygon
            //
            // here we check for all polyhedron faces if it contains this edge, which is kinda inefficient
            let n_face_idx = self
                .faces
                .iter()
                .enumerate()
                .position(|(i, n_face)| {
                    i != visit.face_idx
                        && n_face
                            .iter()
                            .zip(n_face.iter().cycle().skip(1))
                            .position(|(start, end)| (*start, *end) == rev)
                            .is_some()
                })
                .unwrap();
            if let Some(prev_visit_idx) = path
                .iter()
                .rposition(|a_visit| a_visit.face_idx == n_face_idx)
            {
                // get neighbour face id
                // Not sure why we need that though?
                // maybe to check if we are in between two edges
                //
                // so the 02 13 case, which says 1 is illegal on an existing 02
                // For that, we only need to check if any _later_ edges are in the path
                // Since we rotate on first visit, this is correct
                //
                // The only BIG problem is that we're checking entering edge and don't know exiting edge
                // except... that's ofcofc the next-in-line on the path
                let prev_crossing = path[prev_visit_idx];

                if path.iter().any(|crossing| crossing.enter == edge) {
                    continue;
                }

                let n_face = &self.faces[n_face_idx];
                let n = n_face
                    .iter()
                    .position(|vidx| prev_crossing.exit.0 == *vidx)
                    .unwrap();
                // crossing rule:
                if n < face.len() - 1
                    && !(n..n_face.len()).any(|test_edge_start| {
                        // check if path already contains that edge somewhere
                        //
                        // edges in path are stored clockwise relative to their face, so when searching,
                        // need to search for the counter-clockwise equivalent. from this face
                        let edge = (
                            n_face[(test_edge_start + 1) % n_face.len()],
                            n_face[test_edge_start],
                        );
                        path.iter().any(|v| v.exit == edge)
                    })
                {
                    revisits.push((
                        visit.exit(edge),
                        PolygonVisit {
                            face_idx: n_face_idx,
                            enter: rev,
                        },
                    ));
                }
            } else {
                path.push(visit.exit(edge));
                if self.dfs(
                    path,
                    visited,
                    PolygonVisit {
                        face_idx: n_face_idx,
                        enter: rev,
                    },
                ) {
                    return true;
                }
                // we want to closely hug visited pcbs, so break before diverging
                break;
            }
        }
        for revisit in revisits {
            path.push(revisit.0);
            if self.dfs(path, visited, revisit.1) {
                return true;
            }
        }

        path.pop();
        // if we've rotated the face, we were the ones visiting
        if self.faces[visit.face_idx][0] == visit.enter.0 {
            visited[visit.face_idx] = false;
        }

        false
    }
}

/// A path through faces
pub type Path = Vec<PolygonCrossing>;

/// A visit event of a polygon
///
#[derive(Debug, Clone, Copy)]
pub struct PolygonVisit {
    face_idx: usize,
    enter: (u32, u32),
}

impl PolygonVisit {
    fn exit(self, exit: (u32, u32)) -> PolygonCrossing {
        PolygonCrossing {
            face_idx: self.face_idx,
            enter: self.enter,
            exit,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PolygonCrossing {
    face_idx: usize,
    enter: (u32, u32),
    exit: (u32, u32),
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_make_path_tet() {
        // stole ascii representation of tetrahedron from here: https://bendwavy.org/klitzing/explain/dynkin.htm#fundamental-simplex
        //     o             a
        //     r             R     left:  general Goursat tetrahedron (any node is 3-valent)
        //   p o t         P c T   right: rewritten by assigning position characters
        //   q   s         Q   S
        // o   u   o     b   U   d
        //     1
        //     r
        //   p 0 t
        //   q   s
        // 2   u   3
        Polyhedron {
            name: "tet".to_string(),
            vertices: vec![
                vec3(1.0, 1.0, 1.0),
                vec3(-1.0, -1.0, 1.0),
                vec3(1.0, -1.0, -1.0),
                vec3(-1.0, 1.0, -1.0),
            ],
            faces: vec![vec![1, 0, 2], vec![1, 2, 3], vec![1, 3, 0], vec![0, 3, 2]],
            face_transforms: Vec::new(),
            edge_path: Vec::new(),
        };

        assert_eq!(2 + 2, 5);
    }

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

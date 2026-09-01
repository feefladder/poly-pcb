use derive_more::Display;
use exn::{OptionExt, ResultExt};
use log::{debug, error, info, warn};
use rusqlite::Connection;
use std::{collections::HashMap, error::Error};
use three_d::*;

use crate::design::PcbPath;

#[derive(Debug, Clone, PartialEq)]
pub struct Polyhedron {
    pub name: String,
    /// vertices in 3d space
    pub vertices: Vec<Vec3>,
    /// list of faces, with their vertices
    ///
    /// This is u32 because [`three_d::Indices`] is strongly typed and usize is u32 on wasm.
    pub faces: Vec<Vec<u32>>,
    /// Polyhedron edges
    ///
    /// These know what faces they connect with dihedral angle
    pub edges: HashMap<Edge, PolyEdge>,
    // /// Graph between faces with edge index.
    // ///
    // /// So to get the edge that connects face 0 to face ?? over edge 2, do
    // /// even though that's bs and we'd just need to
    // /// ```
    // /// poly.face_graph[0][2]
    // /// ```
    // pub face_graph: Vec<BTreeMap<usize, usize>>,
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
    pub edge_path: Vec<PolygonCrossing>,
}

/// An edge of a polyhedron
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Edge {
    pub start: u32,
    pub end: u32,
}

/// An edge of a polyhedron that knows where it is
///
/// because it knows where it isn't
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PolyEdge {
    faces: [usize; 2],
    dihedral: f32,
}

impl Edge {
    fn rev(self) -> Self {
        Edge {
            start: self.end,
            end: self.start,
        }
    }

    fn sorted(self) -> Self {
        if self.end < self.start {
            self.rev()
        } else {
            self
        }
    }
}

impl From<(u32, u32)> for Edge {
    fn from(value: (u32, u32)) -> Self {
        Self {
            start: value.0,
            end: value.1,
        }
    }
}

#[derive(Debug, Display, Clone)]
pub struct PolyError(String);
impl Error for PolyError {}
impl From<String> for PolyError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

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

    pub fn face_centroid(&self, face_idx: usize) -> Vec3 {
        self.face_transforms[face_idx].w.truncate()
    }

    pub fn edge_centroid(&self, edge: Edge) -> Vec3 {
        (self.vertices[edge.start as usize] + self.vertices[edge.end as usize]) / 2.0
    }

    pub fn face_normal(&self, face_idx: usize) -> Vec3 {
        self.face_transforms[face_idx].z.truncate()
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
            edges: HashMap::new(),
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
                    row.get::<_, u32>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, u32>(2)?,
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
            .map(|slice| slice.iter().map(|(_, vertex_id, _)| *vertex_id).collect())
            .collect();

        let mut stmt = conn
            .prepare(
                "
            SELECT
                e.id,
                MIN(l.id1) AS v0,
                MAX(l.id1) AS v1,
                e.dihedral,
                e.face1,
                e.face2
            FROM Edge AS e
            JOIN lattice AS l
                ON e.poly = l.poly
               AND e.id   = l.id2
            WHERE e.poly = ?
              AND l.dim1 = 0
              AND l.dim2 = 1
            GROUP BY e.id
            ORDER BY e.id;
            ",
            )
            .or_raise(|| PolyError(format!("Could not load polyhedron {longname}")))?;

        poly.edges = stmt
            .query_map([poly_id], |row| {
                let edge = Edge {
                    start: row.get::<_, u32>(1)?,
                    end: row.get::<_, u32>(2)?,
                };
                Ok((
                    edge,
                    PolyEdge {
                        faces: [
                            row.get::<_, u32>(4)?.try_into().unwrap(),
                            row.get::<_, u32>(5)?.try_into().unwrap(),
                        ],
                        dihedral: row.get::<_, f32>(3)?,
                    },
                ))
            })
            .or_raise(|| PolyError(format!("Could not load edges for polyhedron {longname}")))?
            .collect::<Result<HashMap<Edge, _>, _>>()
            .or_raise(|| PolyError(format!("Could not decode edges for {longname}")))?;

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
                .push(Polyhedron::face_transform(face, &poly.vertices));
            let z = poly.face_transforms[face_idx].z.truncate();
            let face_centroid = poly.face_transforms[face_idx].w.truncate();
            // but the rake is on the pcb's bottom side, so normal points inwards
            if z.dot(face_centroid - poly_centroid) > 0.0 {
                face.reverse();
                // also re-create axes with updated faces
                poly.face_transforms[face_idx] = Polyhedron::face_transform(face, &poly.vertices);
            }
        }

        // if let Some(p) = poly.find_path(0) {
        //     poly.edge_path = p;
        // }
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

    /// Update face transforms based on faces
    pub fn update_transforms(&mut self) {
        for (i, transform) in self.face_transforms.iter_mut().enumerate() {
            *transform = Self::face_transform(&self.faces[i], &self.vertices)
        }
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

    pub fn mean_r(&self) -> f32 {
        let centroid = self.vertices.iter().sum::<Vec3>() / self.vertices.len() as f32;
        self.vertices
            .iter()
            .map(|v| (v - centroid).magnitude())
            .sum::<f32>()
            / self.vertices.len() as f32
    }

    /// Create a sphere with average radius of the polyhedron
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

    /// Apply the given path, clearing the current one
    ///
    /// It is very possible that an impossible path is specified, in which case the number of successful turns is returned
    ///
    /// currently doesn't check very much, this will be improved as considered non-breaking changes
    pub fn apply_path(&mut self, path: &PcbPath) -> Result<(), usize> {
        debug!("applying path {path:?}");
        self.edge_path.clear();
        let start_face = self
            .iter_ngon(path.start_ngon)
            .nth(path.start_nth)
            .ok_or(0usize)?;
        if path.turns.is_empty() {
            //
            return Ok(());
        } else if path.turns.len() == 1 {
            let face_idx = start_face;
            self.edge_path.push(PolygonCrossing {
                face_idx,
                enter: self.edge_from_face(face_idx, 0),
                exit: self.edge_from_face(face_idx, 1),
            });
            return Ok(());
        }
        // we make a visited array because we rotate polygons on first visit
        let mut visited = vec![false; self.faces.len()];
        visited[start_face] = true;
        let n = path.turns[0] + 1;
        let exit = self.edge_from_face(start_face, n);

        self.edge_path.push(PolygonCrossing {
            face_idx: start_face,
            enter: self.edge_from_face(start_face, 0),
            exit,
        });
        let mut visit = PolygonVisit {
            face_idx: self.other_face(start_face, n),
            enter: exit.rev(),
        };
        for (i, turn) in path.turns.iter().enumerate().skip(1) {
            // rotate the face so it points at the enter
            // this is also in dfs, so maybe generify somehow?
            if !visited[visit.face_idx] {
                // rotate poly so we're entering on edge 0-1
                visited[visit.face_idx] = true;
                let rotate_amount = self.edge_n_on_face(visit.face_idx, visit.enter).unwrap();
                self.faces[visit.face_idx].rotate_left(rotate_amount);
                // then set next visit
                let n = turn + 1;
                let exit = self.edge_from_face(visit.face_idx, n);
                self.edge_path.push(visit.exit(exit));
                visit = PolygonVisit {
                    face_idx: self.other_face(visit.face_idx, n),
                    enter: exit.rev(),
                }
            } else {
                // re-entering a face, so now turn is counterclockwise
                // since for any direction we'd have to solderjumper, we don't add 1
                let n_enter = self.edge_n_on_face(visit.face_idx, visit.enter).unwrap();
                let n_exit = n_enter.checked_sub(*turn).ok_or(i)?;
                let exit = self.edge_from_face(visit.face_idx, n_exit);
                self.edge_path.push(visit.exit(exit));
                visit = PolygonVisit {
                    face_idx: self.other_face(visit.face_idx, n_exit),
                    enter: exit.rev(),
                };
            }
        }
        let n_enter = self.edge_n_on_face(visit.face_idx, visit.enter).unwrap();
        // self.edge_path
        //     .push(visit.exit(self.edge_from_face(visit.face_idx, n_enter + 1)));
        self.update_transforms();
        Ok(())
    }

    pub fn current_path(&self) -> Result<PcbPath, PcbPath> {
        let first_face = self.edge_path.first().ok_or(PcbPath::default())?;
        debug!("first path face: {first_face:?}");
        let start_ngon = self.faces[first_face.face_idx].len();
        debug!("start_ngon: {start_ngon:?}");
        let start_nth = self
            .iter_ngon(start_ngon)
            .position(|i| i == first_face.face_idx)
            .ok_or(PcbPath {
                start_ngon,
                ..Default::default()
            })?;
        debug!("start nth: {start_nth}");
        let mut turns = Vec::with_capacity(self.edge_path.len());
        for PolygonCrossing {
            face_idx,
            enter,
            exit,
        } in &self.edge_path
        {
            if *enter == self.edge_from_face(*face_idx, 0) {
                // first time visiting
                turns.push(
                    self.edge_n_on_face(*face_idx, *exit)
                        .unwrap()
                        .saturating_sub(1),
                );
            } else {
                let n_enter = self.edge_n_on_face(*face_idx, *enter).unwrap();
                let n_exit = self.edge_n_on_face(*face_idx, *exit).unwrap();
                let Some(turn) = n_enter.checked_sub(n_exit) else {
                    return Err(PcbPath {
                        start_ngon,
                        start_nth,
                        turns,
                    });
                };
                turns.push(turn);
            }
        }
        Ok(PcbPath {
            start_ngon,
            start_nth,
            turns,
        })
    }

    /// Find a path starting at the given face
    pub fn find_path(&mut self, start_face_idx: usize) -> Option<()> {
        let mut path = Vec::with_capacity(self.faces.len() + 5);
        let mut visited = vec![false; self.faces.len()];
        let start_face = &self.faces[start_face_idx];

        if self.dfs(
            &mut path,
            &mut (0..self.faces.len()).map(|_i| Vec::new()).collect(),
            &mut visited,
            PolygonVisit {
                face_idx: start_face_idx,
                enter: (start_face[0], start_face[1]).into(),
            },
        ) {
            info!(
                "found path {:?}",
                path.iter().map(|pc| pc.face_idx).collect::<Vec<_>>()
            );
            self.edge_path = path;

            // update face transforms
            self.update_transforms();
            Some(())
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
    #[inline]
    fn edge_from_face(&self, face_idx: usize, edge_n: usize) -> Edge {
        let f = &self.faces[face_idx];
        Edge {
            start: f[edge_n % f.len()],
            end: f[(edge_n + 1) % f.len()],
        }
    }

    /// Get the edge that connects this face to the other face
    ///
    /// The returned direction is the direction as seen from this face
    fn edge_from_two_faces(&self, face: usize, other_face: usize) -> exn::Result<Edge, PolyError> {
        self.edges
            .iter()
            .find(|(_k, v)| v.faces == [face, other_face] || v.faces == [other_face, face])
            .map(|(k, _v)| {
                if self.face_edges(face, 0).any(|e| e == *k) {
                    *k
                } else {
                    k.rev()
                }
            })
            .ok_or_raise(|| format!("No edge connecting face {} with {}", face, other_face).into())
    }

    /// get n of an edgeedge_from_two_faces
    fn edge_n_on_face(&self, face_idx: usize, edge: Edge) -> Option<usize> {
        self.face_edges(face_idx, 0).position(|e| e == edge)
    }

    /// Get the face on the other side of the nth edge
    #[inline]
    fn other_face(&self, my_face: usize, flip_edge: usize) -> usize {
        let edge = self.edge_from_face(my_face, flip_edge).rev();

        let faces = self.edges[&edge.sorted()].faces;
        if faces[0] != my_face {
            faces[0]
        } else {
            faces[1]
        }
    }

    /// Get an iterator over all edges in this face
    #[inline]
    fn face_edges(&self, face_idx: usize, start_idx: usize) -> impl Iterator<Item = Edge> {
        let face = &self.faces[face_idx];
        face.iter()
            .zip(face.iter().cycle().skip(1))
            .map(|(start, end)| (*start, *end).into())
            .skip(start_idx)
    }

    /// Make a path, starting at the given face.
    ///
    /// If a path is found, will update self
    ///
    /// path is a path as found on self, and face_path_index is an index of where in the path a given face can be found
    fn dfs(
        &mut self,
        path: &mut Path,
        face_path_index: &mut Vec<Vec<usize>>,
        visited: &mut Vec<bool>,
        visit: PolygonVisit,
    ) -> bool {
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
        assert!(
            !visited[visit.face_idx],
            "should call revisit_dfs when revisiting {path:?}"
        );
        // rotate poly so we're entering on edge 0-1
        visited[visit.face_idx] = true;
        let rotate_amount = self.edge_n_on_face(visit.face_idx, visit.enter).unwrap();
        self.faces[visit.face_idx].rotate_left(rotate_amount);
        // success condition: all faces visited (this can only happen on first visit)
        if visited.iter().all(|v| *v) {
            // add current visit
            // exit is first possible exit
            path.push(visit.exit(self.edge_from_face(visit.face_idx, 1)));
            return true;
        }

        // current face
        // let face = &self.faces[visit.face_idx];
        // for dfs we want to go left first, then cycle around the polygon.
        //
        // Also if we're revisiting the polygon, then only check next n edges
        let face_edges = self.face_edges(visit.face_idx, 0).collect::<Vec<_>>();
        let mut revisits = Vec::new();
        let mut n_new_faces = 0;
        // skip entering edge and only test two??
        // no, that's wrong, we only test two _past the last revisit_
        for (i, edge) in face_edges.iter().enumerate().skip(1) {
            // check if we're visiting an already-crossed polygon
            //
            // here we check for all polyhedron faces if it contains this edge, which is kinda inefficient
            let n_face_idx = self.other_face(visit.face_idx, i);

            // what's this again?
            // something like checking if it's already entered earlier
            if face_path_index[n_face_idx]
                .iter()
                .any(|&crossing_idx| path[crossing_idx].enter == edge.rev())
            {
                continue;
            }
            // If this face has already been visited, check the crossing rule: 0-2 1-3 is not allowed
            //
            // wait... on hexagon, 01  45  23 is allowed actually and below would disregard that
            // even though in the order thingy, that would make more sense
            // ```
            //    3
            // 2 /-\ 4
            // 1 \_/ 5
            //    0
            // ```
            // because it like goes around the polygon and then will come back at 5
            // and the best heuristic there is actually to try to exit asap as well, in stead of searching from 2
            // but whatevs
            //
            // maybe it'd also be very fast to check if it's visited and a triangle
            if !face_path_index[n_face_idx].is_empty() {
                // triangle shortcut (they can't be visited twice)
                if self.faces[n_face_idx].len() == 3 {
                    continue;
                }
                // get neighbour face id
                // Not sure why we need that though?
                // maybe to check if we are in between two edges
                //
                // so the 02 13 case, which says 1 is illegal on an existing 02
                // For that, we only need to check if any _later_ edges are in the path
                // Since we rotate on first visit, this is correct

                // crossing rule:
                // 02 disallows 13
                // e.g. 0 enter and 2 exit disallows 1 as enter
                //
                // but also,
                if !face_path_index[n_face_idx].iter().any(|&i| {
                    let cr = path[i];
                    (self.edge_n_on_face(n_face_idx, cr.enter).unwrap() < i
                        && self.edge_n_on_face(n_face_idx, cr.exit).unwrap() > i)
                        || cr.enter == edge.rev()
                        || cr.exit == edge.rev()
                }) {
                    revisits.push((
                        visit.exit(*edge),
                        PolygonVisit {
                            face_idx: n_face_idx,
                            enter: edge.rev(),
                        },
                    ));
                }
            } else {
                face_path_index[visit.face_idx].push(path.len());
                path.push(visit.exit(*edge));
                if self.dfs(
                    path,
                    face_path_index,
                    visited,
                    PolygonVisit {
                        face_idx: n_face_idx,
                        enter: edge.rev(),
                    },
                ) {
                    return true;
                }
                // we want to closely hug visited pcbs, so break before diverging
                // this greatly speeds up search time, but kinda sad
                n_new_faces += 1;
                if n_new_faces == 2 {
                    break;
                }
            }
        }
        for revisit in revisits {
            face_path_index[revisit.0.face_idx].push(path.len());
            path.push(revisit.0);
            // debug!(
            //     "revisiting face {:?} which has been visited by {:?}",
            //     revisit.1,
            //     face_path_index[revisit.1.face_idx]
            //         .iter()
            //         .map(|&i| path[i])
            //         .collect::<Vec<_>>()
            // );
            // So here it'd be better to do some alternative "I'm revisiting a face!"-type dfs
            if self.revisit_dfs(path, face_path_index, visited, revisit.1) {
                return true;
            } else {
                face_path_index[revisit.0.face_idx].pop();
                path.pop();
            }
        }

        if let Some(v) = path.pop() {
            face_path_index[v.face_idx].pop();
        }

        // we were the ones visiting
        visited[visit.face_idx] = false;
        false
    }

    fn revisit_dfs(
        &mut self,
        path: &mut Path,
        face_path_index: &mut Vec<Vec<usize>>,
        visited: &mut Vec<bool>,
        visit: PolygonVisit,
    ) -> bool {
        // ```
        //    3
        // 2 /-\ 4
        // 1 \_/ 5
        //    0
        // ```
        // Since we're spiralling, the most likely face we'd enter on is 5
        // then we want to check 2->4 in that order
        //
        // If in stead, we'd be entering through 3, we'd dfs 2,5,4
        // even though bla..
        // what's bla?
        // bla is that going in 5 direction will kind of force a spiral, because the 4-face needs to be visited and then is locked in
        // So... 5 is actually forcing a spiral,
        // but 4 would be forcing a spiral the other way around
        // so it's all a bit suboptimal to be entering on 3
        let fidx = visit.face_idx;
        let n = self.edge_n_on_face(fidx, visit.enter).unwrap();
        debug!("revisiting face {:?} from edge {n}", fidx);
        let largest_smaller_n = face_path_index[fidx]
            .iter()
            .filter_map(|&v| {
                let cr = path[v];
                let e = self.edge_n_on_face(fidx, cr.exit).unwrap();
                if e < n { Some(e) } else { None }
            })
            .max()
            .unwrap_or(n);
        // else {
        //     error!(
        //         "Could not find largest smaller n for {:?}, {:?}",
        //         self.edge_n_on_face(fidx, visit.enter),
        //         face_path_index[fidx]
        //             .iter()
        //             .map(|&v| {
        //                 let c = path[v];
        //                 (
        //                     // self.edge_n_on_face(fidx, c.enter),
        //                     self.edge_n_on_face(fidx, c.exit),
        //                 )
        //             })
        //             .collect::<Vec<_>>()
        //     );
        //     return false;
        // };
        // If there's no larger n, we'll try till the end of the face
        let smallest_larger_n = face_path_index[fidx]
            .iter()
            .filter_map(|&v| {
                let cr = path[v];
                let e = self.edge_n_on_face(fidx, cr.exit).unwrap();
                if e > n { Some(e) } else { None }
            })
            .min()
            .unwrap_or(self.faces[fidx].len());
        for edge_n in (largest_smaller_n..n).chain(n + 1..smallest_larger_n) {
            let e = self.edge_from_face(fidx, edge_n);
            let n_face_idx = self.other_face(fidx, edge_n);
            if face_path_index[n_face_idx].is_empty() {
                face_path_index[fidx].push(path.len());
                path.push(visit.exit(e));
                if self.dfs(
                    path,
                    face_path_index,
                    visited,
                    PolygonVisit {
                        face_idx: n_face_idx,
                        enter: e.rev(),
                    },
                ) {
                    return true;
                } else {
                    path.pop();
                    face_path_index[fidx].pop();
                }
            } else {
                // revisiting from a revisit, that's sad
                // so just give up?

                // if let Some(v) = path.pop() {
                //     face_path_index[v.face_idx].pop();
                // }
                // return false;
            }
        }
        // it could be that above for loop didn't run, so that's also a fail
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
    enter: Edge,
}

impl PolygonVisit {
    fn exit(self, exit: Edge) -> PolygonCrossing {
        PolygonCrossing {
            face_idx: self.face_idx,
            enter: self.enter,
            exit,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolygonCrossing {
    pub face_idx: usize,
    pub enter: Edge,
    pub exit: Edge,
}

#[cfg(test)]
mod test {
    use log::error;

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
        let mut tet = Polyhedron {
            name: "tet".to_string(),
            vertices: vec![
                vec3(1.0, 1.0, 1.0),
                vec3(-1.0, -1.0, 1.0),
                vec3(1.0, -1.0, -1.0),
                vec3(-1.0, 1.0, -1.0),
            ],
            //             0                 1               2              3
            faces: vec![vec![1, 0, 2], vec![1, 2, 3], vec![1, 3, 0], vec![0, 3, 2]],
            // follow pqrstu
            edges: HashMap::from([
                (
                    (1, 2).into(),
                    PolyEdge {
                        // p 0
                        faces: [0, 1],
                        dihedral: 70.5287793655093,
                    },
                ),
                (
                    (0, 2).into(),
                    PolyEdge {
                        // q 1
                        faces: [0, 3],
                        dihedral: 70.5287793655093,
                    },
                ),
                (
                    (0, 1).into(),
                    PolyEdge {
                        // r 2
                        faces: [0, 2],
                        dihedral: 70.5287793655093,
                    },
                ),
                (
                    (0, 3).into(),
                    PolyEdge {
                        // s 3
                        faces: [2, 3],
                        dihedral: 70.5287793655093,
                    },
                ),
                (
                    (1, 3).into(),
                    PolyEdge {
                        // t 4
                        faces: [1, 2],
                        dihedral: 70.5287793655093,
                    },
                ),
                (
                    (2, 3).into(),
                    PolyEdge {
                        // u 5
                        faces: [1, 3],
                        dihedral: 70.5287793655093,
                    },
                ),
            ]),
            // face_graph: vec![
            //     BTreeMap::from([(1, 0), (2, 2), (3, 1)]),
            //     BTreeMap::from([(0, 0), (2, 4), (3, 5)]),
            //     BTreeMap::from([(0, 2), (1, 4), (3, 3)]),
            //     BTreeMap::from([(0, 1), (1, 5), (2, 3)]),
            // ],
            face_transforms: vec![Mat4::zero(); 4],
            edge_path: Vec::new(),
        };
        tet.find_path(0).unwrap();
        assert_eq!(
            tet.edge_path,
            vec![
                PolygonCrossing {
                    face_idx: 0,
                    enter: (1, 0).into(),
                    exit: (0, 2).into(),
                },
                PolygonCrossing {
                    face_idx: 3,
                    enter: (2, 0).into(),
                    exit: (0, 3).into()
                },
                PolygonCrossing {
                    face_idx: 2,
                    enter: (3, 0).into(),
                    exit: (1, 3).into()
                },
                PolygonCrossing {
                    face_idx: 1,
                    enter: (3, 1).into(),
                    exit: (1, 2).into()
                }
            ]
        );

        // let conn = rusqlite::Connection::open("web/src/assets/polydb.sqlite3").expect("open db");
        // let t = Polyhedron::load(&conn, "tetrahedron").unwrap();
        // assert_eq!(tet, t);
    }

    // #[test]
    // fn test_make_path_bilb() {
    //     env_logger::builder().is_test(true).init();
    //     let conn = rusqlite::Connection::open("web/src/assets/polydb.sqlite3").expect("open db");
    //     let ps = conn
    //         .prepare("SELECT longname from Polyhedron")
    //         .unwrap()
    //         .query_map([], |row| row.get::<_, String>(0))
    //         .unwrap()
    //         .collect::<Result<Vec<String>, _>>()
    //         .unwrap();
    //     for p in ps {
    //         error!("{p}");
    //         let mut poly = Polyhedron::load(&conn, &p).expect("load bilb");
    //         if let Some(edge_path) = poly.find_path(0) {
    //             poly.edge_path = edge_path;
    //         }
    //     }
    // }

    #[test]
    // #[ignore = "takes long"]
    fn test_truncated_cube() {
        let _ = env_logger::builder().is_test(true).try_init();
        let conn = rusqlite::Connection::open("web/src/assets/polydb.sqlite3").expect("open db");
        let tc = Polyhedron::load(&conn, "truncated cube").unwrap();
        assert_eq!(
            tc.edge_path.iter().map(|c| c.face_idx).collect::<Vec<_>>(),
            [0, 6, 4, 2, 1, 3, 5, 7, 8, 11, 13, 9, 4, 13, 10, 1, 13, 12]
        );
    }

    #[test]
    #[ignore = "takes long"]
    fn test_snub_dodecahedron() {
        let _ = env_logger::builder().is_test(true).try_init();
        let conn = rusqlite::Connection::open("web/src/assets/polydb.sqlite3").expect("open db");
        let tc = Polyhedron::load(&conn, "snub dodecahedron (R)").unwrap();
        assert_eq!(
            tc.edge_path.iter().map(|c| c.face_idx).collect::<Vec<_>>(),
            vec![
                0, 1, 6, 7, 5, 9, 8, 2, 4, 3, 11, 10, 15, 14, 19, 18, 30, 31, 21, 22, 20, 26, 13,
                12, 17, 16, 28, 29, 24, 25, 23, 27, 41, 47, 46, 51, 50, 35, 34, 37, 53, 42, 38, 39,
                45, 44, 49, 48, 33, 32, 36, 52, 43, 40, 59, 58, 65, 67, 73, 84, 80, 91, 90, 81, 87,
                89, 85, 65, 79, 76, 77, 63, 62, 69, 68, 36, 54, 66, 72, 82, 83, 88, 86, 78, 64, 56,
                57, 74, 75, 61, 60, 71, 70, 37, 55
            ]
        );
    }

    #[test]
    #[ignore = "takes long"]
    fn test_triaugmented_truncated_dodecahedron() {
        let _ = env_logger::builder().is_test(true).try_init();
        let conn = rusqlite::Connection::open("web/src/assets/polydb.sqlite3").expect("open db");
        let tc = Polyhedron::load(&conn, "triaugmented truncated dodecahedron").unwrap();
        assert_eq!(
            tc.edge_path.iter().map(|c| c.face_idx).collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    #[test]
    // #[ignore = "takes long"]
    fn test_rhombicuboctahedron() {
        let _ = env_logger::builder().is_test(true).try_init();
        let conn = rusqlite::Connection::open("web/src/assets/polydb.sqlite3").expect("open db");
        let tc = Polyhedron::load(&conn, "rhombicuboctahedron").unwrap();
        info!(
            "rhombicuboctahedron has first face an {}-gon",
            tc.faces[0].len()
        );
        assert_eq!(
            tc.edge_path.iter().map(|c| c.face_idx).collect::<Vec<_>>(),
            vec![
                0, 4, 8, 6, 7, 2, 3, 1, 5, 11, 13, 15, 16, 14, 12, 10, 9, 17, 19, 21, 23, 24, 22,
                20, 25, 17, 18
            ]
        );
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

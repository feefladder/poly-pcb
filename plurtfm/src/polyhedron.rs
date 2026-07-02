use derive_more::Display;
use exn::ResultExt;
use log::info;
use rusqlite::Connection;
use std::{collections::HashMap, error::Error};
use three_d::*;

#[derive(Debug, Clone, PartialEq)]
pub struct Polyhedron {
    pub name: String,
    /// vertices in 3d space
    pub vertices: Vec<Vec3>,
    /// list of faces, with their vertices
    pub faces: Vec<Vec<u32>>,
    /// per-face transforms
    /// ```
    /// // put stl on face `i`:
    /// stl.transform(poly.face_transforms[i])
    /// ```
    pub face_transforms: Vec<Mat4>,
}

#[derive(Debug, Display, Clone)]
pub struct PolyError(String);
impl Error for PolyError {}

impl Polyhedron {
    pub fn iter_ngon(&self, n_sides: usize) -> impl Iterator<Item = usize> {
        self.faces
            .iter()
            .enumerate()
            .filter(move |(i, f)| f.len() == n_sides)
            .map(|(i, _)| i)
    }

    pub fn load(conn: &Connection, longname: &str) -> rusqlite::Result<Polyhedron> {
        let poly_id: i64 = conn.query_row(
            "SELECT id FROM Polyhedron WHERE longname = ?",
            [longname],
            |row| row.get(0),
        )?;

        let mut stmt = conn.prepare(
            "
        SELECT id, x, y, z
        FROM Vertex
        WHERE poly = ?
        ORDER BY id
        ",
        )?;

        let mut vertex_map = HashMap::new();
        let mut vertices = Vec::new();

        let rows = stmt.query_map([poly_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, f32>(1)?,
                row.get::<_, f32>(2)?,
                row.get::<_, f32>(3)?,
            ))
        })?;

        for (idx, row) in rows.enumerate() {
            let (vertex_id, x, y, z) = row?;
            vertex_map.insert(vertex_id, idx as u32);
            vertices.push(vec3(x, y, z));
        }

        let poly_centroid = vertices.iter().sum::<Vec3>() / vertices.len() as f32;

        let mut stmt = conn.prepare(
            "
        SELECT face, vertex, idx
        FROM Polygon
        WHERE poly = ?
        ORDER BY face, idx
        ",
        )?;

        let mut faces_raw: HashMap<i64, Vec<(i64, i64)>> = HashMap::new();

        let rows = stmt.query_map([poly_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;

        for row in rows {
            let (face_id, vertex_id, idx) = row?;
            faces_raw.entry(face_id).or_default().push((idx, vertex_id));
        }

        let mut face_ids: Vec<_> = faces_raw.keys().copied().collect();
        face_ids.sort();

        let mut faces: Vec<Vec<u32>> = Vec::with_capacity(face_ids.len());

        for face_id in face_ids {
            let mut vertices_for_face = faces_raw.remove(&face_id).unwrap();

            vertices_for_face.sort_by_key(|(idx, _)| *idx);

            faces.push(
                vertices_for_face
                    .into_iter()
                    .map(|(_, vertex_id)| vertex_map[&vertex_id])
                    .collect(),
            );
        }

        let mut face_transforms = Vec::with_capacity(faces.len());

        for face in faces.iter() {
            let face_centroid: Vec3 =
                face.iter().map(|idx| vertices[*idx as usize]).sum::<Vec3>() / face.len() as f32;

            let v0 = vertices[face[0] as usize];
            let v1 = vertices[face[1] as usize];
            let v2 = vertices[face[2] as usize];

            let x = (v1 - v0).normalize();
            // Local y-axis
            let mut y = (v2 - v1).normalize();
            // Make y orthogonal to x
            y = (y - x * x.dot(y)).normalize();
            // Local z-axis
            // don't need normalize, cuz x,y orthonormal
            let mut z = x.cross(y);

            if (z.dot(face_centroid - poly_centroid) > 0.0) {
                z = -z;
            }

            // now [x y z] is rotation matrix, [w] is translate
            face_transforms.push(Mat4::from_cols(
                x.extend(0.0),
                y.extend(0.0),
                z.extend(0.0),
                face_centroid.extend(1.0),
            ));
        }

        Ok(Polyhedron {
            name: longname.to_owned(),
            vertices,
            faces,
            face_transforms,
        })
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
}

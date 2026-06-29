use std::sync::Arc;

use crate::polyhedron::Polyhedron;
use colorous::{PAIRED, SINEBOW, VIRIDIS};
use log::{debug, info};
use rusqlite::{Connection, Result};
use three_d::*;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

mod polyhedron;
mod ui;

/// The interface is the entrypoint for wasm
///
/// it mainly handles events and keeps state
#[wasm_bindgen]
pub struct Interface {
    connection: Connection,
    #[allow(unused)] // need to keep alive backing memory during connection
    backing_bytes: Vec<u8>,
    polyhedron: Polyhedron,
    scene: Scene,
    canvas: HtmlCanvasElement,
    context: Context,
    /// different stls per polygon, this is template
    /// need also store somewhere their transforms?
    /// maximum is 10-gon, want nice indexing: face_meshes[3] = triangles
    /// I don't care about unused first 3 units and 7 and 9
    pcbs: [Vec<Option<CpuMesh>>; 11],
    // /// I guess also transform per stl
    // /// so these are kinda...
    // /// nah, I think re-calculate them is enough?
    // /// or no, bc need actually keep state
    // /// but they are generated from polyhedron
    // /// organized like:
    // /// // triangle -> version -> instances
    // /// transforms[3][0]
    // /// maybe need better organization with some hashmap somewhere or something to go
    // /// face -> stl
    // /// for make easy change stl for face
    // ///
    // /// also question is how do rotation?
    // ///
    // /// but for instance calculating, e.g. how is rendered, below is best
    // pcb_transforms: [Vec<Vec<Mat4>>; 11],
    /// Mapping from polygon face index -> pcb variant
    face_variant_mapping: Vec<usize>,
    /// super single-source-of-truth-and-ignore-face_variant_mapping_plz_or_something
    instances: Vec<Instances>,
}

/// The scene is well, the scene
///
/// camera, lights, model and faces
pub struct Scene {
    camera: Camera,
    model: Gm<Mesh, PhysicalMaterial>,
    instanced_pcbs: Vec<Gm<InstancedMesh, PhysicalMaterial>>,
    lights: Vec<Box<dyn Light>>,
    face_instance_map: Vec<(usize, usize)>,
}

#[wasm_bindgen]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlurEvent {
    PolyhedronChanged(String),
}

#[wasm_bindgen]
pub fn init_iface(canvas: HtmlCanvasElement, db_bytes: Vec<u8>) -> Result<Interface, JsValue> {
    // Set up panic hook for better error messages in the browser
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Trace).unwrap();

    info!("logging works");
    // Open an in-memory database
    let connection = Connection::open(":memory:").map_err(|e| e.to_string())?;
    let len = db_bytes.len() as i64;

    debug!(
        "loading database: {}",
        String::from_utf8_lossy(&db_bytes[..10])
    );
    unsafe {
        sqlite_wasm_rs::sqlite3_deserialize(
            connection.handle().cast(),
            c"main".as_ptr(),
            db_bytes.as_ptr() as *mut u8,
            len,
            len,
            sqlite_wasm_rs::SQLITE_DESERIALIZE_READONLY,
        );
    }

    let webgl_context = canvas
        .get_context("webgl2")?
        .unwrap()
        .dyn_into::<web_sys::WebGl2RenderingContext>()?;

    let context = three_d::Context::from_gl_context(Arc::new(
        three_d::context::Context::from_webgl2_context(webgl_context),
    ))
    .map_err(|e| e.to_string())?;

    // Create camera
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(1, 1),
        vec3(0.0, 4.0, 8.0),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        degrees(45.0),
        0.1,
        100.0,
    );
    // Create model
    let model = Gm::new(
        Mesh::new(&context, &CpuMesh::cube()),
        PhysicalMaterial::new(&context, &CpuMaterial::default()),
    );

    // // glb is big deps, stl smol
    // // http is huge deps, bc we don't understand that fetch api exists?
    // let key = "assets/3-01.stl";
    // let stl_bytes = fetch(key).await?;
    // let mut loaded: CpuMesh =
    //     three_d_asset::io::deserialize(key, stl_bytes).map_err(|e| e.to_string())?;

    // loaded.transform(Mat4::from_translation(vec3(0.0, 3.0f32.sqrt() / 2.0, 0.0)));
    // loaded.transform(Mat4::from_scale(2.0 / 50.0));
    // model = Gm::new(
    //     Mesh::new(&context, &loaded),
    //     PhysicalMaterial::new(&context, &CpuMaterial::default()),
    // );

    // add light
    let ambient = AmbientLight::new(&context, 0.05, Srgba::WHITE);
    let point = PointLight::new(
        &context,
        0.5,
        Srgba::WHITE,
        vec3(-20.0, -20.0, 20.0),
        Attenuation::default(),
    );
    let polyhedron = Polyhedron::load(&connection, "truncated cube").map_err(|e| e.to_string())?;
    // face_meshes[3].push(loaded);
    let iface = Interface {
        backing_bytes: db_bytes,
        connection,
        scene: Scene {
            camera,
            model: model,
            lights: vec![Box::new(point), Box::new(ambient)],
            instanced_pcbs: Vec::new(),
            face_instance_map: Vec::new(),
        },
        polyhedron,
        canvas,
        context,
        // https://stackoverflow.com/a/54134142/14681457
        pcbs: Default::default(),
        // only populated when there is something
        face_variant_mapping: Vec::new(),
        instances: Vec::new(),
    };

    Ok(iface)
}

#[wasm_bindgen]
impl Interface {
    pub fn polyhedron_names(&mut self) -> Result<Vec<String>, JsError> {
        let mut stmt = self.connection.prepare("SELECT longname FROM Polyhedron")?;
        let res = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>>>()?;
        Ok(res)
    }

    pub fn render(&mut self) {
        // actually draw something?
        let screen = RenderTarget::screen(&self.context, self.canvas.width(), self.canvas.height());
        screen
            .clear(ClearState::color_and_depth(0.8, 0.8, 0.8, 1.0, 1.0))
            .render(
                &self.scene.camera,
                self.scene.instanced_pcbs.iter().flat_map(|f| f.into_iter()),
                &self
                    .scene
                    .lights
                    .iter()
                    .map(|l| l.as_ref())
                    .collect::<Vec<_>>(),
            );
    }

    pub fn set_polyhedron(&mut self, poly: &str) -> Result<JsValue, JsError> {
        info!("poly request for {poly}");
        let new_poly = Polyhedron::load(&self.connection, &poly)?;
        self.face_variant_mapping.clear();
        self.face_variant_mapping.resize(new_poly.faces.len(), 0);

        let mut new_mesh = new_poly.cpu_mesh();
        new_mesh.compute_normals();
        let new_model = Gm::new(
            Mesh::new(&self.context, &new_mesh),
            self.scene.model.material.clone(),
        );
        self.scene.model = new_model;
        self.polyhedron = new_poly;
        self.add_mesh_to_faces()?;
        // so
        serde_wasm_bindgen::to_value(&self.missing_variants()).map_err(|e| JsError::from(e))
    }

    /// Load pcb stls into the simulation
    ///
    /// needs to be organized as
    /// `pcb = [nr_of_edges][variant]`
    /// so to get second variant of pentagon, can do `pcbs[5][1]`
    pub fn add_pcb(&mut self, n_gon: usize, variant: usize, data: Vec<u8>) -> Result<(), JsError> {
        // add None for non-existent variants
        while self.pcbs[n_gon].len() <= variant {
            self.pcbs[n_gon].push(None);
        }
        let key = &format!("{}-{:b}.stl", n_gon, variant);
        let mut mesh: CpuMesh = three_d_asset::io::deserialize(key, data)?;

        mesh.transform(Mat4::from_scale(2.0 / 50.0))?;
        if n_gon == 3 {
            // kicad exports the center as like the board origin which is calculated from bounding box
            // so we transform it on y-axis by 1/3-1/2=1/6
            // because real center of triangle is 1/3 of its height
            mesh.transform(Mat4::from_translation(vec3(
                0.0,
                // size  diff           height-side ratio
                2.0 * 1.0 / 6.0 * 3.0f32.sqrt() / 2.0,
                0.0,
            )))?;
        } else if n_gon == 5 {
            // same story here, but ofc with pentagon it's more difficult eh
            mesh.transform(Mat4::from_translation(vec3(
                0.0,
                // https://en.wikipedia.org/wiki/Pentagon
                // side length * (og - correct)
                2.0 * ((5.0 + 2.0 * 5.0f32.sqrt()).sqrt() / 4.0
                    - 1.0 / (2.0 * (5.0 - 20.0f32.sqrt()).sqrt())),
                0.0,
            )))?;
        }
        self.pcbs[n_gon][variant] = Some(mesh);
        info!("successfully loaded stl for {key}");
        // side-effects, yay!
        self.add_mesh_to_faces()?;
        Ok(())
    }
}

impl Interface {
    pub fn instanced_pcb_index(&self, face_id: usize) -> (usize, usize) {
        self.scene.face_instance_map[face_id]
    }

    pub fn face_index(&self, instanced_pcb_id: usize, instance_id: usize) -> Option<usize> {
        self.scene
            .face_instance_map
            .iter()
            .position(|a| *a == (instanced_pcb_id, instance_id))
    }

    pub fn missing_variants(&self) -> Vec<Vec<usize>> {
        let mut missing_variants = vec![Vec::new(); self.pcbs.len()];
        for (i, face) in self.polyhedron.faces.iter().enumerate() {
            let n = face.len();
            if n >= self.pcbs.len() {
                continue;
            }
            let var = self.face_variant_mapping[i];
            debug!("checking if variant {var} exists as pcb in {:?}", self.pcbs);
            // yes vector search, but probs small container, so this better than hashset
            if self.pcbs[n].len() <= var {
                missing_variants[n].push(var);
            } else if self.pcbs[n][var].is_none() && !missing_variants[n].contains(&var) {
                missing_variants[n].push(var);
            }
        }
        missing_variants
    }

    pub fn add_mesh_to_faces(&mut self) -> Result<(), JsError> {
        // This is slightly ugly now, because we re-upload the meshes, but I
        // guess that's fine because it allows to update them or something
        self.scene.instanced_pcbs.clear();
        self.scene.face_instance_map.clear();
        self.instances.clear();
        self.scene
            .face_instance_map
            .resize(self.polyhedron.faces.len(), Default::default());
        let mut fallback_mesh = CpuMesh::sphere(8);
        fallback_mesh.transform(Mat4::from_scale(0.1))?;
        // go through all n of n-gon and all variants.
        // INVARIANT: self.scene.faces order is depended on for face-click detection
        // this is a flattened version of self.pcbs
        // that's actually sensible
        // so range(self.faces, 4) = {
        //  let start = self.pcbs.iter().take(4).map(|n| n.len()).sum();
        //  let len = self.pcbs[4].len();
        //  start..start+len
        // }
        for n_gon in 3..=10 {
            for (variant, mesh) in self.pcbs[n_gon]
                .iter()
                .enumerate()
                .filter(|(_, m)| m.is_some())
            {
                let transformations: Vec<Mat4> = self
                    .polyhedron
                    .face_transforms
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| {
                        self.polyhedron.faces[*i].len() == n_gon
                            && self.face_variant_mapping[*i] == variant
                    })
                    .map(|(_, tr)| *tr)
                    .collect();
                let instances = Instances {
                    colors: Some(
                        (0..transformations.len())
                            .map(|i| {
                                let color = SINEBOW.eval_rational(i, transformations.len()); // PAIRED[i % PAIRED.len()]; // VIRIDIS.eval_rational(i, transformations.len());
                                Srgba::new_opaque(color.r, color.g, color.b)
                            })
                            .collect(),
                    ),
                    transformations,
                    ..Default::default()
                };
                self.instances.push(instances.clone());
                let instanced_gm = Gm::new(
                    // TODO: don't upload to GPU every f'ing time, but set_transforms on the instancedmeshes
                    // that'll also alllow for a better mapping
                    InstancedMesh::new(&self.context, &instances, mesh.as_ref().unwrap()),
                    PhysicalMaterial {
                        albedo: Srgba::WHITE,
                        ..Default::default()
                    },
                );
                self.scene.instanced_pcbs.push(instanced_gm);
                for (instance_index, (face_index, _transform)) in self
                    .polyhedron
                    .face_transforms
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| {
                        self.polyhedron.faces[*i].len() == n_gon
                            && self.face_variant_mapping[*i] == variant
                    })
                    .enumerate()
                {
                    self.scene.face_instance_map[face_index] =
                        (self.scene.instanced_pcbs.len() - 1, instance_index)
                }
            }
        }
        // let instances = face_instances(&self.polyhedron, &self.context);
        self.render();
        Ok(())
    }
}

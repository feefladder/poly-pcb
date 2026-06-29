use std::{ops::Index, sync::Arc};

use crate::polyhedron::Polyhedron;
use log::{debug, info};
use rusqlite::{Connection, Result};
use three_d::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    CustomEventInit, HtmlCanvasElement, KeyboardEvent, MouseEvent, PointerEvent, WheelEvent,
    js_sys::Uint8Array, window,
};

mod polyhedron;

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
}

/// The scene is well, the scene
///
/// camera, lights, model and faces
pub struct Scene {
    camera: Camera,
    model: Gm<Mesh, PhysicalMaterial>,
    faces: Vec<Gm<InstancedMesh, PhysicalMaterial>>,
    lights: Vec<Box<dyn Light>>,
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
    let ambient = AmbientLight::new(&context, 0.02, Srgba::new_opaque(249, 240, 107));
    let point = PointLight::new(
        &context,
        0.2,
        Srgba::WHITE,
        vec3(-10.0, -10.0, 10.0),
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
            faces: Vec::new(),
        },
        polyhedron,
        canvas,
        context,
        // https://stackoverflow.com/a/54134142/14681457
        pcbs: std::array::from_fn(|_| vec![None; 4]),
        // only populated when there is something
        face_variant_mapping: Vec::new(),
    };

    Ok(iface)
}

#[wasm_bindgen]
impl Interface {
    pub fn on_key(&mut self, key_event: KeyboardEvent) -> Result<(), JsError> {
        info!("on_key called");
        match key_event.key().as_str() {
            "ArrowLeft" => {
                self.scene
                    .camera
                    .rotate_around(Vec3::zero(), std::f32::consts::FRAC_PI_8, 0.0);
            }
            "ArrowRight" => {
                self.scene
                    .camera
                    .rotate_around(Vec3::zero(), -std::f32::consts::FRAC_PI_8, 0.0);
            }
            "ArrowUp" => {
                self.scene
                    .camera
                    .rotate_around(Vec3::zero(), 0.0, std::f32::consts::FRAC_PI_8);
            }
            "ArrowDown" => {
                self.scene
                    .camera
                    .rotate_around(Vec3::zero(), 0.0, -std::f32::consts::FRAC_PI_8);
            }
            "Tab" | " " => {
                let polyhedra = self.polyhedron_names()?;
                if let Some(i) = polyhedra.iter().position(|n| **n == self.polyhedron.name) {
                    let next_polyhedron = &polyhedra[(i + 1) % polyhedra.len()];
                    let e_detail = CustomEventInit::new();
                    e_detail.set_detail(&next_polyhedron.into());
                    self.canvas
                        .dispatch_event(
                            &web_sys::CustomEvent::new_with_event_init_dict(
                                "next_polyhedron",
                                &e_detail,
                            )
                            .unwrap(),
                        )
                        .unwrap();
                }
            }
            k => info!("pressed {k:?}"),
        }
        self.render();
        Ok(())
    }

    pub fn on_pointer_down(&mut self, event: PointerEvent) -> Result<(), JsValue> {
        info!("pointer down {event:?}");
        self.canvas.set_pointer_capture(event.pointer_id())
    }

    pub fn on_pointer_move(&mut self, event: PointerEvent) -> Result<(), JsError> {
        info!("pointer moved {event:?}");
        // Only rotate while the primary button is held.
        if event.buttons() & 1 == 0 {
            return Ok(());
        }
        self.scene.camera.rotate_around(
            Vec3::zero(),
            event.movement_x() as f32 / 42.0,
            event.movement_y() as f32 / 42.0,
        );
        self.render();
        Ok(())
    }

    pub fn on_pointer_up(&mut self, event: PointerEvent) -> Result<(), JsValue> {
        info!("pointer moved {event:?}");
        self.canvas.release_pointer_capture(event.pointer_id())
    }

    pub fn on_wheel(&mut self, event: WheelEvent) -> Result<(), JsValue> {
        info!("scroll {event:?}");
        let delta = event.delta_y() as f32;

        // Zoom in/out.
        self.scene
            .camera
            .zoom(-delta / 42.0, std::f32::NEG_INFINITY, std::f32::INFINITY);

        self.render();
        Ok(())
    }

    pub fn on_click(&mut self, event: MouseEvent) -> Result<(), JsError> {
        info!("mouse event happened: {event:?}");
        let rect = self.canvas.get_bounding_client_rect();

        // is f64 bc css pixels are fake, scale by canvas size to get back to physics the gpu understands
        let x = ((event.client_x() as f64 - rect.left()) * self.canvas.width() as f64
            / rect.width()) as f32;

        let y = ((event.client_y() as f64 - rect.top()) * self.canvas.height() as f64
            / rect.height()) as f32;

        if let Some(p) = pick(
            &self.context,
            &self.scene.camera,
            (x, y),
            self.scene.faces.iter().flat_map(|f| f.into_iter()),
            Cull::Back,
        )? {
            info!(
                "clicked on face with geometry id {}, instance id {}",
                p.geometry_id, p.instance_id
            );
        }
        info!(
            "at ({:?},{:?}), client: ({:?},{:?}), canvas: ({:?},{:?})",
            event.x(),
            event.y(),
            event.client_x(),
            event.client_y(),
            x,
            y,
        );
        Ok(())
    }

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
        // ok, so this is slightly weird, but we first create an iterator from faces and then
        screen
            .clear(ClearState::color_and_depth(0.8, 0.8, 0.8, 1.0, 1.0))
            .render(
                &self.scene.camera,
                self.scene.faces.iter().flat_map(|f| f.into_iter()), //self.scene.model.into_iter().chain(&self.scene.faces),
                &self
                    .scene
                    .lights
                    .iter()
                    .map(|l| l.as_ref())
                    .collect::<Vec<_>>(),
            );
    }

    pub fn on_resize(&mut self) {
        let width = self.canvas.client_width() as u32;
        let height = self.canvas.client_height() as u32;

        self.canvas.set_width(width);
        self.canvas.set_height(height);

        self.scene
            .camera
            .set_viewport(Viewport::new_at_origo(width, height));
    }

    pub fn set_polyhedron(&mut self, poly: &str) -> Result<JsValue, JsError> {
        info!("poly request for {poly}");
        let new_poly = Polyhedron::load(&self.connection, &poly)?;
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
        debug!("{} bytes", data.len());
        debug!("{:?}", &data[..16.min(data.len())]);
        debug!("loading stl {}", String::from_utf8_lossy(&data[..10]));
        let mut mesh: CpuMesh = three_d_asset::io::deserialize(key, data)?;

        mesh.transform(Mat4::from_scale(2.0 / 50.0))?;
        if n_gon == 3 {
            // kicad exports the center as like the board origin which is calculated from bounding box
            // so we transform it on y-axis by 1/3-1/2=1/6
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
    pub fn missing_variants(&self) -> Vec<Vec<usize>> {
        let mut missing_variants = vec![Vec::new(); self.pcbs.len()];
        for (i, face) in self.polyhedron.faces.iter().enumerate() {
            let n = face.len();
            if n >= self.pcbs.len() {
                continue;
            }
            let var = self.face_variant_mapping[i];
            // yes vector search, but probs small container, so this better than hashset
            if self.pcbs[n][var].is_none() && !missing_variants[n].contains(&var) {
                missing_variants[n].push(var);
            }
        }
        missing_variants
    }

    pub fn add_mesh_to_faces(&mut self) -> Result<(), JsError> {
        // This is slightly ugly now, because we re-upload the meshes, but I
        // guess that's fine because it allows to update them or something
        self.scene.faces.clear();
        let mut fallback_mesh = CpuMesh::sphere(8);
        fallback_mesh.transform(Mat4::from_scale(0.1))?;
        // go through all n of n-gon and maybe all variants? but we'll do variants later
        for n in 3..=10 {
            for (var, mesh) in self.pcbs[n].iter().enumerate().filter(|(_, m)| m.is_some()) {
                let instances = Instances {
                    transformations: self
                        .polyhedron
                        .face_transforms
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| {
                            self.polyhedron.faces[*i].len() == n
                                && self.face_variant_mapping[*i] == var
                        })
                        .map(|(_, tr)| *tr)
                        .collect(),
                    ..Default::default()
                };
                let instanced_gm = Gm::new(
                    // TODO: fix panic on not present pcb
                    InstancedMesh::new(&self.context, &instances, mesh.as_ref().unwrap()),
                    PhysicalMaterial {
                        albedo: Srgba::new_opaque(255, 255, 127),
                        ..Default::default()
                    },
                );
                self.scene.faces.push(instanced_gm);
            }
        }
        // let instances = face_instances(&self.polyhedron, &self.context);
        self.render();
        Ok(())
    }
}

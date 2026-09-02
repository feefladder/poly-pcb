use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
use crate::design::{LampDesign, PcbDesign};
use crate::{
    design::VariantMap,
    pcbdron::MultiPcbdron,
    polyhedron::Polyhedron,
    ui::{CurrentStep, STEPS},
};
use log::info;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use three_d::{
    AmbientLight, Attenuation, Camera, ClearState, Context, CpuGeometry, CpuModel, Light,
    PointLight, RenderTarget, Viewport,
};
#[cfg(target_arch = "wasm32")]
use three_d::{context::RGB10_A2, prelude::*};
#[cfg(target_arch = "wasm32")]
use tsify::Tsify;
use tsify::{Ts, declare};
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::HtmlCanvasElement;
use web_sys::{CustomEvent, CustomEventInit};

mod design;
mod pcbdron;
mod polyhedron;
#[cfg(target_arch = "wasm32")]
mod ui;

#[derive(Tsify, Serialize)]
pub struct SetResult(pub Vec<Vec<usize>>, pub Option<LampDesign>);

#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PcbId {
    pub n_gon: usize,
    pub variant: usize,
}

#[wasm_bindgen]
pub struct VarId {
    pub nth_ngon: usize,
    pub pcb_id: PcbId,
    pub need_fetch: bool,
}

/// Planned variants
///
/// Not all variants exist physically for all ngons
///
/// This already exhausts the single-digit hexadecimal representation
/// So development gets a "hard" limit lol
#[derive(Tsify, Serialize)]
pub enum VarFlags {
    Cut = 1,
    HalfLeds = 2,
    Controller = 4,
    Power = 8,
}
pub const VAR_FLAGS: [VarFlags; 4] = [
    VarFlags::Cut,
    VarFlags::HalfLeds,
    VarFlags::Controller,
    VarFlags::Power,
];
#[derive(Tsify, Serialize)]
pub struct AllVarFlags(pub [VarFlags; VAR_FLAGS.len()]);
#[wasm_bindgen]
pub fn var_flags() -> Result<Ts<AllVarFlags>, JsError> {
    Ok(AllVarFlags(VAR_FLAGS).into_ts()?)
}

impl VarFlags {
    /// Get the binary representation
    pub fn b0(self) -> usize {
        self as usize
    }
    /// Remove this option
    #[inline]
    pub fn rm(self, var: &mut usize) {
        *var &= !self.b0()
    }

    /// check if this bit is set
    #[inline]
    pub fn has(self, var: usize) -> bool {
        self.b0() & var != 0
    }

    /// Add this option
    #[inline]
    pub fn add(self, var: &mut usize) {
        *var |= self.b0()
    }

    /// Toggle this option
    #[inline]
    pub fn switch(self, var: &mut usize) {
        *var ^= self.b0()
    }
}

/// The interface is the entrypoint for wasm
///
/// it mainly handles events and keeps state
#[wasm_bindgen]
pub struct Interface {
    connection: Connection,
    #[allow(unused)] // need to keep alive backing memory during db connection
    backing_bytes: Vec<u8>,
    scene: Scene,
    #[cfg(target_arch = "wasm32")]
    canvas: HtmlCanvasElement,
    context: Context,
    /// different stls per polygon, this is template
    /// need also store somewhere their transforms?
    /// maximum is 10-gon, want nice indexing: face_meshes[3] = triangles
    /// I don't care about unused first 3 units and 7 and 9
    pcbs: [Vec<Option<CpuModel>>; 11],
    current_step: CurrentStep,
}

/// The scene is well, the scene
///
/// camera, lights, model and faces
pub struct Scene {
    camera: Camera,
    lights: Vec<Box<dyn Light>>,
    pcbdrons: MultiPcbdron,
}

#[wasm_bindgen]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlurEvent {
    PolyhedronChanged(String),
}

#[wasm_bindgen]
#[cfg(target_arch = "wasm32")]
pub fn init_iface(canvas: HtmlCanvasElement, db_bytes: Vec<u8>) -> Result<Interface, JsValue> {
    // Set up panic hook for better error messages in the browser
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Trace);

    info!("logging works");
    // Open an in-memory database
    let connection = Connection::open(":memory:").map_err(|e| e.to_string())?;
    let len = db_bytes.len() as i64;

    // SAFETY: e
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
    // add light
    let ambient = AmbientLight::new(&context, 0.8, Srgba::WHITE);
    let point = PointLight::new(
        &context,
        5.0,
        Srgba::WHITE,
        vec3(-20.0, -20.0, 20.0),
        Attenuation::default(),
    );
    let polyhedron = Polyhedron::load(&connection, "tetrahedron").map_err(|e| e.to_string())?;
    let pcbdrons = MultiPcbdron::new(&context, polyhedron, &[], &VariantMap::default())
        .map_err(|e| e.to_string())?;
    // face_meshes[3].push(loaded);
    let iface = Interface {
        backing_bytes: db_bytes,
        connection,
        scene: Scene {
            camera,
            lights: vec![Box::new(point), Box::new(ambient)],
            pcbdrons,
        },
        canvas,
        context,
        // https://stackoverflow.com/a/54134142/14681457
        pcbs: Default::default(),
        current_step: STEPS[0],
    };

    Ok(iface)
}

#[wasm_bindgen]
#[cfg(target_arch = "wasm32")]
impl Interface {
    pub fn polyhedron_names(&mut self) -> Result<Vec<String>, JsError> {
        let mut stmt = self.connection.prepare("SELECT longname FROM Polyhedron")?;
        let res = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(res)
    }

    pub fn set_step(&mut self, step: Ts<CurrentStep>) -> Result<(), JsError> {
        self.current_step = step.to_rust()?;
        Ok(())
    }

    pub fn render(&mut self) {
        // actually draw something?
        let screen = RenderTarget::screen(&self.context, self.canvas.width(), self.canvas.height());
        screen
            .clear(ClearState::color_and_depth(0.1, 0.1, 0.2, 1.0, 1.0))
            .render(
                &self.scene.camera,
                self.scene
                    .pcbdrons
                    .into_iter()
                    .chain(self.scene.pcbdrons.debug_path().into_iter()),
                &self
                    .scene
                    .lights
                    .iter()
                    .map(|l| l.as_ref())
                    .collect::<Vec<_>>(),
            );
    }

    pub fn set_polyhedron(&mut self, polyhedron: String) -> Result<Ts<MissingVariants>, JsError> {
        self.scene
            .pcbdrons
            .set_pcbdron(
                Polyhedron::load(&self.connection, &polyhedron).map_err(|e| (*e).clone())?,
                &Vec::new(),
            )
            .map_err(|e| (*e).clone())?;
        self.render();
        Ok(MissingVariants(polyhedron, self.missing_variants()).into_ts()?)
    }

    pub fn apply_design(&mut self, ts_design: Ts<LampDesign>) -> Result<Ts<SetResult>, JsError> {
        let design = ts_design.to_rust()?;
        // compare the given design to our current design

        let r = self
            .scene
            .pcbdrons
            .pcbdrons()
            .map(|p| p.polyhedron.mean_r())
            .max_by(|a, b| a.total_cmp(b))
            .expect("have poly")
            * 4.0;
        self.scene.camera.set_zoom_factor(1.0 / r);

        let maybe_corrected = self
            .scene
            .pcbdrons
            .apply_design(design, &self.connection)
            .map_err(|e| JsError::new(&e.to_string()))?;
        // do the zooming thing
        // still need to figure out what is a good distance multiplier

        // self.update_instances()?;
        // so
        self.render();
        SetResult(self.missing_variants(), maybe_corrected)
            .into_ts()
            .map_err(Into::into)
    }

    /// Load pcb stls into the simulation
    ///
    /// loading and kicad quirks are handled here, then it's passed to pcbdrons
    pub fn add_pcb(
        &mut self,
        n_gon: usize,
        variant: usize,
        data: Vec<u8>,
        name: &str,
    ) -> Result<(), JsError> {
        info!(
            "deserializing {name} from {:?}",
            String::from_utf8_lossy(&data[..10])
        );
        // add None for non-existent variants
        while self.pcbs[n_gon].len() <= variant {
            self.pcbs[n_gon].push(None);
        }
        info!(
            "deserializing {name} from {:?}",
            String::from_utf8_lossy(&data[..10])
        );
        let mut model: CpuModel = three_d_asset::io::deserialize(name, data)?;
        info!(
            "deserialized {name}, which has {} geometries",
            model.geometries.len(),
        );

        for prim in &mut model.geometries {
            if let CpuGeometry::Triangles(mesh) = &mut prim.geometry {
                // kicad export specific stuff:
                const DB_LEN: f32 = 2.0;
                const PCB_LEN: f32 = 50.0;
                // stls are scaled where 1.0=1mm
                if name.ends_with("stl") {
                    mesh.transform(Mat4::from_scale(DB_LEN / PCB_LEN))?;
                } else if name.ends_with("glb") {
                    // glbs are scaled where 1.0=1m
                    mesh.transform(Mat4::from_scale(DB_LEN * 1000.0 / PCB_LEN))?;
                    // and have a different orientation convention
                    mesh.transform(Mat4::from_angle_x(degrees(90.0)))?;
                }
                if n_gon == 3 {
                    // kicad exports the center as like the board origin which is calculated from bounding box
                    // this is different from polygon center
                    // for triangle, that is 1/3 from bottom
                    // so we transform it on y-axis by 1/3-1/2=1/6
                    // because real center of triangle is 1/3 of its height
                    mesh.transform(Mat4::from_translation(vec3(
                        0.0,
                        // size  diff           height-side ratio
                        1.0 / 6.0 * 3.0f32.sqrt(),
                        0.0,
                    )))?;
                } else if n_gon == 5 {
                    // same story here, but ofc with pentagon it's more difficult eh
                    mesh.transform(Mat4::from_translation(vec3(
                        0.0,
                        // from wikipedia
                        // https://en.wikipedia.org/wiki/Pentagon
                        // side length * (og - correct)
                        // where
                        // og = heigth/2
                        // and
                        // correct = inradius
                        DB_LEN
                            * ((5.0 + 2.0 * 5.0f32.sqrt()).sqrt() / 4.0
                                - 1.0 / (2.0 * (5.0 - 20.0f32.sqrt()).sqrt())),
                        0.0,
                    )))?;
                }
            }
        }

        self.scene
            .pcbdrons
            .add_pcb(&self.context, PcbId { n_gon, variant }, &model)
            .map_err(|e| JsError::new(&e.to_string()))?;
        // register the stl in self, so we can reference it
        // self.instance_map
        //     .insert(PcbId { n_gon, variant }, self.instances.len());
        // self.instances.push(Instances::default());
        // let instanced_pcb = InstancedModel::new(
        //     &self.context,
        //     &self.instances[self.instances.len() - 1],
        //     &model,
        // )?;
        // self.scene.instanced_pcbs.push(instanced_pcb);

        // not sure why keep this around?
        self.pcbs[n_gon][variant] = Some(model);
        // self.update_instances()
        self.render();
        Ok(())
    }

    pub fn complete_path(&mut self) {
        self.scene.pcbdrons.complete_path();
        self.render();
        self.notify_update_path();
    }

    pub fn pop_path(&mut self) {
        self.scene.pcbdrons.pop_path();
        self.render();
        self.notify_update_path();
    }

    pub fn push_path(&mut self, i: usize) {
        if let Some(var_id) = self.scene.pcbdrons.push_path(i) {
            // so now
            self.maybe_request_variant(var_id);
        }
        self.render();
        self.notify_update_path();
    }

    pub fn update_variant(&mut self, ngon: usize, nth_ngon: usize, variant: usize) {
        self.scene.pcbdrons.set_variant(ngon, nth_ngon, variant);
        self.render();
    }

    pub fn set_variant(&mut self, face_id: usize, variant: usize) -> Result<(), JsError> {
        // set the face to the variant
        let pcbdron = self.scene.pcbdrons.pcbdrons_mut().nth(0).unwrap();
        pcbdron.variant_map[face_id] = variant;

        // prepare data for event dispatch
        let n_gon = pcbdron.polyhedron.faces[face_id].len();
        //
        // so js-side we keep per-ngon, so need find out which one this is
        // just dispatch an event and let js update our state
        let nth_ngon = pcbdron
            .polyhedron
            .iter_ngon(n_gon)
            .position(|i| i == face_id)
            .ok_or(JsError::new(&format!(
                "could not find position of {n_gon}-gon at face {face_id}"
            )))?;
        self.scene.pcbdrons.update_instances();
        self.maybe_request_variant(VarId {
            nth_ngon,
            pcb_id: PcbId { n_gon, variant },
            need_fetch: false,
        });
        self.render();
        Ok(())
    }

    fn maybe_request_variant(&self, mut var_id: VarId) {
        let PcbId { n_gon, variant } = &var_id.pcb_id;
        var_id.need_fetch = if self.pcbs[*n_gon].len() <= *variant {
            true
        } else if self.pcbs[*n_gon][*variant].is_none() {
            true
        } else {
            false
        };

        let e_detail = CustomEventInit::new();
        e_detail.set_detail(&var_id.into());
        self.canvas
            .dispatch_event(
                &CustomEvent::new_with_event_init_dict("update_variant", &e_detail).unwrap(),
            )
            .unwrap();
    }

    /// Send an update not
    fn notify_update_path(&self) -> Result<(), JsError> {
        let e_detail = CustomEventInit::new();
        if let Some(path) = self.scene.pcbdrons.get_path() {
            e_detail.set_detail(&path.into_ts()?.js_value());
        }
        self.canvas
            .dispatch_event(
                &CustomEvent::new_with_event_init_dict("update_path", &e_detail).unwrap(),
            )
            .unwrap();
        Ok(())
    }
}

#[derive(Tsify, Serialize)]
pub struct MissingVariants(pub String, pub Vec<Vec<usize>>);

impl Interface {
    pub fn missing_variants(&self) -> Vec<Vec<usize>> {
        let mut missing_variants = vec![Vec::new(); self.pcbs.len()];
        for n_gon in 3..=10 {
            for pcbdron in self.scene.pcbdrons.pcbdrons() {
                for var in pcbdron
                    .polyhedron
                    .iter_ngon(n_gon)
                    .map(|idx| pcbdron.variant_map[idx])
                {
                    // the 20-sided prism does not exist
                    if n_gon >= self.pcbs.len() {
                        continue;
                    }
                    // yes vector search, but probs small container, so this better than hashset
                    if self.pcbs[n_gon].len() <= var {
                        missing_variants[n_gon].push(var);
                    } else if self.pcbs[n_gon][var].is_none()
                        && !missing_variants[n_gon].contains(&var)
                    {
                        missing_variants[n_gon].push(var);
                    }
                }
            }
        }
        missing_variants
    }

    // we don't manually update instances, but keep them up-to-date when adding pcbs
    // or changing variant? Not there yet... In any case, that'd be a MultiPcbdron thing
    // pub fn update_instances(&mut self) -> Result<(), JsError> {
    //     let mut fallback_mesh = CpuMesh::sphere(8);
    //     fallback_mesh.transform(Mat4::from_scale(0.1))?;
    //     // just iterate through the map
    //     for (pcb_id, instance_idx) in &self.instance_map {
    //         let transformations: Vec<Mat4> = self
    //             .polyhedron
    //             .face_transforms
    //             .iter()
    //             .enumerate()
    //             .filter(|(i, _)| {
    //                 self.polyhedron.faces[*i].len() == pcb_id.n_gon
    //                     && self.face_variant_mapping[*i] == pcb_id.variant
    //             })
    //             .map(|(_, tr)| *tr)
    //             .collect();
    //         if true {
    //             self.instances[*instance_idx].colors = None;
    //         } else {
    //             // debug colors
    //             self.instances[*instance_idx].colors = Some(
    //                 (0..transformations.len())
    //                     .map(|i| {
    //                         let c = VIRIDIS.eval_rational(i, transformations.len());
    //                         Srgba::new_opaque(c.r, c.g, c.b)
    //                     })
    //                     .collect(),
    //             );
    //         }
    //         self.instances[*instance_idx].transformations = transformations;

    //         self.scene.instanced_pcbs[*instance_idx]
    //             .iter_mut()
    //             .for_each(|mp| mp.geometry.set_instances(&self.instances[*instance_idx]));
    //     }
    //     self.render();
    //     Ok(())
    // }
}

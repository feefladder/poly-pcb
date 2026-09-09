use std::{iter, sync::Arc};

use crate::{design::PcBorsign, pcboron::Fidx};
use crate::{
    design::VariantMap,
    pcboron::Pcboron,
    polyhedron::Polyhedron,
    ui::{Animation, CurrentStep, STEPS, Tween},
};
use log::{info, warn};
use rusqlite::Connection;
use serde::Serialize;
#[cfg(target_arch = "wasm32")]
use three_d::prelude::*;
use three_d::{
    AmbientLight, Attenuation, Camera, ClearState, Context, CpuGeometry, CpuModel, Light,
    PointLight, RenderTarget, Viewport,
};
use tsify::Ts;
#[cfg(target_arch = "wasm32")]
use tsify::Tsify;
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::HtmlCanvasElement;
use web_sys::{CustomEvent, CustomEventInit};

mod design;

mod pcbdron;
mod pcboron;
mod polyhedron;
#[cfg(target_arch = "wasm32")]
mod ui;

mod assign_variants;
mod make_path;
mod select_poly;

#[derive(Tsify, Serialize)]
pub struct SetResult(pub Vec<Vec<usize>>, pub Option<PcBorsign>);

#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PcbId {
    pub n_gon: usize,
    pub variant: usize,
}

#[wasm_bindgen]
#[derive(Debug, Clone, Copy)]
pub struct VarId {
    pub nth_ngon: usize,
    pub pcb_id: PcbId,
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
#[cfg(target_arch = "wasm32")]
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
    tweens: Vec<Tween>,
}

/// The scene is well, the scene
///
/// camera, lights, model and faces
pub struct Scene {
    camera: Camera,
    lights: Vec<Box<dyn Light>>,
    pcboron: Pcboron,
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
    let pcboron = Pcboron::new(&context, polyhedron, &[], &VariantMap::default())
        .map_err(|e| e.to_string())?;
    // face_meshes[3].push(loaded);
    let iface = Interface {
        backing_bytes: db_bytes,
        connection,
        scene: Scene {
            camera,
            lights: vec![Box::new(point), Box::new(ambient)],
            pcboron,
        },
        canvas,
        context,
        // https://stackoverflow.com/a/54134142/14681457
        pcbs: Default::default(),
        current_step: STEPS[0],
        tweens: Vec::new(),
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

    pub fn apply_design(&mut self, ts_design: Ts<PcBorsign>) -> Result<Ts<SetResult>, JsError> {
        let design = ts_design.to_rust()?;
        // compare the given design to our current design

        info!("applying design {design:?}");
        let r = self
            .scene
            .pcboron
            .pcbdrons()
            .iter()
            .map(|p| p.polyhedron.mean_r())
            .max_by(|a, b| a.total_cmp(b))
            .expect("have poly")
            * 4.0;
        self.scene.camera.set_zoom_factor(1.0 / r);

        let maybe_corrected = self
            .scene
            .pcboron
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

    /// Load pcb gltfs into the simulation
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
            .pcboron
            .add_pcb(&self.context, PcbId { n_gon, variant }, &model)
            .map_err(|e| JsError::new(&e.to_string()))?;

        // not sure why keep this around?
        self.pcbs[n_gon][variant] = Some(model);
        // self.update_instances()
        self.render();
        Ok(())
    }

    pub fn animate(&mut self, timestamp: f64) -> bool {
        self.tweens
            .retain_mut(|tween| tween.update(&mut self.scene, timestamp));
        self.render();
        !self.tweens.is_empty()
    }
}

#[cfg(target_arch = "wasm32")]
impl Interface {
    pub fn render(&mut self) {
        // actually draw something?
        let screen = RenderTarget::screen(&self.context, self.canvas.width(), self.canvas.height());
        screen
            .clear(ClearState::color_and_depth(0.2, 0.2, 0.4, 1.0, 1.0))
            .render(
                &self.scene.camera,
                self.scene
                    .pcboron
                    .into_iter()
                    .chain(&*self.scene.pcboron.debug_path()),
                &self
                    .scene
                    .lights
                    .iter()
                    .map(|l| l.as_ref())
                    .collect::<Vec<_>>(),
            );
    }

    pub fn add_tween(&mut self, tween: Tween) {
        // ah ok
        if !self.tweens.is_empty() {
            self.tweens.push(tween);
        } else {
            self.tweens.push(tween);
            self.canvas
                .dispatch_event(&CustomEvent::new("start_animation").unwrap())
                .unwrap();
        }
    }

    fn event<T: Tsify + Serialize>(&self, name: &str, detail: T) -> Result<(), JsError> {
        self.js_ev(name, detail.into_ts()?.js_value());
        Ok(())
    }

    fn js_ev(&self, name: &str, d: JsValue) {
        let e_detail = CustomEventInit::new();
        e_detail.set_detail(&d);
        self.canvas
            .dispatch_event(&CustomEvent::new_with_event_init_dict(name, &e_detail).unwrap())
            .unwrap();
    }
}

#[derive(Tsify, Serialize)]
pub struct MissingVariants(pub String, pub Vec<Vec<usize>>);

impl Interface {
    pub fn missing_variants(&self) -> Vec<Vec<usize>> {
        let mut missing_variants = vec![Vec::new(); self.pcbs.len()];
        for n_gon in 3..=10 {
            for pcbdron in self.scene.pcboron.pcbdrons() {
                for var in pcbdron
                    .polyhedron
                    .iter_ngon(n_gon)
                    .map(|idx| pcbdron.variant_map[idx])
                {
                    // the 20-sided prism does not exist
                    if n_gon >= self.pcbs.len() || missing_variants[n_gon].contains(&var) {
                        continue;
                    }
                    // yes vector search, but probs small container, so this better than hashset
                    if self.pcbs[n_gon].len() <= var {
                        missing_variants[n_gon].push(var);
                    } else if self.pcbs[n_gon][var].is_none() {
                        missing_variants[n_gon].push(var);
                    }
                }
            }
        }
        info!("missing variants: {missing_variants:?}");
        missing_variants
    }
}

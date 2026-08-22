use std::{collections::BTreeMap, sync::Arc};

use crate::{pcbdron::MultiPcbdron, polyhedron::Polyhedron};
use log::info;
use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};
use three_d::{
    AmbientLight, Attenuation, Camera, ClearState, Context, CpuGeometry, CpuModel, Light, Mat4,
    PointLight, RenderTarget, Srgba, Viewport, degrees, vec3,
};
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::HtmlCanvasElement;

mod design;
mod pcbdron;
mod polyhedron;
#[cfg(target_arch = "wasm32")]
mod ui;

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
    };

    Ok(iface)
}

/// Per n-gon mapping
///
/// for truncated tetrahedron, this will only set triangles:
/// ```
/// use std::collections::BTreeMap;
/// use poly_pcb::VariantMap;
/// VariantMap(BTreeMap::from([(3, vec![0,1,1,2])]));
/// ```
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct VariantMap(pub BTreeMap<usize, Vec<usize>>);

#[wasm_bindgen]
#[cfg(target_arch = "wasm32")]
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
        // // build debugging normal arrows
        // let positions = Positions::F32(
        //     self.polyhedron
        //         .face_transforms
        //         .iter()
        //         .flat_map(|t| {
        //             [
        //                 (t * vec4(0.0, -0.01, 0.0, 1.0)).truncate(),
        //                 (t * vec4(0.0, 0.01, 0.0, 1.0)).truncate(),
        //                 (t * vec4(0.0, 0.00, 1.0, 1.0)).truncate(),
        //             ]
        //         })
        //         .collect(),
        // );

        // let gm = Wireframe::new_from_cpu_mesh(
        //     &self.context,
        //     &CpuMesh {
        //         positions,
        //         ..Default::default()
        //     },
        //     1.0,
        //     Srgba::BLUE,
        // // );
        // let mut objects: Vec<&dyn Object> = self
        //     .scene
        //     .instanced_pcbs
        //     .iter()
        //     .flat_map(|f| f.into_iter())
        //     .collect();
        // // render face normals?
        // if false {
        //     objects.extend(gm.into_iter());
        // }
        // // render poly mesh?
        // if false {
        //     objects.extend(self.scene.into_iter())
        // }

        screen
            .clear(ClearState::color_and_depth(0.1, 0.1, 0.2, 0.0, 1.0))
            .render(
                &self.scene.camera,
                self.scene.pcbdrons.into_iter(),
                &self
                    .scene
                    .lights
                    .iter()
                    .map(|l| l.as_ref())
                    .collect::<Vec<_>>(),
            );
    }

    pub fn set_polyhedron(
        &mut self,
        poly: &str,
        variants: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        info!("setting polyhedron to {poly}");
        let new_poly = Polyhedron::load(&self.connection, &poly).map_err(|e| e.to_string())?;
        if let Some(v) = variants {
            let vm = serde_wasm_bindgen::from_value::<VariantMap>(v)?;
            self.scene
                .pcbdrons
                .set_pcbdron(new_poly, &vm)
                .map_err(|e| e.to_string())?;
        };
        // self.update_instances()?;
        // so
        self.render();
        serde_wasm_bindgen::to_value(&self.missing_variants()).map_err(|e| e.to_string().into())
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
}

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

use std::sync::Arc;

use crate::extract_poly::{Polyhedron, list_polyhedra};
use log::info;
use rusqlite::{Connection, Result};
use three_d::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{HtmlCanvasElement, Request, RequestInit, Response, js_sys::Uint8Array, window};

mod extract_poly;

/// The interface is the entrypoint for wasm
///
/// it mainly handles events and keeps state
#[wasm_bindgen]
pub struct Interface {
    connection: Connection,
    backing_bytes: Vec<u8>,
    polyhedron: Polyhedron,
    scene: Scene,
    canvas: HtmlCanvasElement,
    context: Context,
    /// different stls per polygon, this is template
    /// need also store somewhere their transforms?
    /// maximum is 10-gon, want nice indexing: face_meshes[3] = triangles
    /// I don't care about unused first 3 units and 7 and 9
    face_meshes: [Vec<CpuMesh>; 11],
    /// I guess also transform per stl
    /// so these are kinda...
    /// nah, I think re-calculate them is enough?
    /// or no, bc need actually keep state
    /// but they are generated from polyhedron
    /// organized like:
    /// // triangle -> version -> instances
    /// transforms[3][0]
    /// maybe need better organization with some hashmap somewhere or something to go
    /// face -> stl
    /// for make easy change stl for face
    ///
    /// also question is how do rotation?
    ///
    /// but for instance calculating, e.g. how is rendered, below is best
    transforms: [Vec<Vec<Mat4>>; 11],
}

/// The scene is well, the scene
///
/// camera, lights, model and faces
pub struct Scene {
    camera: Camera,
    model: Gm<Mesh, PhysicalMaterial>,
    faces: Gm<InstancedMesh, PhysicalMaterial>,
    lights: Vec<Box<dyn Light>>,
}

#[wasm_bindgen]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlurEvent {
    PolyhedronChanged(String),
}

#[wasm_bindgen]
pub async fn init_iface(canvas: HtmlCanvasElement) -> Result<Interface, JsValue> {
    // Set up panic hook for better error messages in the browser
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Trace).unwrap();

    info!("logging works");
    // Get the polydb and load it
    let db_bytes = fetch("assets/polydb.sqlite3").await?;
    // Open an in-memory database
    let connection = Connection::open(":memory:").map_err(|e| ErrorShim::from(e))?;
    let len = db_bytes.len() as i64;

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
        10.0,
    );
    // Create model
    let mut model = Gm::new(
        Mesh::new(&context, &CpuMesh::cube()),
        PhysicalMaterial::new(&context, &CpuMaterial::default()),
    );

    // glb is big deps, stl smol
    // http is huge deps, bc we don't understand that fetch api exists?
    let key = "assets/3-01.stl";
    let stl_bytes = fetch(key).await?;
    let mut loaded: CpuMesh =
        three_d_asset::io::deserialize(key, stl_bytes).map_err(|e| e.to_string())?;

    loaded.transform(Mat4::from_translation(vec3(0.0, 3.0f32.sqrt() / 2.0, 0.0)));
    loaded.transform(Mat4::from_scale(2.0 / 50.0));
    model = Gm::new(
        Mesh::new(&context, &loaded),
        PhysicalMaterial::new(&context, &CpuMaterial::default()),
    );

    // add light
    let ambient = AmbientLight::new(&context, 0.2, Srgba::RED);
    let point = PointLight::new(
        &context,
        0.2,
        Srgba::BLUE,
        vec3(10.0, 10.0, 10.0),
        Attenuation::default(),
    );
    let polyhedron = Polyhedron::load(&connection, "truncated cube").map_err(|e| e.to_string())?;
    let mut face_meshes: [Vec<CpuMesh>; 11] = Default::default();
    face_meshes[3].push(loaded);
    let iface = Interface {
        backing_bytes: db_bytes,
        connection,
        scene: Scene {
            camera,
            model: model,
            lights: vec![Box::new(ambient), Box::new(point)],
            faces: face_instances(&polyhedron, &context),
        },
        polyhedron,
        canvas,
        context,
        // https://stackoverflow.com/a/54134142/14681457
        face_meshes,
        transforms: Default::default(),
    };

    Ok(iface)
}

#[wasm_bindgen]
impl Interface {
    pub fn polyhedron_names(&mut self) -> Result<Vec<String>, JsValue> {
        list_polyhedra(&self.connection).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    pub fn render(&mut self) {
        let width = self.canvas.client_width() as u32;
        let height = self.canvas.client_height() as u32;

        self.canvas.set_width(width);
        self.canvas.set_height(height);

        self.scene
            .camera
            .set_viewport(Viewport::new_at_origo(width, height));
        // actually draw something?
        let screen = RenderTarget::screen(&self.context, width, height);
        screen
            .clear(ClearState::color_and_depth(0.8, 0.8, 0.8, 1.0, 1.0))
            .render(
                &self.scene.camera,
                &self.scene.faces, //self.scene.model.into_iter().chain(&self.scene.faces),
                &self
                    .scene
                    .lights
                    .iter()
                    .map(|l| l.as_ref())
                    .collect::<Vec<_>>(),
            );
    }

    pub fn set_polyhedron(&mut self, poly: String) -> Result<(), JsError> {
        info!("poly request for {poly}");
        let new_poly = Polyhedron::load(&self.connection, &poly)?;

        let mut new_mesh = new_poly.cpu_mesh();
        new_mesh.compute_normals();
        let new_model = Gm::new(
            Mesh::new(&self.context, &new_mesh),
            self.scene.model.material.clone(),
        );
        self.scene.model = new_model;
        self.polyhedron = new_poly;
        self.add_mesh_to_faces()?;
        self.render();
        Ok(())
    }

    pub fn add_mesh_to_faces(&mut self) -> Result<(), JsError> {
        // let instances = face_instances(&self.polyhedron, &self.context);
        // just put triangle on every face
        let instances = Instances {
            transformations: self
                .polyhedron
                .face_transforms
                .iter()
                .enumerate()
                .filter(|(i, _)| self.polyhedron.faces[*i].len() == 3)
                .map(|(_, tr)| tr / 2.5)
                .collect(),
            ..Default::default()
        };
        let instanced_gm = Gm::new(
            InstancedMesh::new(&self.context, &instances, &self.face_meshes[3][0]),
            PhysicalMaterial::default(),
        );
        self.scene.faces = instanced_gm;
        self.render();
        Ok(())
    }
}

fn face_instances(
    polyhedron: &Polyhedron,
    context: &Context,
) -> Gm<InstancedMesh, PhysicalMaterial> {
    // so we'll need a library of face-meshes at some point and then use instancing I think, so first start with a spehere and instance that
    // (from picking demo)
    // this is ugly, but we just create a sphere here and now, in stead of loading it from self
    let mut sphere = CpuMesh::sphere(8);
    sphere.transform(Mat4::from_scale(0.1)).unwrap();
    let transformations: Vec<_> = polyhedron.face_transforms.to_vec();
    // .faces
    // .iter()
    // .map(|face| {
    //     let centroid: Vec3 = face
    //         .iter()
    //         .map(|idx| polyhedron.vertices[*idx as usize])
    //         .sum::<Vec3>()
    //         / face.len() as f32;
    //     Mat4::from_translation(centroid)
    // })
    // .collect();
    let no_instances = transformations.len();
    let instances = Instances {
        transformations,
        colors: Some(vec![Srgba::GREEN; no_instances]),
        ..Default::default()
    };
    Gm::new(
        InstancedMesh::new(&context, &instances, &sphere),
        PhysicalMaterial::new_transparent(
            &context,
            &CpuMaterial {
                albedo: Srgba::new(255, 255, 0, 255),
                ..Default::default()
            },
        ),
    )
}

// need to fetch db from site
// use bare api: https://rustwasm.app/en/learn/fetch-api
async fn fetch(path: &str) -> Result<Vec<u8>, JsValue> {
    let opts = RequestInit::new();
    opts.set_method("GET");

    let request = Request::new_with_str_and_init(path, &opts)?;
    let window = window().unwrap();
    let resp: Response = JsFuture::from(window.fetch_with_request(&request))
        .await?
        .dyn_into()?;
    // TODO: fail here on error in stead of returning empty array
    let buf = resp.array_buffer()?.await?;

    Ok(Uint8Array::new(&buf).to_vec())
}

struct ErrorShim(String);

impl From<rusqlite::Error> for ErrorShim {
    fn from(value: rusqlite::Error) -> Self {
        ErrorShim(format!("Error: {value}"))
    }
}

impl From<ErrorShim> for JsValue {
    fn from(value: ErrorShim) -> Self {
        JsValue::from_str(&value.0)
    }
}

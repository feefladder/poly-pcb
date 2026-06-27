use std::sync::Arc;

use crate::extract_poly::{Polyhedron, list_polyhedra};
use log::info;
use rusqlite::{Connection, Result};
use three_d::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{HtmlCanvasElement, Request, RequestInit, Response, js_sys::Uint8Array, window};

mod extract_poly;

#[wasm_bindgen]
pub struct Interface {
    connection: Connection,
    backing_bytes: Vec<u8>,
    polyhedron: Polyhedron,
    scene: Scene,
    canvas: HtmlCanvasElement,
    context: Context,
}

pub struct Scene {
    camera: Camera,
    models: Vec<Gm<Mesh, PhysicalMaterial>>,
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
    let db_bytes = fetch_db("assets/polydb.sqlite3").await?;
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
    let mut control = OrbitControl::new(camera.target(), 1.0, 1000.0);

    // Create model
    let mut model = Gm::new(
        Mesh::new(&context, &CpuMesh::cube()),
        PhysicalMaterial::new(&context, &CpuMaterial::default()),
    );
    model.set_animation(|time| Mat4::from_angle_y(radians(time * 0.0005)));

    // add light
    let ambient = AmbientLight::new(&context, 0.2, Srgba::RED);
    let point = PointLight::new(
        &context,
        0.2,
        Srgba::BLUE,
        vec3(10.0, 10.0, 10.0),
        Attenuation::default(),
    );
    let iface = Interface {
        backing_bytes: db_bytes,
        polyhedron: Polyhedron::load(&connection, "truncated cube").map_err(|e| e.to_string())?,
        connection,
        scene: Scene {
            camera,
            models: vec![model],
            lights: vec![Box::new(ambient), Box::new(point)],
        },
        canvas,
        context,
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
                &self.scene.models,
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
            self.scene.models[0].material.clone(),
        );
        self.scene.models.clear();
        self.scene.models.push(new_model);
        self.render();
        Ok(())
    }
}

// need to fetch db from site
// use bare api: https://rustwasm.app/en/learn/fetch-api
async fn fetch_db(path: &str) -> Result<Vec<u8>, JsValue> {
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

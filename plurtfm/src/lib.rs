use crate::extract_poly::{Polyhedron, list_polyhedra};
use log::info;
use rusqlite::{Connection, Result};
use three_d::{FrameInputGenerator, SurfaceSettings, WindowedContext, renderer::*};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    HtmlCanvasElement, Request, RequestInit, Response,
    console::log_1,
    js_sys::{ArrayBuffer, Uint8Array},
    window,
};
use winit::{event::Event, platform::web::WindowBuilderExtWebSys};
use winit::{event_loop::EventLoopWindowTarget, window::WindowBuilder};

mod extract_poly;

#[wasm_bindgen]
pub struct Interface {
    connection: Connection,
    backing_bytes: Vec<u8>,
    polyhedron: Option<Polyhedron>,
}

#[wasm_bindgen]
pub async fn init_iface(canvas: HtmlCanvasElement) -> Result<Interface, JsValue> {
    // Set up panic hook for better error messages in the browser
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Trace).unwrap();

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

    // check canvas
    info!("got canvas: {} canvas", canvas.tab_index());

    // Initialize renderer from canvas
    let event_loop = winit::event_loop::EventLoop::new();
    let window_builder = WindowBuilder::new().with_canvas(Some(canvas));
    let window = window_builder.build(&event_loop).unwrap();
    let context = WindowedContext::from_winit_window(&window, SurfaceSettings::default()).unwrap();

    // Create camera
    let mut camera = Camera::new_perspective(
        Viewport::new_at_origo(1, 1),
        vec3(0.0, 2.0, 4.0),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        degrees(45.0),
        0.1,
        10.0,
    );
    let mut control = OrbitControl::new(camera.target(), 1.0, 100.0);

    // Create model
    let mut model = Gm::new(
        Mesh::new(&context, &CpuMesh::cube()),
        ColorMaterial {
            color: Srgba::GREEN,
            ..Default::default()
        },
    );
    model.set_animation(|time| Mat4::from_angle_y(radians(time * 0.0005)));

    // Event loop
    let mut frame_input_generator = FrameInputGenerator::from_winit_window(&window);
    event_loop.run(move |event, event_loop, control_flow| match event {
        Event::RedrawRequested(window_id) => {
            let mut frame_input = frame_input_generator.generate(&context);

            control.handle_events(&mut camera, &mut frame_input.events);
            camera.set_viewport(frame_input.viewport);
            model.animate(frame_input.accumulated_time as f32);
            frame_input
                .screen()
                .clear(ClearState::color_and_depth(0.8, 0.8, 0.8, 1.0, 1.0))
                .render(&camera, &model, &[]);

            context.swap_buffers().unwrap();
            window.request_redraw();
        }
        winit::event::Event::WindowEvent { ref event, .. } => {
            frame_input_generator.handle_winit_window_event(event);
            match event {
                winit::event::WindowEvent::Resized(physical_size) => {
                    context.resize(*physical_size);
                }
                winit::event::WindowEvent::CloseRequested => {}
                _ => (),
            }
        }
        _ => {}
    });

    // Ok(Interface {
    //     connection,
    //     backing_bytes: db_bytes,
    //     polyhedron: None,
    // })
}

#[wasm_bindgen]
impl Interface {
    pub fn polyhedron_names(&mut self) -> Result<Vec<String>, JsValue> {
        list_polyhedra(&self.connection).map_err(|e| JsValue::from_str(&e.to_string()))
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

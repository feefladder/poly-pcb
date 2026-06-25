use three_d::*;

#[wasm_bindgen]
pub fn spawn_renderer(canvas_id: web_sys::HtmlCanvasElement) {
    wasm_bindgen_futures::spawn_local(async move {
        let window = Window::new(WindowSettings {
            canvas: Some(canvas_id.to_string()),
            ..Default::default()
        })
        .unwrap();

        let context = window.gl();

        let cpu_mesh = CpuMesh::sphere(32);

        let mesh = Mesh::new(&context, &cpu_mesh);

        let material = PhysicalMaterial::new_opaque(&context, &CpuMaterial::default());

        let model = Gm::new(mesh, material);

        let camera = Camera::new_perspective(
            window.viewport(),
            vec3(3.0, 3.0, 3.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 1.0, 0.0),
            degrees(45.0),
            0.1,
            100.0,
        );

        window.render_loop(move |frame_input| {
            Screen::write(&context, ClearState::default(), || {
                model.render(&camera, &[]);
            });

            FrameOutput::default()
        });
    });
}

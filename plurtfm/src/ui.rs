//! User Interactions
//!
//! anything responding to events, because it was growing too big

use log::info;
use three_d::{Cull, InnerSpace, Srgba, Vec3, Viewport, Zero, pick};
use wasm_bindgen::{JsError, JsValue, prelude::wasm_bindgen};
use web_sys::{CustomEvent, CustomEventInit, KeyboardEvent, MouseEvent, PointerEvent, WheelEvent};

use crate::Interface;

#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PcbId {
    pub n_gon: usize,
    pub variant: usize,
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
                self.render();
            }
            "ArrowRight" => {
                self.scene
                    .camera
                    .rotate_around(Vec3::zero(), -std::f32::consts::FRAC_PI_8, 0.0);
                self.render();
            }
            "ArrowUp" => {
                self.scene
                    .camera
                    .rotate_around(Vec3::zero(), 0.0, std::f32::consts::FRAC_PI_8);
                self.render();
            }
            "ArrowDown" => {
                self.scene
                    .camera
                    .rotate_around(Vec3::zero(), 0.0, -std::f32::consts::FRAC_PI_8);
                self.render();
            }
            "Tab" | " " => {
                self.next_polyhedron()?;
            }
            k => info!("pressed {k:?}"),
        }

        Ok(())
    }

    pub fn next_polyhedron(&mut self) -> Result<(), JsError> {
        let polyhedra = self.polyhedron_names()?;
        if let Some(i) = polyhedra.iter().position(|n| **n == self.polyhedron.name) {
            let next_polyhedron = &polyhedra[(i + 1) % polyhedra.len()];
            let e_detail = CustomEventInit::new();
            e_detail.set_detail(&next_polyhedron.into());
            self.canvas
                .dispatch_event(
                    &CustomEvent::new_with_event_init_dict("next_polyhedron", &e_detail).unwrap(),
                )
                .unwrap();
        }
        self.render();
        Ok(())
    }

    pub fn on_pointer_down(&mut self, event: PointerEvent) -> Result<(), JsValue> {
        info!("pointer down {event:?}");
        self.canvas.set_pointer_capture(event.pointer_id())
    }

    pub fn on_pointer_move(&mut self, event: PointerEvent) -> Result<(), JsError> {
        // Only rotate while the primary button is held.
        if event.buttons() & 1 == 0 {
            return Ok(());
        } else {
            // optionally do something here on click-drag
        }
        let frac = 42.0 / self.scene.camera.position().magnitude();
        self.scene.camera.rotate_around(
            Vec3::zero(),
            event.movement_x() as f32 / frac,
            event.movement_y() as f32 / frac,
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

        let y = ((rect.bottom() - event.client_y() as f64) * self.canvas.height() as f64
            / rect.height()) as f32;

        if let Some(p) = pick(
            &self.context,
            &self.scene.camera,
            (x, y),
            self.scene.instanced_pcbs.iter().flat_map(|f| f.into_iter()),
            Cull::Back,
        )? {
            info!(
                "clicked on face with geometry id {}, instance id {}",
                p.geometry_id, p.instance_id
            );
            // unwrap bc from raycast, so must be possible, unless code buggy
            let transform =
                self.instances[p.geometry_id as usize].transformations[p.instance_id as usize];
            let face_id = self
                .polyhedron
                .face_transforms
                .iter()
                .position(|t| *t == transform)
                .ok_or(JsError::new("HELP!"))?;
            let n_gon = self.polyhedron.faces[face_id].len();
            let current_variant = self.face_variant_mapping[face_id];
            if self.face_variant_mapping[face_id] == self.pcbs[n_gon].len() - 1 {
                let e_detail = CustomEventInit::new();
                e_detail.set_detail(
                    &PcbId {
                        n_gon,
                        variant: current_variant + 1,
                    }
                    .into(),
                );

                self.canvas
                    .dispatch_event(
                        &CustomEvent::new_with_event_init_dict("request_pcb", &e_detail).unwrap(),
                    )
                    .unwrap();
                self.face_variant_mapping[face_id] = 0;
                // now it would actually be nice to somewhere have a "this instances array is this n-var" so we don't have to re-upload meshes
            } else {
                self.face_variant_mapping[face_id] = current_variant + 1;
            }
            // also need find out which pcb that is, but for now, we'll magic number 2

            // need to find out which face was clicked, based on geometry/instance ids..
            // maybe it's worth keeping around some mapping? it's kind of load-order dependent...
            // ah, wait geometryId ofc directly corresponds to the place in self.scene.faces
            // but those are a flat-map version
            self.update_instances()?;
        }
        Ok(())
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
}

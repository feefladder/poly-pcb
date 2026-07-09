//! User Interactions
//!
//! anything responding to events, because it was growing too big

use log::info;
use three_d::{Cull, InnerSpace, Vec3, Viewport, Zero, pick};
use wasm_bindgen::{JsError, JsValue, prelude::wasm_bindgen};
use web_sys::{CustomEvent, CustomEventInit, KeyboardEvent, MouseEvent, PointerEvent, WheelEvent};

use crate::{Interface, PcbId, VarId};

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
        if let Some(i) = polyhedra.iter().position(|n| {
            self.scene
                .pcbdrons
                .pcbdrons()
                .any(|p| p.polyhedron.name == *n)
        }) {
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
        if (event.buttons() & 1 == 0) && event.pointer_type() != "touch" {
            return Ok(());
        } else {
            // optionally do something here on click-drag
            // like setting faces' colors to black for example
            // for example, I'd say
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
        let rect = self.canvas.get_bounding_client_rect();
        // is f64 bc css pixels are fake, scale by canvas size to get back to physics the gpu understands
        let x =
            ((event.x() as f64 - rect.left()) * self.canvas.width() as f64 / rect.width()) as f32;

        let y = ((rect.bottom() - event.y() as f64) * self.canvas.height() as f64 / rect.height())
            as f32;

        if let Some(p) = pick(
            &self.context,
            &self.scene.camera,
            (x, y),
            self.scene.pcbdrons.into_iter(),
            Cull::Back,
        )? {
            info!(
                "clicked on face with geometry id {}, instance id {}",
                p.geometry_id, p.instance_id
            );
            let face_id = self
                .scene
                .pcbdrons
                .pick(p.geometry_id, p.instance_id)
                .ok_or(JsError::new("could not locate clicked geometry"))?;
            info!("which corresponds to face n. {face_id} on pcbdron 0");

            if event.ctrl_key() {
                let pcbdron = self.scene.pcbdrons.pcbdrons_mut().nth(0).unwrap();
                info!("while holding ctrl");
                for v in pcbdron.variant_map.iter_mut() {
                    if *v & 4 == 4 {
                        *v ^= 4
                    }
                }
                pcbdron.variant_map[face_id] = 4;
                pcbdron.polyhedron.find_path(face_id);
                self.scene.pcbdrons.update_instances();
                self.render();
            } else {
                let pcbdron = self.scene.pcbdrons.pcbdrons().nth(0).unwrap();
                let n_gon = pcbdron.polyhedron.faces[face_id].len();
                let current_variant = pcbdron.variant_map[face_id];
                let variant = current_variant + 1;
                // so js-side we keep per-ngon, so need find out which one this is
                // just dispatch an event and let js update our state
                let nth_ngon = pcbdron
                    .polyhedron
                    .faces
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v.len() == n_gon)
                    .position(|(i, _)| i == face_id)
                    .ok_or(JsError::new(&format!(
                        "could not find position of {n_gon}-gon at face {face_id}"
                    )))?;
                let e_detail = CustomEventInit::new();
                e_detail.set_detail(
                    &VarId {
                        nth_ngon,
                        pcb_id: PcbId { n_gon, variant },
                    }
                    .into(),
                );
                self.canvas
                    .dispatch_event(
                        &CustomEvent::new_with_event_init_dict("request_pcb", &e_detail).unwrap(),
                    )
                    .unwrap();
            }
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

//! User Interactions
//!
//! anything responding to events, because it was growing too big

use log::{debug, info};
use serde::{Deserialize, Serialize};
use three_d::{Cull, InnerSpace, Vec3, Viewport, Zero, pick};
use tsify::{Ts, Tsify, declare};
use wasm_bindgen::{JsError, JsValue, convert::IntoWasmAbi, prelude::wasm_bindgen};
use web_sys::{CustomEvent, CustomEventInit, KeyboardEvent, MouseEvent, PointerEvent, WheelEvent};

use crate::{Interface, PcbId, VarFlags, VarId};

#[derive(Tsify, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentStep {
    SelectPoly,
    AssignVariants(usize),
    MakePath,
}

pub const N_STEPS: usize = 3;
pub const STEPS: [CurrentStep; N_STEPS] = [
    CurrentStep::SelectPoly,
    CurrentStep::AssignVariants(0),
    CurrentStep::MakePath,
];

#[derive(Tsify, Serialize)]
pub struct Steps(pub [CurrentStep; N_STEPS]);

#[wasm_bindgen]
pub fn steps() -> Result<Ts<Steps>, JsError> {
    Ok(Steps(STEPS).into_ts()?)
}

#[wasm_bindgen]
impl Interface {
    pub fn on_key(&mut self, key_event: KeyboardEvent) -> Result<(), JsError> {
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
            " " if self.current_step == CurrentStep::SelectPoly => {
                self.next_polyhedron();
            }
            "Backspace" => match self.current_step {
                CurrentStep::MakePath => {
                    self.pop_path();
                }
                _ => {}
            },
            "Enter" => match self.current_step {
                CurrentStep::MakePath => {
                    self.complete_path();
                }
                _ => {}
            },
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
            let missing_variants = self.set_polyhedron(next_polyhedron.to_string())?;
            let e_detail = CustomEventInit::new();
            e_detail.set_detail(&missing_variants.js_value());
            self.canvas
                .dispatch_event(
                    &CustomEvent::new_with_event_init_dict("next_polyhedron", &e_detail).unwrap(),
                )
                .unwrap();
        }
        self.scene.pcbdrons.update_debug_path();
        self.render();
        Ok(())
    }

    pub fn on_pointer_down(&mut self, event: PointerEvent) -> Result<(), JsValue> {
        info!("pointer down {event:?}");
        self.canvas.set_pointer_capture(event.pointer_id())
    }

    pub fn on_pointer_move(&mut self, event: PointerEvent) -> Result<(), JsError> {
        // Only rotate while the primary button is held.
        if (event.buttons() & 1 == 0) && event.pointer_type() == "mouse" {
            return Ok(());
        } else {
        }
        // optionally do something here on click-drag
        // like setting faces' colors to black for example
        // for example, I'd say
        if let CurrentStep::AssignVariants(variant) = self.current_step
            && let Some(face_id) = self.pick(&MouseEvent::from(event.clone()))
        {
            debug!("Setting face {face_id} to {variant:?}");
            // paint the face with the current brush
            let pcbdron = self.scene.pcbdrons.pcbdrons_mut().nth(0).unwrap();
            pcbdron.variant_map[face_id] = variant;
            let n_gon = pcbdron.polyhedron.faces[face_id].len();
            pcbdron.variant_map[face_id] = variant;
            let nth_ngon = pcbdron
                .polyhedron
                .iter_ngon(n_gon)
                .position(|i| i == face_id)
                .ok_or(JsError::new(&format!(
                    "could not find position of {n_gon}-gon at face {face_id}"
                )))?;
            let need_fetch = if self.pcbs[n_gon].len() <= variant {
                true
            } else if self.pcbs[n_gon][variant].is_none() {
                true
            } else {
                false
            };
            let e_detail = CustomEventInit::new();
            e_detail.set_detail(
                &VarId {
                    nth_ngon,
                    pcb_id: PcbId { n_gon, variant },
                    need_fetch,
                }
                .into(),
            );
            self.canvas
                .dispatch_event(
                    &CustomEvent::new_with_event_init_dict("update_variant", &e_detail).unwrap(),
                )
                .unwrap();
            self.scene
                .pcbdrons
                .update_instances()
                .expect("can update instances");
            // need also update the design
        }
        let frac = if event.pointer_type() == "mouse" {
            42.0
        } else {
            420.0
        } / self.scene.camera.position().magnitude();
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
        if event.pointer_type() == "mouse" {
            self.canvas.release_pointer_capture(event.pointer_id())?
        }
        Ok(())
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

    fn event_to_xy(&self, event: &MouseEvent) -> (f32, f32) {
        let rect = self.canvas.get_bounding_client_rect();
        // is f64 bc css pixels are fake, scale by canvas size to get back to physics the gpu understands
        let x =
            ((event.x() as f64 - rect.left()) * self.canvas.width() as f64 / rect.width()) as f32;

        let y = ((rect.bottom() - event.y() as f64) * self.canvas.height() as f64 / rect.height())
            as f32;
        (x, y)
    }

    fn pick(&self, event: &MouseEvent) -> Option<usize> {
        let (x, y) = self.event_to_xy(event);
        if let Some(p) = pick(
            &self.context,
            &self.scene.camera,
            (x, y),
            self.scene.pcbdrons.into_iter(),
            Cull::Back,
        )
        .ok()?
        {
            info!(
                "clicked on face with geometry id {}, instance id {}",
                p.geometry_id, p.instance_id
            );
            self.scene.pcbdrons.pick(p.geometry_id, p.instance_id)
        } else {
            None
        }
    }

    pub fn on_click(&mut self, event: MouseEvent) -> Result<(), JsError> {
        let face_id = self
            .pick(&event)
            .ok_or(JsError::new("clicked on nothing"))?;
        info!("which corresponds to face n. {face_id} on pcbdron 0");
        match self.current_step {
            CurrentStep::SelectPoly => {}
            CurrentStep::MakePath => {
                self.scene.pcbdrons.add_face_to_path(face_id);
                // let pcbdron = self.scene.pcbdrons.pcbdrons_mut().nth(0).unwrap();
                // for v in pcbdron.variant_map.iter_mut() {
                //     VarFlags::Controller.rm(v);
                // }
                // pcbdron.variant_map[face_id] = 4;
                // pcbdron.polyhedron.find_path(face_id);
                // let design = pcbdron.get_design();
                // let e_detail = CustomEventInit::new();
                // e_detail.set_detail(&design.into_ts().unwrap().into());
                // self.canvas
                //     .dispatch_event(
                //         &CustomEvent::new_with_event_init_dict("design_changed", &e_detail)
                //             .unwrap(),
                //     )
                //     .unwrap();
                // self.scene
                //     .pcbdrons
                //     .update_instances()
                //     .map_err(|e| JsError::new(&e.to_string()))?;
                // self.scene.pcbdrons.update_debug_path();
                self.render();
            }
            CurrentStep::AssignVariants(variant) => {
                self.set_variant(face_id, variant);
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

//! User Interactions
//!
//! anything responding to events, because it was growing too big
use log::{info, warn};
use serde::{Deserialize, Serialize};
use three_d::{Cull, InnerSpace, Mat3, Quat, Vec3, Viewport, Zero, pick};
use tsify::{Ts, Tsify};
use wasm_bindgen::{JsError, JsValue, prelude::wasm_bindgen};
use web_sys::{CustomEvent, CustomEventInit, KeyboardEvent, MouseEvent, PointerEvent, WheelEvent};

use crate::{Interface, Scene, VarId, pcboron::Fidx};

#[derive(Tsify, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentStep {
    SelectPoly,
    AssignVariants(usize),
    MakePath,
}

#[derive(Debug, Clone)]
pub struct Tween {
    pub start: Option<f64>,
    pub duration: f64,
    pub ease: fn(f64) -> f64,
    pub animation: Animation,
}

impl Tween {
    pub fn new(duration: f64, animation: Animation) -> Self {
        Self {
            start: None,
            duration,
            ease: |x| x,
            animation,
        }
    }

    /// update the tween to the given timestamp, indicating if it's still busy
    pub fn update(&mut self, scene: &mut Scene, timestamp: f64) -> bool {
        let start = *self.start.get_or_insert(timestamp);
        let t = (timestamp - start) / self.duration;
        if timestamp < start {
            warn!("timestamp {timestamp} less than start: {:?}", self.start);
        }
        // check now for completion, so the ease can do overshoot
        if t >= 1.0 {
            self.animation.apply(scene, 1.0);
            false
        } else {
            self.animation.apply(scene, (self.ease)(t));
            true
        }
    }
}

#[derive(Debug, Clone)]
pub enum Animation {
    /// Just orbit the camera around the pcbdron(s) to look at the given face
    OrbitTo { rot_start: Quat, rot_end: Quat },
    /// Update instance transforms of the pcbdron(s) to project them onto a plane
    ProjectPcbDrons,
    /// Assemble or dissolve the lamp by removing/adding pcbs in path order
    AssembleLamp,
}

impl Animation {
    pub fn orbit_to_face(scene: &Scene, Fidx { fidx, dridx }: Fidx) -> Self {
        // ok, so this is a face transform
        //    ^
        //    |
        // <--x
        // if I understand correctly
        // yes:
        // ```text
        //  2
        //   \
        //    1--0
        // ```
        // then this will produce a set of axes like
        // ```text
        //    y
        //    |
        // x<-z (into screen)
        // ```
        // to ensure the rake is on the `01` edge
        let ft = &scene.pcboron.pcbdrons()[dridx].polyhedron.face_transforms[fidx];
        let rot_end = Quat::from(Mat3::from_cols(
            ft.x.truncate(),
            ft.y.truncate(),
            ft.z.truncate(),
        ));
        // so copy that convention for the camera
        let c = &scene.camera;
        let rot_start = Quat::from(Mat3::from_cols(
            -c.right_direction(),
            c.up_orthogonal(),
            c.target(),
        ));
        Self::OrbitTo { rot_start, rot_end }
    }

    fn apply(&mut self, scene: &mut Scene, value: f64) {
        match *self {
            Animation::OrbitTo { rot_start, rot_end } => {
                // info!("orbitting to {rot_end:?} at {value:?}");
                // so "looking at a face" means:
                // normal/target/the vector pointing away from the camera == face normal
                // (face normals point inside)
                // up is perpendicular to first edge of polygo
                // Quat::new(w, xi, yj, zk)
                let c = &mut scene.camera;

                // this is slightly sad, cuz it means we're always origin-centered
                // but whatevs
                // otherwise, it'd be... well, the classic affine-ish thing
                let r = c.position().magnitude();

                let rot = Mat3::from(rot_start.slerp(rot_end, value as f32));

                c.set_view(-rot.z * r, rot.z, rot.y);
            }
            _ => todo!(),
        }
    }
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
                CurrentStep::SelectPoly => {
                    self.pop_polyhedron();
                    self.js_ev("pop_polyhedron", JsValue::null());
                }
                CurrentStep::MakePath => self.pop_path(),
                _ => {}
            },
            "Enter" => match self.current_step {
                CurrentStep::SelectPoly => {
                    self.current_step = CurrentStep::AssignVariants(0);
                    self.event("update_step", self.current_step).ok();
                }
                CurrentStep::AssignVariants(_) => {
                    self.current_step = CurrentStep::MakePath;
                    self.event("update_step", self.current_step).ok();
                }
                CurrentStep::MakePath => {
                    self.complete_path();
                }
                _ => {}
            },
            k if "0123456789".contains(k) => match &mut self.current_step {
                CurrentStep::AssignVariants(v) => {
                    *v = k.parse().unwrap();
                    // emit random event
                    let e_detail = CustomEventInit::new();
                    e_detail.set_detail(&JsValue::from(*v));
                    self.canvas
                        .dispatch_event(
                            &CustomEvent::new_with_event_init_dict("update_current_var", &e_detail)
                                .unwrap(),
                        )
                        .unwrap();
                }
                CurrentStep::MakePath => {
                    // let n = "0123456789".find(k).unwrap();
                    // exit the current (last) path with this number
                    self.path_jump(k.parse().unwrap());
                    self.cam_to_last_dron();
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
                .pcboron
                .pcbdrons()
                .iter()
                .any(|p| p.polyhedron.name == *n)
        }) {
            let next_polyhedron = &polyhedra[(i + 1) % polyhedra.len()];
            let missing_variants =
                self.set_polyhedron(next_polyhedron.to_string(), self.scene.pcboron.n() - 1)?;
            let e_detail = CustomEventInit::new();
            e_detail.set_detail(&missing_variants.js_value());
            self.canvas
                .dispatch_event(
                    &CustomEvent::new_with_event_init_dict("next_polyhedron", &e_detail).unwrap(),
                )
                .unwrap();
        }
        self.scene.pcboron.update_debug_path();
        self.render();
        Ok(())
    }

    fn cam_to_last_dron(&mut self) {
        if let Some((i, dron)) = self
            .scene
            .pcboron
            .pcbdrons()
            .iter()
            .enumerate()
            .filter(|(_i, p)| !p.polyhedron.edge_path.is_empty())
            .next_back()
        {
            self.add_tween(Tween::new(
                500.0,
                Animation::orbit_to_face(
                    &self.scene,
                    Fidx {
                        dridx: i,
                        fidx: dron.polyhedron.edge_path.last().unwrap().face_idx,
                    },
                ),
            ));
        }
    }

    pub fn on_pointer_down(&mut self, event: PointerEvent) -> Result<(), JsValue> {
        info!("pointer down {event:?}");
        self.canvas.set_pointer_capture(event.pointer_id())
    }

    pub fn on_pointer_move(&mut self, event: PointerEvent) -> Result<(), JsError> {
        // Only rotate while the primary button is held.
        if (event.buttons() & 1 == 0) && event.pointer_type() == "mouse" {
            return Ok(());
        }
        // optionally do something here on click-drag
        // like setting faces' colors to black for example
        // for example, I'd say
        if let CurrentStep::AssignVariants(variant) = self.current_step
            && let Some(varid) = self.pick(&MouseEvent::from(event.clone()))
        {
            self.set_variant(varid, variant);

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

    fn pick(&self, event: &MouseEvent) -> Option<VarId> {
        let (x, y) = self.event_to_xy(event);
        if let Some(p) = pick(
            &self.context,
            &self.scene.camera,
            (x, y),
            self.scene.pcboron.body_iter(),
            Cull::Back,
        )
        .ok()?
        {
            info!(
                "clicked on face with geometry id {}, instance id {}",
                p.geometry_id, p.instance_id
            );
            self.scene.pcboron.pick(p.geometry_id, p.instance_id)
        } else {
            None
        }
    }

    pub fn on_click(&mut self, event: MouseEvent) {
        let Some(varid) = self.pick(&event) else {
            return;
        };
        info!("which corresponds to face n. {varid:?}");
        match self.current_step {
            CurrentStep::SelectPoly => {}
            CurrentStep::MakePath => {
                if let Some(controller_id) = self.scene.pcboron.add_face_to_path(varid) {
                    self.maybe_request_variant(controller_id);
                }
                self.cam_to_last_dron();
                self.notify_update_path().ok();
                // let pcbdron = self.scene.pcboron.pcboron_mut().nth(0).unwrap();
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
                //     .pcboron
                //     .update_instances()
                //     .map_err(|e| JsError::new(&e.to_string()))?;
                // self.scene.pcboron.update_debug_path();
                self.render();
            }
            CurrentStep::AssignVariants(variant) => {
                self.set_variant(varid, variant);
            }
        }
    }

    pub fn on_resize(&mut self) {
        let width = self.canvas.client_width() as u32;
        let height = self.canvas.client_height() as u32;

        self.canvas.set_width(width);
        self.canvas.set_height(height);

        self.scene
            .camera
            .set_viewport(Viewport::new_at_origo(width, height));
        self.render();
    }
}

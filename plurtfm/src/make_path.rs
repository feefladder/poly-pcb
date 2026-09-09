use log::{debug, error};
use wasm_bindgen::JsError;

use crate::{
    design::{PcbPath, PcbPaths},
    pcbdron::Pcbdron,
    polyhedron::Edge,
    *,
};

/// A visit event of a polygon
///
#[derive(Debug, Clone, Copy)]
pub struct PolygonVisit {
    pub face_idx: usize,
    pub enter: Edge,
}

impl PolygonVisit {
    pub fn exit(self, exit: Edge) -> PolygonCrossing {
        PolygonCrossing {
            face_idx: self.face_idx,
            enter: self.enter,
            exit,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolygonCrossing {
    pub face_idx: usize,
    pub enter: Edge,
    pub exit: Edge,
}

impl PolygonCrossing {
    pub fn to_visit(self) -> PolygonVisit {
        PolygonVisit {
            face_idx: self.face_idx,
            enter: self.enter,
        }
    }
}

#[wasm_bindgen]
impl Interface {
    pub fn complete_path(&mut self) {
        self.scene.pcboron.complete_path();
        self.render();
        self.notify_update_path().ok();
    }

    pub fn pop_path(&mut self) {
        if let Some(fidx) = self.scene.pcboron.pop_path() {
            self.add_tween(Tween::new(
                500.0,
                Animation::orbit_to_face(&self.scene, fidx),
            ));

            self.notify_update_path().ok();
        }
        self.render();
    }

    pub fn path_jump(&mut self, i: usize) {
        if let Some(var_id) = self.scene.pcboron.path_jump(i) {
            // so now
            self.maybe_request_variant(var_id);
        }
        self.render();
        self.notify_update_path().ok();
    }

    /// Send an update not
    pub(crate) fn notify_update_path(&self) -> Result<(), JsError> {
        let path = self.scene.pcboron.get_path();
        self.event("update_path", path)
    }
}

impl Pcboron {
    pub fn complete_path(&mut self) {
        if let Some(todron) = self.pcbdrons.iter_mut().find(|p| {
            !p.polyhedron.edge_path.is_empty() && !p.polyhedron.path_complete_questionmark()
        }) {
            todron.polyhedron.complete_path();
            self.update_instances();
            self.update_debug_path();
        }
    }

    /// Add a face to the path
    /// face indices are local and based on the
    pub fn add_face_to_path(&mut self, varid: VarId) -> Option<VarId> {
        let Some((fidx, pidx)) = self
            .pcbdrons
            .iter()
            .enumerate()
            .flat_map(|(i, p)| p.iter_variant(varid.pcb_id).zip(iter::repeat(i)))
            .nth(varid.nth_ngon)
        else {
            return None;
        };
        info!("adding face {fidx} to dron {pidx}");
        let res = if let Some(cidx) = self.pcbdrons[pidx].polyhedron.add_face_to_path(fidx)
            && pidx == 0
        {
            self.pcbdrons[0].set_controller(cidx)
        } else {
            None
        };
        self.update_instances();
        self.update_debug_path();
        res
    }

    pub fn path_jump(&mut self, jumps: usize) -> Option<VarId> {
        let mut res = None;
        if let Some(i) = self
            .pcbdrons
            .iter()
            .enumerate()
            .position(|(_i, dron)| !dron.polyhedron.path_complete_questionmark())
        {
            let activedron = &mut self.pcbdrons[i];
            info!("working path on {i}:{:?}", activedron.polyhedron.name);
            if let Some(fidx) = activedron.polyhedron.path_jump(jumps)
                && i == 0
            {
                res = activedron.set_controller(fidx);
            }
        }
        self.update_instances();
        self.update_debug_path();
        res
    }

    /// pop the last index from the path
    ///
    /// somethingsomething about needing a linear path
    /// so even if a path has multiple like pcbdrons, it's still not allowed to be patchy
    pub fn pop_path(&mut self) -> Option<Fidx> {
        if let Some((dridx, activedron)) = self
            .pcbdrons
            .iter_mut()
            .enumerate()
            .rfind(|(_i, dron)| !dron.polyhedron.edge_path.is_empty())
        {
            let res = activedron
                .polyhedron
                .pop_path()
                .map(|fidx| Fidx { fidx, dridx });
            self.update_debug_path();
            res
        } else {
            None
        }
    }

    pub fn get_path(&self) -> PcbPaths {
        self.pcbdrons
            .iter()
            .flat_map(|d| d.current_path())
            .collect::<Vec<_>>()
            .into()
    }
}

impl Pcbdron {
    pub fn current_path(&self) -> Option<PcbPath> {
        self.polyhedron
            .current_path()
            .transpose()
            .unwrap_or_else(Some)
    }

    pub fn update_path(&mut self, path: &PcbPath) -> Result<(), usize> {
        self.polyhedron.apply_path(path)
    }

    /// Set the controller on this pcbdron
    ///
    /// Kind of by definition-ish, there can only be one controller and it has
    /// to be on the first pcbdron
    ///
    /// otherwise this entire pathfinding is a bit meaningless
    pub fn set_controller(&mut self, face_idx: usize) -> Option<VarId> {
        let mut res = None;
        if face_idx > self.polyhedron.faces.len() {
            return res;
        }
        for (i, v) in self.variant_map.iter_mut().enumerate() {
            if i == face_idx {
                let variant = VarFlags::Controller.b0();
                *v = variant;
                let n_gon = self.polyhedron.faces[i].len();
                let pcb_id = PcbId { n_gon, variant };
                let nth_ngon = self
                    .polyhedron
                    .iter_ngon(n_gon)
                    .position(|f| f == face_idx)
                    .unwrap();
                res = Some(VarId { nth_ngon, pcb_id });
            } else {
                VarFlags::Controller.rm(v);
            }
        }
        res
    }
}

impl Polyhedron {
    pub fn current_path(&self) -> Option<Result<PcbPath, PcbPath>> {
        let Some(first_face) = self.edge_path.first() else {
            return None;
        };
        debug!("first path face: {first_face:?}");
        let start_ngon = self.faces[first_face.face_idx].len();
        debug!("start_ngon: {start_ngon:?}");
        let Some(start_nth) = self
            .iter_ngon(start_ngon)
            .position(|i| i == first_face.face_idx)
        else {
            return Some(Err(PcbPath {
                start_ngon,
                ..Default::default()
            }));
        };
        debug!("start nth: {start_nth}");
        let mut turns = Vec::with_capacity(self.edge_path.len());
        for PolygonCrossing {
            face_idx,
            enter,
            exit,
        } in &self.edge_path
        {
            if *enter == self.edge_from_face(*face_idx, 0) {
                // first time visiting
                turns.push(
                    self.edge_n_on_face(*face_idx, *exit)
                        .unwrap_or(0)
                        .saturating_sub(1),
                );
            } else {
                let n_enter = self.edge_n_on_face(*face_idx, *enter).unwrap();
                let n_exit = self.edge_n_on_face(*face_idx, *exit).unwrap_or(n_enter);
                let Some(turn) = n_enter.checked_sub(n_exit) else {
                    return Some(Err(PcbPath {
                        start_ngon,
                        start_nth,
                        turns,
                    }));
                };
                turns.push(turn);
            }
        }
        Some(Ok(PcbPath {
            start_ngon,
            start_nth,
            turns,
        }))
    }

    pub fn complete_path(&mut self) {
        // so the last entry we're just taking the visit
        let Some(face_idx) = self.pop_path() else {
            return;
        };

        if self.dfs(PolygonVisit {
            face_idx,
            enter: self.edge_from_face(face_idx, 0),
        }) {
            self.update_transforms();
        }
    }

    /// Apply the given path, clearing the current one
    ///
    /// It is very possible that an impossible path is specified, in which case the number of successful turns is returned
    ///
    /// currently doesn't check very much, this will be improved as considered non-breaking changes
    pub fn apply_path(&mut self, path: &PcbPath) -> Result<(), usize> {
        debug!("applying path {path:?}");
        self.edge_path.clear();
        let start_face = self
            .iter_ngon(path.start_ngon)
            .nth(path.start_nth)
            .ok_or(0usize)?;
        if path.turns.is_empty() {
            //
            return Ok(());
        } else if path.turns.len() == 1 {
            let face_idx = start_face;
            self.cross(PolygonCrossing {
                face_idx,
                enter: self.edge_from_face(face_idx, 0),
                exit: self.edge_from_face(face_idx, 1),
            });
            return Ok(());
        }
        let n = path.turns[0] + 1;
        let exit = self.edge_from_face(start_face, n);

        self.cross(PolygonCrossing {
            face_idx: start_face,
            enter: self.edge_from_face(start_face, 0),
            exit,
        });
        let mut visit = PolygonVisit {
            face_idx: self.other_face(start_face, n),
            enter: exit.rev(),
        };
        for (i, turn) in path.turns.iter().enumerate().skip(1) {
            // rotate the face so it points at the enter
            // this is also in dfs, so maybe generify somehow?
            if self.face_path_index[visit.face_idx].is_empty() {
                // rotate poly so we're entering on edge 0-1
                let rotate_amount = self.edge_n_on_face(visit.face_idx, visit.enter).unwrap();
                self.faces[visit.face_idx].rotate_left(rotate_amount);
                // then set next visit
                let n = turn + 1;
                let exit = self.edge_from_face(visit.face_idx, n);
                self.cross(visit.exit(exit));
                visit = PolygonVisit {
                    face_idx: self.other_face(visit.face_idx, n),
                    enter: exit.rev(),
                }
            } else {
                // re-entering a face, so now turn is counterclockwise
                // since for any direction we'd have to solderjumper, we don't add 1
                let n_enter = self.edge_n_on_face(visit.face_idx, visit.enter).unwrap();
                let n_exit = n_enter.checked_sub(*turn).ok_or(i)?;
                let exit = self.edge_from_face(visit.face_idx, n_exit);
                self.edge_path.push(visit.exit(exit));
                visit = PolygonVisit {
                    face_idx: self.other_face(visit.face_idx, n_exit),
                    enter: exit.rev(),
                };
            }
        }
        let _n_enter = self.edge_n_on_face(visit.face_idx, visit.enter).unwrap();
        // self.edge_path
        //     .push(visit.exit(self.edge_from_face(visit.face_idx, n_enter + 1)));
        self.update_transforms();
        Ok(())
    }

    pub fn add_face_to_path(&mut self, n_face_idx: usize) -> Option<usize> {
        if let Some(cr) = self.edge_path.last() {
            let fidx = cr.face_idx;
            let Ok(edge) = self.edge_from_two_faces(fidx, n_face_idx) else {
                return None;
            };
            // so now we add the next face, so it's a crossing now
            // except like this it would never like do anything
            // So... let's say that for this moment we don't really do anything in some way?
            // as in, the last
            self.edge_path.last_mut().unwrap().exit = edge;
            if self.face_path_index[n_face_idx].is_empty() {
                let rotate_amount = self.edge_n_on_face(n_face_idx, edge.rev()).unwrap();
                self.faces[n_face_idx].rotate_left(rotate_amount);
            }

            self.face_path_index[n_face_idx].push(self.edge_path.len());
            self.edge_path.push(PolygonCrossing {
                face_idx: n_face_idx,
                enter: edge.rev(),
                // designed to cause devastation if misused
                exit: Edge {
                    start: u32::MAX,
                    end: u32::MAX,
                },
            });

            self.update_transforms();
            None
        } else {
            // There is no path yet
            let enter = self.edge_from_face(n_face_idx, 0);
            self.face_path_index[n_face_idx].push(0);
            self.edge_path.push(PolygonCrossing {
                face_idx: n_face_idx,
                enter,
                // designed to cause devastation if misused
                exit: Edge {
                    start: u32::MAX,
                    end: u32::MAX,
                },
            });
            Some(n_face_idx)
        }
    }

    /// Push a new move onto the path
    ///
    /// This will pop the last crossing from the path and then check if that was
    /// a revisit. In that case, we're operating from a revisit.
    ///
    /// Additionally, the next face can be a revisit. If not, we'll rotate the face.
    ///
    /// TODO: check crossing rules; do we need to keep a face_path_index around?
    pub fn path_jump(&mut self, jumps: usize) -> Option<usize> {
        if let Some(last) = self.edge_path.pop() {
            // need to do a full path search, since it's already marked as visited previously
            // If we have a face_path_index, is there really a need for a visited array?
            let exit_n = if !self.edge_path.iter().any(|cr| cr.face_idx == last.face_idx) {
                // first time visiting this polygon
                // That means the exit edge is rotating clockwise and 1 extra
                jumps + 1
            } else {
                // revisiting polygon
                // so it's exactly opposite: enter - jumps
                let enter_n = self.edge_n_on_face(last.face_idx, last.enter).unwrap();
                enter_n.saturating_sub(jumps)
            };
            let exit = self.edge_from_face(last.face_idx, exit_n);
            // here directly push, since it's already in visits
            self.edge_path.push(last.to_visit().exit(exit));
            // now, add the next polygon
            let n_face_idx = self.other_face(last.face_idx, exit_n);
            // rotate it if first time visitor
            if self
                .edge_path
                .iter()
                .find(|cr| cr.face_idx == n_face_idx)
                .is_none()
            {
                info!("visiting {n_face_idx} for the first time");
                let rotate_amount = self.edge_n_on_face(n_face_idx, exit.rev()).unwrap();
                self.faces[n_face_idx].rotate_left(rotate_amount);
                self.update_transforms();
            } else {
                info!("already visited {n_face_idx}");
                if !self.can_revisit(n_face_idx, exit.rev()) {
                    info!("cannot revisit {n_face_idx:?}");
                    return None;
                }
            }
            self.push(PolygonVisit {
                face_idx: n_face_idx,
                enter: exit.rev(),
            });
            None
        } else {
            // There is no path yet, so we just get the nth triangle and call it start
            let Some(face_idx) = self.iter_ngon(3).cycle().nth(jumps) else {
                return None;
            };
            let enter = self.edge_from_face(face_idx, 0);
            self.push(PolygonVisit { face_idx, enter });
            Some(face_idx)
        }
    }

    pub fn pop_path(&mut self) -> Option<usize> {
        let Some(last) = self.edge_path.pop() else {
            return None;
        };
        self.face_path_index[last.face_idx].pop();
        self.edge_path.last().map(|cr| cr.face_idx)
    }

    pub fn clear_path(&mut self) {
        self.edge_path.clear();
        for f in &mut self.face_path_index {
            f.clear()
        }
    }

    /// Check if a face can be revisited from the given edge
    ///
    /// crossing rule:
    /// ```text
    ///    3
    /// 2 /-\ 4
    /// 1 \_/ 5
    ///    0
    /// ```
    /// 03 disallows 0,1,2,3:
    /// - 0 because it's enter
    /// - 1,2 because it's sandwiched
    /// - 3 because it's exit
    pub fn can_revisit(&self, face_idx: usize, edge: Edge) -> bool {
        if self.faces[face_idx].len() == 3 {
            return false;
        }
        // get neighbour face id
        // Not sure why we need that though?
        // maybe to check if we are in between two edges
        //
        // so the 02 13 case, which says 1 is illegal on an existing 02
        // For that, we only need to check if any _later_ edges are in the path
        // Since we rotate on first visit, this is correct

        !self.face_path_index[face_idx].iter().any(|&i| {
            let cr = self.edge_path[i];
            (self.edge_n_on_face(face_idx, cr.enter).unwrap() < i
                && self.edge_n_on_face(face_idx, cr.exit).unwrap() > i)
                || cr.enter == edge.rev()
                || cr.exit == edge.rev()
        })
    }

    /// Make a path, starting at the given face.
    ///
    /// If a path is found, will update self
    ///
    /// path is a path as found on self, and face_path_index is an index of where in the path a given face can be found
    pub(crate) fn dfs(&mut self, visit: PolygonVisit) -> bool {
        // So I mean, this works, but it's illegible. So what would be nice is
        // to have some face/edge-related functions on polyhedron....
        //
        // and store something edge-like, since idk.. they don't have _that_
        // much data and we need dihedral angle-thingies anyways in order to
        // tell people to not solder a led on sharp angles.
        //
        // Like yes, everything can be calculated, but trading memory for computation can do..
        //
        // So what is often done?
        // searching the path is kind of a necessary evil, since we want to have it ordered
        //
        // but the sad thing is this discrepancy between (face,id)<->(e0,e1) and
        // altogether I think the ordered-ness is very stokes and that's nice,
        // but it's kind of an unneeded extra requirement in an already rather
        // hard algorithm to also have to consider reversing edges?
        //
        // anyways, let's add a poly.edge(face_idx, id) or something?
        // it almost feels like having some silly type that is Face(Vec<u32>) just to be able to nicen the zip(skip) iterator hell?
        let fidx = visit.face_idx;

        if !self.face_path_index[fidx].is_empty() {
            error!(
                "should call revisit_dfs when revisiting {:?}",
                self.edge_path
            );
            return false;
        }
        // rotate poly so we're entering on edge 0-1
        let rotate_amount = self.edge_n_on_face(fidx, visit.enter).unwrap();
        self.faces[fidx].rotate_left(rotate_amount);
        self.face_path_index[fidx].push(self.edge_path.len());
        // success condition: all faces visited (this can only happen on first visit)
        if self.face_path_index.iter().all(|v| !v.is_empty()) {
            // add current visit
            // exit is devastating if misused
            self.edge_path.push(visit.exit((u32::MAX, u32::MAX).into()));
            return true;
        }

        // for dfs we want to go left first, then cycle around the polygon.
        //
        // Also if we're revisiting the polygon, then only check next n edges
        let face_edges = self.face_edges(fidx, 0).collect::<Vec<_>>();
        let mut revisits = Vec::new();
        let mut n_new_faces = 0;
        // skip entering edge and only test two??
        // no, that's wrong, we only test two _past the last revisit_
        for (i, edge) in face_edges.iter().enumerate().skip(1) {
            // check if we're visiting an already-crossed polygon
            //
            // here we check for all polyhedron faces if it contains this edge, which is kinda inefficient
            let n_face_idx = self.other_face(fidx, i);

            // If this face has already been visited, check the crossing rule: 0-2 1-3 is not allowed
            //
            // wait... on hexagon, 01  45  23 is allowed actually and below would disregard that
            // even though in the order thingy, that would make more sense

            // because it like goes around the polygon and then will come back at 5
            // and the best heuristic there is actually to try to exit asap as well, in stead of searching from 2
            // but whatevs
            //
            // maybe it'd also be very fast to check if it's visited and a triangle
            if !self.face_path_index[n_face_idx].is_empty() {
                // triangle shortcut (they can't be visited twice)
                if self.faces[n_face_idx].len() == 3 {
                    continue;
                }
                // get neighbour face id
                // Not sure why we need that though?
                // maybe to check if we are in between two edges
                //
                // so the 02 13 case, which says 1 is illegal on an existing 02
                // For that, we only need to check if any _later_ edges are in the path
                // Since we rotate on first visit, this is correct

                // crossing rule:
                // 03 disallows 0,1,2,3
                // 0 because it's enter
                // 1,2 because it's sandwiched
                // 3 because it's exit
                if self.can_revisit(n_face_idx, edge.rev()) {
                    revisits.push((
                        visit.exit(*edge),
                        PolygonVisit {
                            face_idx: n_face_idx,
                            enter: edge.rev(),
                        },
                    ));
                }
            } else {
                self.edge_path.push(visit.exit(*edge));
                if self.dfs(PolygonVisit {
                    face_idx: n_face_idx,
                    enter: edge.rev(),
                }) {
                    return true;
                } else {
                    self.edge_path.pop();
                }
                // we want to closely hug visited pcbs, so break before diverging
                // this greatly speeds up search time, but kinda sad
                n_new_faces += 1;
                if n_new_faces == 2 {
                    break;
                }
            }
        }
        for revisit in revisits {
            // we already added
            self.edge_path.push(revisit.0);
            if self.revisit_dfs(revisit.1) {
                return true;
            } else {
                self.edge_path.pop();
            }
        }

        // we were the ones visiting
        self.face_path_index[fidx].pop();
        false
    }

    fn revisit_dfs(&mut self, visit: PolygonVisit) -> bool {
        // ```
        //    3
        // 2 /-\ 4
        // 1 \_/ 5
        //    0
        // ```
        // Since we're spiralling, the most likely face we'd enter on is 5
        // then we want to check 2->4 in that order
        //
        // If in stead, we'd be entering through 3, we'd dfs 2,5,4
        // even though bla..
        // what's bla?
        // bla is that going in 5 direction will kind of force a spiral, because the 4-face needs to be visited and then is locked in
        // So... 5 is actually forcing a spiral,
        // but 4 would be forcing a spiral the other way around
        // so it's all a bit suboptimal to be entering on 3
        let fidx = visit.face_idx;
        let n = self.edge_n_on_face(fidx, visit.enter).unwrap();
        let largest_smaller_n = self.face_path_index[fidx]
            .iter()
            .filter_map(|&v| {
                let cr = self.edge_path[v];
                let e = self.edge_n_on_face(fidx, cr.exit).unwrap();
                if e < n { Some(e) } else { None }
            })
            .max()
            .unwrap_or(n);
        // If there's no larger n, we'll try till the end of the face
        let smallest_larger_n = self.face_path_index[fidx]
            .iter()
            .filter_map(|&v| {
                let cr = self.edge_path[v];
                let e = self.edge_n_on_face(fidx, cr.exit).unwrap();
                if e > n { Some(e) } else { None }
            })
            .min()
            .unwrap_or(self.faces[fidx].len());
        for edge_n in (largest_smaller_n..n).chain(n + 1..smallest_larger_n) {
            let e = self.edge_from_face(fidx, edge_n);
            let n_face_idx = self.other_face(fidx, edge_n);
            if self.face_path_index[n_face_idx].is_empty() {
                // so here we push the current face
                // the next face is pushed in dfs
                self.face_path_index[fidx].push(self.edge_path.len());
                self.edge_path.push(visit.exit(e));
                if self.dfs(PolygonVisit {
                    face_idx: n_face_idx,
                    enter: e.rev(),
                }) {
                    return true;
                } else {
                    self.face_path_index[fidx].pop();
                    self.edge_path.pop();
                }
            } else {
                // revisiting from a revisit, that's sad
                // so just give up?

                // if let Some(v) = path.pop() {
                //     face_path_index[v.face_idx].pop();
                // }
                // return false;
            }
        }
        // it could be that above for loop didn't run, so that's also a fail
        false
    }
}

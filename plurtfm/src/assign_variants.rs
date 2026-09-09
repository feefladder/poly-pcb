use wasm_bindgen::prelude::*;

use crate::{pcbdron::Pcbdron, *};

#[wasm_bindgen]
impl Interface {
    pub fn update_variant(&mut self, VarId { nth_ngon, pcb_id }: VarId) {
        self.scene
            .pcboron
            .set_variant(pcb_id.n_gon, nth_ngon, pcb_id.variant);
        self.render();
    }

    pub fn set_variant(&mut self, varid: VarId, new_var: usize) {
        // set the face to the variant
        let Some(f_idx) = self.scene.pcboron.varid_to_fidx(varid) else {
            warn!("Could not find face idx for {varid:?}");
            return;
        };
        let n_gon = varid.pcb_id.n_gon;
        let Some(nth_ngon) = self
            .scene
            .pcboron
            .pcbdrons()
            .iter()
            .enumerate()
            .flat_map(|(i, p)| p.polyhedron.iter_ngon(n_gon).zip(iter::repeat(i)))
            .position(|(fidx, dridx)| Fidx { dridx, fidx } == f_idx)
        else {
            warn!("could not find nth_ngon for {f_idx:?}");
            return;
        };
        self.scene.pcboron.put_variant(varid, new_var);
        self.maybe_request_variant(VarId {
            nth_ngon,
            pcb_id: PcbId {
                n_gon,
                variant: new_var,
            },
        });
        self.render();
    }

    pub(crate) fn maybe_request_variant(&self, var_id: VarId) {
        // so there's some shenanigans that happened in the refactor and it'd really be better to solve them earlier, but idk, so here's some uglyness
        //
        // so the problem is that varid changed it's meaning from nth ngon to nth variant
        // so that's sad and the ui really likes nth ngon, but idk how to get there?
        info!("requesting {var_id:?}");
        let e_detail = CustomEventInit::new();
        e_detail.set_detail(&var_id.into());
        self.canvas
            .dispatch_event(
                &CustomEvent::new_with_event_init_dict("update_variant", &e_detail).unwrap(),
            )
            .unwrap();
    }
}

impl Pcbdron {
    /// Apply the variant map and consume used variants
    ///
    ///
    pub fn apply_variant_map(&mut self, variant_map: &mut VariantMap) {
        self.variant_map.clear();
        self.variant_map.resize(self.polyhedron.faces.len(), 0);
        for (ngon, vars) in variant_map.iter_mut() {
            let mut counter = 0;
            for (face_idx, var) in self.polyhedron.iter_ngon(*ngon).zip(vars.iter()) {
                self.variant_map[face_idx] = *var;
                counter += 1;
            }
            vars.copy_within(counter.., 0);
            vars.truncate(vars.len() - counter);
        }
    }
}

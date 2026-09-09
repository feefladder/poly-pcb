//! all stuff related to selecting a pcboron/polyhedrons

use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;

use crate::{pcbdron::Pcbdron, pcboron::PcboronError, *};

#[wasm_bindgen]
#[cfg(target_arch = "wasm32")]
impl Interface {
    pub fn set_polyhedron(
        &mut self,
        polyhedron: String,
        index: usize,
    ) -> Result<Ts<MissingVariants>, JsError> {
        self.scene
            .pcboron
            .set_pcbdron(
                Polyhedron::load(&self.connection, &polyhedron).map_err(|e| (*e).clone())?,
                &mut Vec::new(),
                index,
            )
            .map_err(|e| (*e).clone())?;
        self.render();
        Ok(MissingVariants(polyhedron, self.missing_variants()).into_ts()?)
    }

    pub fn push_polyhedron(&mut self, polyhedron: String) -> Result<Ts<MissingVariants>, JsError> {
        self.scene.pcboron.push_polyhedron(
            Polyhedron::load(&self.connection, &polyhedron).map_err(|e| (*e).clone())?,
        );
        self.render();
        Ok(MissingVariants(polyhedron, self.missing_variants()).into_ts()?)
    }

    pub fn pop_polyhedron(&mut self) {
        self.scene.pcboron.pop_polyhedron();
        self.render();
    }
}

impl Pcboron {
    /// removes all pcbdrons and sets it to this single one
    ///
    /// simple function before adding more complexity
    pub fn set_pcbdron(
        &mut self,
        polyhedron: Polyhedron,
        variant_map: &mut VariantMap,
        nth: usize,
    ) -> exn::Result<(), PcboronError> {
        self.pcbdrons[nth].set_poly(polyhedron, variant_map);
        self.update_debug_path();
        // so we do set new faces here, but not change/update old ones?
        self.update_instances();
        Ok(())
    }

    pub fn push_polyhedron(&mut self, polyhedron: Polyhedron) {
        self.pcbdrons
            .push(Pcbdron::new(polyhedron, &mut VariantMap::new()));
        self.update_instances();
    }

    pub fn pop_polyhedron(&mut self) -> Option<Pcbdron> {
        let p = self.pcbdrons.pop();
        self.update_instances();
        self.update_debug_path();
        p
    }
}

impl Pcbdron {
    /// Set the polyhedron with the given variant map
    ///
    /// This will also clear the edge path
    pub fn set_poly(&mut self, polyhedron: Polyhedron, variant_map: &mut VariantMap) {
        self.polyhedron = polyhedron;
        self.polyhedron.edge_path.clear();
        self.apply_variant_map(variant_map);
    }
}

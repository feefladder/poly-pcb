use log::debug;
use three_d::{Mat4, One};

use crate::design::{PcbDrosign, PcbPath};
use crate::{PcbId, VariantMap, polyhedron::Polyhedron};
use crate::{VarFlags, VarId};

/// A PcbGon knows where which Pcb variant is on a polyhedron
///
/// this allows to have multiple polyhedra in one scene
///
/// pcb variants are still globally instanced, so it adds considerable bookkeeping
/// hencewhy I've added this here small struct.
///
/// not too sure if this should also have rotations and where the algorithm for
/// making a led strip from these thingies should live. I think here, but also want to have projection at some point
///
/// and projection is maybe a more higher-level thing? because for making a "proper lamp" we still need a bit more state than "which pcb on which face", also per-face "rotations", so guess
pub struct Pcbdron {
    /// the polyhedron
    pub polyhedron: Polyhedron,
    // the transform applied to self (if any, otherwise just identity)
    pub transform: Mat4,
    /// which variant lives on which face
    ///
    /// This _needs_ to cover all of polyhedron.faces, so it's different from
    /// possibly-incomplete VariantMap
    ///
    /// also not entirely sure why it's global and not per-ngon, but this maps easily to faces...
    pub variant_map: Vec<usize>,
    // /// Some mesh that helps in visualizing when rendering pcbs isn't appropriate
    // pub debug_model: Gm<Mesh, PhysicalMaterial>,
}

impl Pcbdron {
    pub fn new(polyhedron: Polyhedron, variant_map: &mut VariantMap) -> Self {
        let mut res = Self {
            transform: Mat4::one(),
            variant_map: vec![0; polyhedron.faces.len()],
            polyhedron,
        };
        res.apply_variant_map(variant_map);
        res
    }
    /// iterate over all faces that are this variant
    ///
    /// will return face index
    pub fn iter_variant(&self, pcb_id: PcbId) -> impl Iterator<Item = usize> {
        self.polyhedron
            .faces
            .iter()
            .enumerate()
            .zip(&self.variant_map)
            .filter(move |((_, f), v)| f.len() == pcb_id.n_gon && **v == pcb_id.variant)
            .map(|((i, _), _)| i)
    }

    pub fn get_design(&self) -> PcbDrosign {
        let polyhedron = self.polyhedron.name.to_owned();
        // convert from flat polygon-index to per-ngon-index
        // I think we've done this before somewhere?
        // well, only missing_variants comes close and that's clearly different, but can still copy over the code
        // Except let's make this a nice btreemap?
        let mut variant_map: VariantMap = VariantMap::with_capacity((3..11).len());
        for n_gon in 3..=10 {
            let variants: Vec<usize> = self
                .polyhedron
                .iter_ngon(n_gon)
                .map(|idx| self.variant_map[idx])
                .collect();
            if !variants.is_empty() {
                variant_map.push((n_gon, variants));
            }
        }

        let path = self.current_path();
        debug!("current path: {path:?}");
        PcbDrosign {
            polyhedron,
            variant_map,
            path,
        }
    }

    pub fn current_path(&self) -> Option<PcbPath> {
        self.polyhedron
            .current_path()
            .transpose()
            .unwrap_or_else(|p| Some(p))
    }

    /// Set the polyhedron with the given variant map
    ///
    /// This will also clear the edge path
    pub fn set_poly(&mut self, polyhedron: Polyhedron, variant_map: &mut VariantMap) {
        self.polyhedron = polyhedron;
        self.polyhedron.edge_path.clear();
        self.apply_variant_map(variant_map);
    }

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

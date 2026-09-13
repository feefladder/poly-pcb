use std::collections::BTreeMap;
use std::f32::consts::PI;

use log::{debug, info};
use three_d::{InnerSpace, Mat3, Mat4, One, SquareMatrix, Transform, VectorSpace};

use crate::design::PcbDrosign;
use crate::stereojection::Stereojection;
use crate::{PcbId, VariantMap, polyhedron::Polyhedron};

/// A Pcbdron knows where which Pcb variant is on a polyhedron
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
    #[allow(dead_code)]
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
    /// Projections
    /// These should be indexed by path index
    pub projections: BTreeMap<usize, Stereojection>,
}

impl Pcbdron {
    pub fn new(polyhedron: Polyhedron, variant_map: &mut VariantMap) -> Self {
        let mut res = Self {
            transform: Mat4::one(),
            variant_map: vec![0; polyhedron.faces.len()],
            polyhedron,
            projections: BTreeMap::new(),
        };
        res.apply_variant_map(variant_map);
        res.update_projections(None);
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
}

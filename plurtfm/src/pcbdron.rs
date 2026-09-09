use std::collections::BTreeMap;

use log::{debug, info};
use three_d::{Mat4, One};

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

    fn stereojection(&self, face_idx: usize) -> Stereojection {
        let p = &self.polyhedron;
        // put point on opposite side of sphere thing
        let s_trans = p.face_transforms[face_idx];
        let dist = 2.0 * p.mean_r();
        let point = s_trans.w.truncate() + dist * s_trans.z.truncate();
        // now I'm not entirely sure if I want to have like the plane also tuneable
        // but otherwise it'd be pretty easy and the original is just not transformed?
        // let's do that first
        let arrow = -s_trans.z.truncate();
        Stereojection { point, arrow, dist }
    }

    pub fn update_projections(&mut self, break_idx: Option<usize>) {
        self.projections.clear();
        // so in far future we want also to have like different possible break_idxes
        // as in, be able to... further specify projections
        // main example is nonconvex geometry such as start tetrahedron where each point wants its own litte sphere and they are joined like that
        // I think that's more general-ish, in some smart way, but for now, just do the path thing
        let break_idx = break_idx.unwrap_or(self.polyhedron.edge_path.len() / 2);
        // okidoki, now need make projection thing...
        // what is projection thing?
        //
        // take the first path face

        let start = self
            .polyhedron
            .edge_path
            .first()
            .map(|cr| cr.face_idx)
            .unwrap_or(0);

        info!("making stereojection from face {start:?}");
        self.projections.insert(0, self.stereojection(start));
        // if self.polyhedron.edge_path.len() <= 1 {
        //     return;
        // }
        // let end = self.polyhedron.edge_path.last().unwrap();
        // self.projections
        //     .insert(break_idx, self.stereojection(end.face_idx));
    }

    pub fn face_transform(&self, face_idx: usize) -> Mat4 {
        let ft = self.polyhedron.face_transforms[face_idx];
        if let Some((_, p)) = self.projections.range(..=face_idx).next_back() {
            let r = p.project(ft);
            r
        } else {
            ft
        }
    }
}

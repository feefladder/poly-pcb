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

    fn stereojection(&self, face_idx: usize) -> (Stereojection, Stereojection) {
        let p = &self.polyhedron;
        // put point on opposite side of sphere thing
        let s_trans = p.face_transforms[face_idx];
        let dist = 2.0 * p.mean_r();
        let point = s_trans.w.truncate() + dist * s_trans.z.truncate();
        // At this point have to calculate the thingy...
        // the distance between the two like closest faces on this side
        //
        // I guess I should really have a constant for edge length, so...
        // ok, so the like maximum distance is given by this
        let dist = (0..p.faces[face_idx].len())
            .map(|e| {
                // get neighbouring face
                let n_face = p.other_face(face_idx, e);
                // distance needed is ours + theirs + margin
                let x_dist = p.face_inradius(n_face) + 0.1 + p.face_inradius(face_idx);
                // now, need to find the thingy...
                // I guess we could just dot-product to get the distance?
                // because here we strongly assume that faces will be like projected and their angles preserved
                // so polygons move directly away from each other with aligned sides
                let n_dir = (p.face_transforms[n_face].w.truncate() - point).normalize();
                // so I guess we can use scale
                //     we're moving in direction b, and a is moving away at which speed?
                //   /|  also since the'yre normalized vecs, it's  all a bit easier
                //a / | b
                // /--|
                //  ? = a*sin(theta)
                // So that's a cross product?
                let sin = s_trans.z.truncate().cross(n_dir).magnitude();
                let cos = -s_trans.z.truncate().dot(n_dir);
                // and now we can calculate b
                // sos => sin = o/s
                // cas => cos = a/s
                // toa => tan = o/a = sin/cos
                //    /|   toa => tan = x_dist/?
                //   / | ? = x_dist/tan
                //  /  |   = x_dist*cos/sin
                // /---|
                //   x_dist
                let res = x_dist * cos / sin;
                info!("face {n_face} needs distance {x_dist}, so length {res}");
                res
            })
            // get max needed distance
            .max_by(|a, b| a.total_cmp(b))
            .unwrap();
        let arrow = Mat3::from_cols(
            s_trans.x.truncate(),
            s_trans.y.truncate(),
            -s_trans.z.truncate(),
        );
        let one = Stereojection { point, arrow, dist };
        let p2 = s_trans.w.truncate();
        let a2 = -arrow;
        let two = Stereojection {
            point: p2,
            arrow: a2,
            dist,
        };
        (one, two)
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
        let (startjection, endjection) = self.stereojection(start);
        self.projections.insert(0, startjection);
        self.projections
            .insert(self.polyhedron.faces.len() / 2, endjection);
    }

    pub fn face_transform(&self, face_idx: usize, amount: f32) -> Mat4 {
        let ft = self.polyhedron.face_transforms[face_idx];
        let path_idx = *self.polyhedron.face_path_index[face_idx]
            .get(0)
            .unwrap_or(&0);
        // get the last transform
        let mut ps = self.projections.range(..=path_idx);
        if let Some((path_idx, p)) = ps.next_back() {
            // get projection amount
            let mut r = p.project(ft, amount);
            if let Some(pp) = ps.next_back() {
                // there was a previous projection, so we're going to find that
                // and the face that is like nth in the path
                // so we get that face
                let border_face = self.polyhedron.face_path_index[*path_idx][0];
                // so now we need to do the thing where we like...
                // do the inverse transform of the matrix or something?
                // or well, so we parent ourselves to the projected border_face, then apply the transform of the projected border face in the previous projection
                // and how was that again?
                let border_og = self.polyhedron.face_transforms[border_face];
                // and then we do the parenting thing, which should be like the inverse?
                //
                // AT'=T <=> T'=inv(A)T
                assert_eq!(p.project(border_og, 0.0), border_og);
                r = pp.1.project(border_og, amount)
                    * p.project(border_og, amount).invert().unwrap()
                    * r;
                // now, we want to apply the other transform
                // r =  * r_rel;
            }
            r
        } else {
            // don't do shit; there are no transforms yet
            ft
        }
    }
}

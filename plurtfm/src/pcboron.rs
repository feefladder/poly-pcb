use std::{
    error::Error,
    iter::{self},
};

use derive_more::Display;
use exn::ResultExt;
use log::{debug, info};
use rusqlite::Connection;
use three_d::{
    ColorMaterial, Context, CpuMaterial, CpuMesh, CpuModel, Gm, InstancedMesh, InstancedModel,
    Instances, Mat4, Object, PhysicalMaterial, Srgba, Vec3, prelude::*,
};

use crate::{
    PcbId, VarFlags, VarId,
    design::{PcBorsign, PcbDrosign, VariantMap},
    make_path::PolygonCrossing,
    pcbdron::Pcbdron,
    polyhedron::Polyhedron,
};

/// [`Pcboron`] can be rendered as self-contained something
///
/// but doesn't know how to construct itself
///
/// importantly, it can go geometryid + instanceid -> varid
pub struct Pcboron {
    pub project_amount: f32,
    /// A linear list of pcbdrons
    ///
    /// The path will follow this order
    pub(crate) pcbdrons: Vec<Pcbdron>,
    /// The actual pcbs, including their transforms
    ///
    /// These are InstancedModels to support multi-mesh gltf pcbs
    pcb_models: Vec<InstancedModel<PhysicalMaterial>>,
    /// where is a specific pcb in instances?
    /// why is this not just Vec<PcbId>?
    instance_map: Vec<PcbId>,
    /// instances, these are constructed by chaining each pcbdron's variantmap
    ///
    /// keep it around so we can update, in stead of re-creating
    /// even though it's just a vec of transforms, so kinda useless
    /// maybe better keep Vec<Vec<Mat4>>
    instances: Vec<Instances>,
    /// arrow instances(transforms)
    path_instances: Instances,
    /// Show the path as ugly blue arrows
    ///
    /// This is a simple instancedmesh
    path_gm: Gm<InstancedMesh, ColorMaterial>,
}

#[derive(Debug, Display, Clone)]
pub struct PcboronError(String);
impl Error for PcboronError {}

impl From<String> for PcboronError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// A face index on a pcboron
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fidx {
    /// dron index
    pub dridx: usize,
    /// face index
    pub fidx: usize,
}

impl Pcboron {
    pub fn n(&self) -> usize {
        self.pcbdrons.len()
    }

    pub fn pcbdrons(&self) -> &[Pcbdron] {
        &self.pcbdrons
    }

    pub fn debug_path(&self) -> &Gm<InstancedMesh, ColorMaterial> {
        &self.path_gm
    }

    /// from geometry_id, instance_id, get nth variant
    pub fn pick(&self, geometry_id: u32, instance_id: u32) -> Option<VarId> {
        let id = geometry_id as usize;

        // let model_idx = self.pcb_models[id];
        let pcb_id = self.instance_map[id]; // else {
        //     info!(
        //         "model idx {model_idx} no in instance map {:?}",
        //         self.instance_map
        //     );
        //     return None;
        // };
        return Some(VarId {
            nth_ngon: instance_id as usize,
            pcb_id,
        });
    }

    /// Create a new Pcboron from a polyhedron, variant map and pcbs
    ///
    /// all pcbs will be uploaded to the GPU and they will be controlled through instancing
    pub fn new(
        context: &Context,
        polyhedron: Polyhedron,
        pcbs: &[Vec<Option<CpuModel>>],
        variant_map: &VariantMap,
    ) -> exn::Result<Self, PcboronError> {
        let _material = PhysicalMaterial::new_opaque(
            context,
            &CpuMaterial {
                albedo: Srgba::WHITE,
                ..Default::default()
            },
        );
        // so we have a per-ngon variant map, and that's nice but have to translate to per-face
        // and not too sure how to do that?
        let mut vmap = vec![0usize; polyhedron.faces.len()];

        for (ngon, vars) in variant_map {
            for (var, idx) in vars.iter().zip(polyhedron.iter_ngon(*ngon)) {
                vmap[idx] = *var;
            }
        }
        let path_instances = Instances {
            transformations: Vec::with_capacity(polyhedron.faces.len()),
            colors: Some(Vec::with_capacity(polyhedron.faces.len())),
            ..Default::default()
        };
        // I think it's simpler to just create an empty version and add pcbs later
        let pcbdron = Pcbdron {
            transform: Mat4::one(),
            variant_map: vmap,
            // debug_model: polyhedron
            //     .sphere(context, material)
            //     .or_raise(|| PcboronError("could not add debug sphere to Pcboron".to_string()))?,
            polyhedron,
            projections: Default::default(),
        };

        let mut res = Self {
            pcbdrons: vec![pcbdron],
            pcb_models: Vec::new(),
            instances: Vec::new(),
            instance_map: Vec::new(),
            path_gm: Gm::new(
                InstancedMesh::new(context, &path_instances, &CpuMesh::arrow(0.8, 0.5, 8)),
                ColorMaterial::new_opaque(
                    context,
                    &CpuMaterial {
                        albedo: Srgba::WHITE,
                        ..Default::default()
                    },
                ),
            ),
            path_instances,
            project_amount: 0.0,
        };
        for (n_gon, pcb_vars) in pcbs.iter().enumerate() {
            for (variant, pcb) in pcb_vars
                .iter()
                .enumerate()
                .filter_map(|(i, p)| p.as_ref().map(|pp| (i, pp)))
            {
                res.add_pcb(context, PcbId { n_gon, variant }, pcb)?;
            }
        }
        Ok(res)
    }

    /// Apply a [`PcBorsign`]
    pub fn apply_design(
        &mut self,
        design: PcBorsign,
        sqlite: &Connection,
    ) -> exn::Result<Option<PcBorsign>, PcboronError> {
        let PcBorsign {
            polyhedra,
            mut variant_map,
            path,
        } = design;
        let mut fail = false;
        let mut n_applied = 0;
        //aaaaaahhhhh, what is good way to zip the longer one of the two
        // like, build pcbdrons from polyhedra, but mutate in-place otherwise
        // I think it'd almost be the inverse, where we allow the PcBorSign to eat pcbdrons or something
        // but why is that better?
        //
        //actually I don't think it is, except that...
        // we basically want the pcbdrosign to "eat" or no give birht to.. a pcbdron
        // well, transform or birth a pcbdron
        // or push-or-insert
        // self.pcbdrons.resize_with(polyhedra.len(), Pcbdron::);

        for (i, polyhedron) in polyhedra.iter().enumerate() {
            info!("setting poly {i} to {polyhedron}");
            // If this is a new pcbdron, create it
            if self.pcbdrons.len() == i {
                let hedron = Polyhedron::load(sqlite, polyhedron).or_raise(|| {
                    format!("Could not load {polyhedron:?}, which is the {i}th polyhedron").into()
                })?;
                let mut bdron = Pcbdron::new(hedron, &mut variant_map);
                if path.get(i).is_none() || fail {
                    bdron.polyhedron.clear_path();
                } else {
                    // If there was a faulty path, mark error
                    if let Err(_e) = bdron.update_path(&path[i]) {
                        fail = true;
                    }
                }
                self.pcbdrons.push(bdron);
            // otherwise, update the existing
            } else {
                let PcbDrosign {
                    polyhedron: cpol,
                    variant_map: cmap,
                    path: cpath,
                } = self.pcbdrons[i].get_design();
                if polyhedron != &cpol {
                    // so here, it'd actually be better (I think?) or not?
                    self.pcbdrons[i].set_poly(
                        Polyhedron::load(sqlite, polyhedron).or_raise(|| {
                            format!("could not apply design for poly {}", polyhedron).into()
                        })?,
                        &mut variant_map,
                    );
                } else {
                    self.pcbdrons[i].apply_variant_map(&mut variant_map);
                }
                // so this would also do nothing
                if path.get(i).is_none() || fail {
                    self.pcbdrons[0].polyhedron.clear_path();
                } else if Some(&path[i]) != cpath.as_ref()
                    && let Err(_e) = self.pcbdrons[i].update_path(&path[i])
                {
                    fail = true;
                }
            }

            n_applied += 1;
        }
        self.pcbdrons.truncate(n_applied);
        self.update_instances();
        self.update_debug_path();
        if fail {
            Ok(Some(self.get_design()))
        } else {
            Ok(None)
        }
    }

    pub fn get_design(&self) -> PcBorsign {
        self.pcbdrons
            .iter()
            .fold(PcBorsign::default(), |sign, dron| {
                sign.add(dron.get_design())
            })
    }

    /// transforms associated with this variant
    ///
    /// Basically pcb transforms, but filter-mapped to only be one [`PcbId`]
    ///
    /// used when adding pcbs and updating transforms. This is not a method
    /// because in the latter case, we're also holding a `&mut` to other fields on `self`
    fn variant_transforms(
        pcbdrons: &[Pcbdron],
        pcb_id: PcbId,
        project_amount: f32,
    ) -> impl Iterator<Item = Mat4> {
        pcbdrons.iter().flat_map(move |p| {
            p.iter_variant(pcb_id)
                .filter(|&fidx| {
                    true || *p.polyhedron.face_path_index[fidx]
                        .get(0)
                        .unwrap_or(&usize::MAX)
                        < p.polyhedron.faces.len() / 2
                })
                .map(move |idx| p.face_transform(idx, project_amount))
        })
    }

    /// Add this pcb to self
    ///
    /// if this variant is in the variant_map, also properly adds pcbs at the transform locations
    pub fn add_pcb(
        &mut self,
        context: &Context,
        pcb_id: PcbId,
        model: &CpuModel,
    ) -> exn::Result<(), PcboronError> {
        //
        let transformations =
            Self::variant_transforms(&self.pcbdrons, pcb_id, self.project_amount).collect();
        let colors = None;
        // Some(
        //     self.pcbdron
        //         .iter_variant(pcb_id)
        //         .map(|i| {
        //             let c = PLASMA.eval_rational(i, self.pcbdron.polyhedron.faces.len());
        //             Srgba::new_opaque(c.r, c.g, c.b)
        //         })
        //         .collect(),
        // );
        let instances = Instances {
            transformations,
            colors,
            ..Default::default()
        };
        let instanced_model = InstancedModel::new(context, &instances, model)
            .or_raise(|| PcboronError(format!("could not add {pcb_id:?} to multihedron")))?;
        self.pcb_models.push(instanced_model);
        self.instance_map.push(pcb_id);
        self.instances.push(Instances::default());
        Ok(())
    }

    /// Update Pcb's GPU instances
    ///
    /// Call this whenever variants or polyhedra change
    pub fn update_instances(&mut self) {
        // clear-and-rebuild for now, would be better to remove-insert later
        // first build own instances, then upload to GPU by changing pcb_models
        for (i, pcb_model) in self.pcb_models.iter_mut().enumerate() {
            // a pcb_model is a single pcb, but contains more than one mesh for different parts
            let pcb_id = self.instance_map[i];
            // set own instances to face transforms
            let transforms =
                Self::variant_transforms(&self.pcbdrons, pcb_id, self.project_amount).collect();
            self.instances[i].transformations = transforms;

            // optional debug colors
            // self.instances[i].colors = Some(
            //     self.pcbdron
            //         .iter_variant(pcb_id)
            //         .map(|i| {
            //             let c = BLUES.eval_rational(i, self.pcbdron.polyhedron.faces.len());
            //             Srgba::new_opaque(c.r, c.g, c.b)
            //         })
            //         .collect(),
            // );
            // kind of destructive but if colors array is shorter than
            // transforms, get opaque error
            self.instances[i].colors = None;
            // and upload to gpu
            pcb_model
                .iter_mut()
                .for_each(|pm| pm.geometry.set_instances(&self.instances[i]));
        }
    }

    fn nth_ngon(&self, ngon: usize, nth_ngon: usize) -> Option<(usize, usize)> {
        self.pcbdrons
            .iter()
            .enumerate()
            .flat_map(|(i, p)| p.polyhedron.iter_ngon(ngon).zip(iter::repeat(i)))
            .nth(nth_ngon)
    }

    /// Set the given ngon to this variant
    ///
    ///
    pub fn set_variant(&mut self, ngon: usize, nth_ngon: usize, variant: usize) {
        let Some((fidx, dridx)) = self.nth_ngon(ngon, nth_ngon) else {
            return;
        };
        self.pcbdrons[dridx].variant_map[fidx] = variant;

        self.update_instances();
    }

    pub fn varid_to_fidx(&self, VarId { nth_ngon, pcb_id }: VarId) -> Option<Fidx> {
        self.pcbdrons
            .iter()
            .enumerate()
            .flat_map(|(dridx, p)| p.iter_variant(pcb_id).zip(iter::repeat(dridx)))
            .nth(nth_ngon)
            .map(|(fidx, dridx)| Fidx { fidx, dridx })
    }

    pub fn put_variant(&mut self, varid: VarId, new_var: usize) {
        let Some(Fidx { dridx, fidx }) = self.varid_to_fidx(varid) else {
            return;
        };
        info!("setting {dridx} {fidx} to {new_var}");
        self.pcbdrons[dridx].variant_map[fidx] = new_var;
        self.update_instances();
    }

    /// ok, here's a very hacky but brilliant idea:
    ///
    /// so the last exit edge of a path is like not really used, and for
    /// devastation I've set it to Edge::from((usize::MAX, usize::MAX)), which
    /// ensures it overflows any place it is used.
    ///
    /// therefore, it's safe to say that it's useless at this point.
    ///
    /// so in that case, the semantics could be changed, where it indicates that in stead of being a polyhedron edge, it's a face-face edge from one pcbdron to the next
    /// so it'll just make that debug path nicely, but have to work around possible devastation
    pub fn update_debug_path(&mut self) {
        // so here we basically want to have arrows that point in the right directions or something
        // maybe we can also do that with an instancedmodel of an arrow?

        let instances = &mut self.path_instances;
        let transforms = &mut instances.transformations;
        transforms.clear();
        let colors = instances.colors.as_mut().unwrap();
        colors.clear();
        for dron in &self.pcbdrons {
            for (_, p) in &dron.projections {
                info!("making arrow for projection {p:?}");
                transforms.push(from_to_transform(
                    p.point,
                    p.point + p.arrow.z * p.dist,
                    p.point.normalize().cross(Vec3::unit_z()),
                ));
                colors.push(Srgba::BLUE);
            }

            let hedron = &dron.polyhedron;

            // we still want to clear everything on "no path"
            //
            // Since the path is monotonically increasing with drons, we can
            // return
            //
            // avoiding the overflow-subtract below
            if hedron.edge_path.is_empty() {
                self.path_gm.set_instances(&self.path_instances);
                return;
            }
            let imax = hedron.edge_path.len() - 1;
            for (
                i,
                PolygonCrossing {
                    face_idx,
                    enter,
                    exit,
                },
            ) in hedron.edge_path.iter().enumerate()
            {
                let proj = dron.projections.range(..=i).next_back();
                let p = |p: Mat4| {
                    if let Some(pr) = proj {
                        pr.1.project(p, self.project_amount)
                    } else {
                        p
                    }
                };
                if i == imax {
                    // let edge_n = hedron.edge_n_on_face(*face_idx, *enter).unwrap();
                    // let n_face_idx = hedron.other_face(*face_idx, edge_n);
                    // for the last, there is no crossing, so we add the enter arrow from the last one
                    // (this made more sense wrt. serializing a path)
                    // except it messes everything up in case we're manually making a path
                    // because in that case the exit edge is bs
                    // so...
                    // aah...
                    // ehh...
                    //
                    transforms.push(p(from_to_transform(
                        hedron.edge_centroid(*enter),
                        hedron.face_centroid(*face_idx),
                        hedron.face_normal(*face_idx),
                    )));
                    let c = colorous::MAGMA.eval_rational(i, imax.max(1));
                    colors.push(Srgba::new_opaque(c.r, c.g, c.b));
                    // also add the arrow (if it exists) from this poly to the next
                    if exit.start as usize == *face_idx
                        && let Some(next_dron) = self.pcbdrons.get(i + 1)
                    {
                        // there could be the case that the user added a
                        // smaller dron and the edge ran out-of-sync
                        //
                        // ignore
                        let from = hedron.face_centroid(*face_idx);
                        let to = next_dron.polyhedron.face_centroid(exit.end as usize);
                        // just pick a slightly random vec and orthogonize
                        let mut z = hedron.face_transforms[*face_idx].x.truncate();
                        z -= z * z.dot((from - to).normalize());
                        transforms.push(from_to_transform(from, to, z));
                    }
                } else if i == 0 && VarFlags::Controller.has(dron.variant_map[*face_idx]) {
                    // for the first, just give the output arrow
                    transforms.push(p(from_to_transform(
                        hedron.face_centroid(*face_idx),
                        hedron.edge_centroid(*exit),
                        hedron.face_normal(*face_idx),
                    )));
                } else {
                    // point from edge to edge
                    transforms.push(p(from_to_transform(
                        hedron.edge_centroid(*enter),
                        hedron.edge_centroid(*exit),
                        hedron.face_normal(*face_idx),
                    )));
                }
                let c = colorous::MAGMA.eval_rational(i, imax.max(1));
                colors.push(Srgba::new_opaque(c.r, c.g, c.b));
            }
        }
        // build instances
        //         self.path_instances
        self.path_gm.set_instances(&self.path_instances);
    }

    /// Iterate only over the bodies of the board
    ///
    /// Kind of hacky depending on the index of the board body in the gltfs from kicad exports
    ///
    /// but it works, eh?
    pub fn body_iter(&self) -> impl Iterator<Item = &dyn Object> {
        self.pcb_models
            .iter()
            .flat_map(|pm| iter::once(&pm[2] as &dyn Object))
    }

    /// Iterate over all modelparts
    pub fn into_iter(&self) -> impl Iterator<Item = &dyn Object> {
        self.pcb_models.iter().flat_map(|pm| pm.into_iter())
    }
}

/// For making an arrow, width is set to the constant 0.1
///
/// `z` is assumed orthonormal wrt `start->end`
fn from_to_transform(start: Vec3, end: Vec3, z: Vec3) -> Mat4 {
    const WIDTH: f32 = 0.1;
    let w = start;
    let x = end - start;
    let y = z.cross(x).normalize();
    Mat4::from_cols(
        x.extend(0.0),
        y.extend(0.0) * WIDTH,
        z.extend(0.0) * WIDTH,
        w.extend(1.0),
    )
}

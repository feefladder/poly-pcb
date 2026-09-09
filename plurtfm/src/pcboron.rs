use std::{
    error::Error,
    iter::{self},
};

use derive_more::Display;
use exn::ResultExt;
use log::info;
use rusqlite::Connection;
use three_d::{
    ColorMaterial, Context, CpuMaterial, CpuMesh, CpuModel, Gm, InstancedMesh, InstancedModel,
    Instances, Mat4, Object, PhysicalMaterial, Srgba, Vec3, prelude::*,
};

use crate::{
    PcbId, VarFlags, VarId,
    design::{PcBorsign, PcbDrosign, PcbPaths, VariantMap},
    pcbdron::Pcbdron,
    polyhedron::{PolygonCrossing, Polyhedron},
};

/// [`Pcboron`] can be rendered as self-contained something
///
/// but doesn't know how to construct itself
///
/// importantly, it can go geometryid + instanceid -> faceid
pub struct Pcboron {
    /// A linear list of pcbdrons
    ///
    /// The path will follow this order
    pcbdrons: Vec<Pcbdron>,
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
    // /// The number of unvisited faces in each pcbdron's path
    // ///
    // /// Not sure if I like this though, maybe just have a boolean?
    // /// because...
    // /// well, the pcbdron doesn't know if it's path is completed
    // /// also it doesn't really care, in theory a path could like jump faces and such
    // unpathed_faces: Vec<usize>,
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

    pub fn pcbdrons_mut(&mut self) -> impl Iterator<Item = &mut Pcbdron> {
        self.pcbdrons.iter_mut()
    }

    /// from geometry_id, instance_id, get face id
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
        // find which geometry corresponds to which model
        //
        // since models can have multiple geometries, need subtract len in stead
        // of just iterating
        for (model_idx, geometries) in self.pcb_models.iter().enumerate() {
            if id < geometries.len() {
                // here model_idx is what we want, no need to find which face
                // now need to get from model_idx -> PcbId{n_gon, variant},
                // so then we can use instance_id to find polyhedron face
                // and that's actually exactly what instance_map does
                let Some(pcb_id) = self.instance_map.get(model_idx) else {
                    info!(
                        "model idx {model_idx} no in instance map {:?}",
                        self.instance_map
                    );
                    return None;
                };

                // now need find nth
                //
                // this function needs to be kept in sync with update_instances
                // machinery:
                //
                //
                // what is going on?
                // the pick returns a model id, instance id.
                // from model id, we get pcb_id
                // in case of multiple pcbdrons, we chain them together
                //
                // and the instance_id needs to be retrievable by just going
                // in-order over the variants
                return Some(VarId {
                    nth_ngon: instance_id as usize,
                    pcb_id: *pcb_id,
                });
            }
            id -= geometries.len();
        }
        None
    }

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

    pub fn push_polyhedron(&mut self, polyhedron: Polyhedron) {
        self.pcbdrons
            .push(Pcbdron::new(polyhedron, &mut VariantMap::new()));
        self.update_instances();
        self.update_debug_path();
    }

    pub fn pop_polyhedron(&mut self) -> Option<Pcbdron> {
        let p = self.pcbdrons.pop();
        self.update_instances();
        self.update_debug_path();
        p
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
            if self.pcbdrons.len() == i {
                self.pcbdrons.push(Pcbdron::new(
                    Polyhedron::load(sqlite, polyhedron).or_raise(|| {
                        format!("Could not load {polyhedron:?}, which is the {i}th polyhedron")
                            .into()
                    })?,
                    &mut variant_map,
                ));
            }

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
            } else if variant_map != cmap {
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

    fn variant_transforms(pcbdrons: &[Pcbdron], pcb_id: PcbId) -> impl Iterator<Item = Mat4> {
        pcbdrons.iter().flat_map(move |p| {
            p.iter_variant(pcb_id)
                .map(|idx| p.polyhedron.face_transforms[idx])
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
        let transformations = Self::variant_transforms(&self.pcbdrons, pcb_id).collect();
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
            let transforms = Self::variant_transforms(&self.pcbdrons, pcb_id).collect();
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

    pub fn get_path(&self) -> PcbPaths {
        self.pcbdrons
            .iter()
            .flat_map(|d| d.current_path())
            .collect::<Vec<_>>()
            .into()
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
        instances.transformations.clear();
        let colors = instances.colors.as_mut().unwrap();
        colors.clear();
        for dron in &self.pcbdrons {
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
                    instances.transformations.push(from_to_transform(
                        hedron.edge_centroid(*enter),
                        hedron.face_centroid(*face_idx),
                        hedron.face_normal(*face_idx),
                    ));
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
                        instances
                            .transformations
                            .push(from_to_transform(from, to, z));
                    }
                } else if i == 0 && VarFlags::Controller.has(dron.variant_map[*face_idx]) {
                    // for the first, just give the output arrow
                    instances.transformations.push(from_to_transform(
                        hedron.face_centroid(*face_idx),
                        hedron.edge_centroid(*exit),
                        hedron.face_normal(*face_idx),
                    ));
                } else {
                    // point from edge to edge
                    instances.transformations.push(from_to_transform(
                        hedron.edge_centroid(*enter),
                        hedron.edge_centroid(*exit),
                        hedron.face_normal(*face_idx),
                    ));
                }
                let c = colorous::MAGMA.eval_rational(i, imax.max(1));
                colors.push(Srgba::new_opaque(c.r, c.g, c.b));
            }
        }
        // build instances
        //         self.path_instances
        self.path_gm.set_instances(&self.path_instances);
    }

    pub fn body_iter(&self) -> impl Iterator<Item = &dyn Object> {
        self.pcb_models
            .iter()
            .flat_map(|pm| iter::once(&pm[2] as &dyn Object))
    }

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

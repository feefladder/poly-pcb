use std::fmt::Debug;
use std::iter;
use std::{error::Error, iter::FlatMap};

use derive_more::Display;
use exn::ResultExt;
use log::{debug, info};
use rusqlite::Connection;
use three_d::{
    Axes, ColorMaterial, Context, CpuMaterial, CpuMesh, CpuModel, Gm, InnerSpace, InstancedMesh,
    InstancedModel, Instances, Mat4, Matrix4, Mesh, Object, One, PhysicalMaterial, Srgba, Vec3,
};
use wasm_bindgen::convert::OptionIntoWasmAbi;
use wasm_bindgen::instance;

use crate::design::{LampDesign, PcbDesign, PcbPath, PcbPaths};
use crate::polyhedron::PolygonCrossing;
use crate::{PcbId, VariantMap, polyhedron::Polyhedron};
use crate::{VarFlags, VarId, pcbdron, polyhedron};

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
    /// Some mesh that helps in visualizing when rendering pcbs isn't appropriate
    pub debug_model: Gm<Mesh, PhysicalMaterial>,
}

impl Pcbdron {
    /// iterate over all faces that are this variant
    ///
    /// will return face index
    fn iter_variant(&self, pcb_id: PcbId) -> impl Iterator<Item = usize> {
        self.polyhedron
            .faces
            .iter()
            .enumerate()
            .zip(&self.variant_map)
            .filter(move |((_, f), v)| f.len() == pcb_id.n_gon && **v == pcb_id.variant)
            .map(|((i, _), _)| i)
    }

    pub fn get_design(&self) -> PcbDesign {
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
        PcbDesign {
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
    pub fn set_poly(&mut self, polyhedron: Polyhedron, variant_map: &VariantMap) {
        self.polyhedron = polyhedron;
        self.polyhedron.edge_path.clear();
        self.apply_variant_map(variant_map);
    }

    pub fn apply_variant_map(&mut self, variant_map: &VariantMap) {
        self.variant_map.clear();
        self.variant_map.resize(self.polyhedron.faces.len(), 0);
        for (n, vars) in variant_map {
            for (i, var) in self.polyhedron.iter_ngon(*n).zip(vars) {
                self.variant_map[i] = *var;
            }
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
    fn set_controller(&mut self, face_idx: usize) -> Option<VarId> {
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
                res = Some(VarId {
                    nth_ngon,
                    pcb_id,
                    // actually we don't know at this point
                    need_fetch: false,
                });
            } else {
                VarFlags::Controller.rm(v);
            }
        }
        res
    }
}

/// [`MultiPcbdron`] can be rendered as self-contained something
///
/// but doesn't know how to construct itself
///
/// importantly, it can go geometryid + instanceid -> faceid
pub struct MultiPcbdron {
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
pub struct MultiPcbdronError(String);
impl Error for MultiPcbdronError {}

impl From<String> for MultiPcbdronError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl MultiPcbdron {
    pub fn pcbdrons(&self) -> impl Iterator<Item = &Pcbdron> + Clone {
        self.pcbdrons.iter()
    }

    pub fn debug_path(&self) -> &Gm<InstancedMesh, ColorMaterial> {
        &self.path_gm
    }

    pub fn pcbdrons_mut(&mut self) -> impl Iterator<Item = &mut Pcbdron> {
        self.pcbdrons.iter_mut()
    }

    /// from geometry_id, instance_id, get face id
    pub fn pick(&self, geometry_id: u32, instance_id: u32) -> Option<usize> {
        let mut id = geometry_id as usize;

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
                return self
                    .pcbdrons()
                    .flat_map(|p| p.iter_variant(*pcb_id))
                    .nth(instance_id as usize);
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
        variant_map: &VariantMap,
    ) -> exn::Result<(), MultiPcbdronError> {
        self.pcbdrons.truncate(1);
        self.pcbdrons[0].set_poly(polyhedron, variant_map);
        self.update_debug_path();
        // so we do set new faces here, but not change/update old ones?
        self.update_instances();
        Ok(())
    }

    /// Create a new MultiPcbdron from a polyhedron, variant map and pcbs
    ///
    /// all pcbs will be uploaded to the GPU and they will be controlled through instancing
    pub fn new(
        context: &Context,
        polyhedron: Polyhedron,
        pcbs: &[Vec<Option<CpuModel>>],
        variant_map: &VariantMap,
    ) -> exn::Result<Self, MultiPcbdronError> {
        let material = PhysicalMaterial::new_opaque(
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
            debug_model: polyhedron.sphere(context, material).or_raise(|| {
                MultiPcbdronError("could not add debug sphere to multipcbdron".to_string())
            })?,
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

    pub fn apply_design(
        &mut self,
        design: LampDesign,
        sqlite: &Connection,
    ) -> exn::Result<Option<LampDesign>, MultiPcbdronError> {
        let LampDesign::SinglePoly(PcbDesign {
            polyhedron,
            variant_map,
            path,
        }) = design;
        let PcbDesign {
            polyhedron: cpol,
            variant_map: cmap,
            path: cpath,
        } = self.pcbdrons[0].get_design();
        if polyhedron != cpol {
            self.pcbdrons[0].set_poly(
                Polyhedron::load(sqlite, &polyhedron).or_raise(|| {
                    format!("could not apply design for poly {}", polyhedron).into()
                })?,
                &variant_map,
            );
        } else if variant_map != cmap {
            self.pcbdrons[0].apply_variant_map(&variant_map);
        }
        let res = if path != cpath {
            match path {
                Some(mut p) => match self.pcbdrons[0].update_path(&p) {
                    Err(path_len) => {
                        p.turns.truncate(path_len);
                        Ok(Some(LampDesign::SinglePoly(PcbDesign {
                            polyhedron,
                            variant_map,
                            path: Some(p),
                        })))
                    }
                    Ok(_) => Ok(None),
                },
                None => {
                    self.pcbdrons[0].polyhedron.edge_path.clear();
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        };
        self.update_instances();
        self.update_debug_path();
        res
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
    ) -> exn::Result<(), MultiPcbdronError> {
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
            .or_raise(|| MultiPcbdronError(format!("could not add {pcb_id:?} to multihedron")))?;
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
        if let Some(todron) = self
            .pcbdrons
            .iter_mut()
            .find(|p| !p.polyhedron.path_complete_questionmark())
        {
            todron.polyhedron.complete_path();
            self.update_instances();
            self.update_debug_path();
        }
    }

    /// Add a face to the path
    /// face indices are local and based on the
    pub fn add_face_to_path(&mut self, face_idx: usize) -> Option<VarId> {
        info!("adding face {face_idx}");
        let mut res = None;
        if let Some((fidx, dron)) = self
            .pcbdrons
            .iter_mut()
            .find(|d| d.polyhedron.path_complete_questionmark())
            .map(|p| p.polyhedron.add_face_to_path(face_idx).zip(Some(p)))
            .flatten()
        {
            dron.set_controller(fidx);
        }
        self.update_instances();
        self.update_debug_path();
        res
    }

    /// pop the last index from the path
    ///
    /// somethingsomething about needing a linear path
    /// so even if a path has multiple like pcbdrons, it's still not allowed to be patchy
    pub fn pop_path(&mut self) {
        if let Some(activedron) = self.pcbdrons.iter_mut().rfind(|dron| {
            !dron.polyhedron.edge_path.is_empty() && dron.polyhedron.path_complete_questionmark()
        }) {
            activedron.polyhedron.edge_path.pop();
            self.update_debug_path();
        }
    }

    pub fn push_path(&mut self, jumps: usize) -> Option<VarId> {
        let mut res = None;
        if let Some(activedron) = self.pcbdrons_mut().find(|dron| {
            !dron.polyhedron.edge_path.is_empty() && dron.polyhedron.path_complete_questionmark()
        }) {
            if let Some(fidx) = activedron.polyhedron.push_path(jumps) {
                activedron.set_controller(fidx);
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

    /// Set the given ngon to this variant
    ///
    ///
    pub fn set_variant(&mut self, ngon: usize, nth_ngon: usize, variant: usize) {
        if let Some((fidx, dridx)) = self
            .pcbdrons
            .iter()
            .enumerate()
            .flat_map(|(i, p)| p.polyhedron.iter_ngon(ngon).zip(iter::repeat(i)))
            .nth(nth_ngon)
        {
            self.pcbdrons[dridx].variant_map[fidx] = variant;
            self.update_instances();
        }
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
        // for hedron in self.pcbdrons.iter().filter(predicate) {
        todo!();
        let hedron = &self.pcbdrons[0].polyhedron;
        // we still want to clear everything on "no path"
        // so then we return early, avoiding the overflow-subtract below
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
            } else if i == 0 && VarFlags::Controller.has(self.pcbdron.variant_map[*face_idx]) {
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
        // build instances
        //         self.path_instances
        self.path_gm.set_instances(&self.path_instances);
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

impl<'a> IntoIterator for &'a MultiPcbdron {
    type Item = &'a dyn Object;
    type IntoIter = FlatMap<
        std::slice::Iter<'a, InstancedModel<PhysicalMaterial>>,
        std::vec::IntoIter<&'a dyn Object>,
        fn(&'a InstancedModel<PhysicalMaterial>) -> std::vec::IntoIter<&'a dyn Object>,
    >;
    fn into_iter(self) -> Self::IntoIter {
        self.pcb_models.iter().flat_map(|pm| pm.into_iter())
    }
}

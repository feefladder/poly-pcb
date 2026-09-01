use std::fmt::Debug;
use std::{error::Error, iter::FlatMap};

use derive_more::Display;
use exn::ResultExt;
use log::{debug, info};
use rusqlite::Connection;
use three_d::{
    Axes, ColorMaterial, Context, CpuMaterial, CpuMesh, CpuModel, Gm, InnerSpace, InstancedMesh,
    InstancedModel, Instances, Mat4, Matrix4, Mesh, Object, One, PhysicalMaterial, Srgba, Vec3,
};
use wasm_bindgen::instance;

use crate::VarFlags;
use crate::design::{LampDesign, PcbDesign, PcbPath};
use crate::polyhedron::PolygonCrossing;
use crate::{PcbId, VariantMap, polyhedron::Polyhedron};

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

        let path = self.polyhedron.current_path().unwrap_or_else(|p| p);
        debug!("current path: {path:?}");
        PcbDesign {
            polyhedron,
            variant_map,
            path,
        }
    }

    pub fn set_poly(&mut self, polyhedron: Polyhedron, variant_map: &VariantMap) {
        self.polyhedron = polyhedron;
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
}

/// [`MultiPcbdron`] can be rendered as self-contained something
///
/// but doesn't know how to construct itself
///
/// importantly, it can go geometryid + instanceid -> faceid
pub struct MultiPcbdron {
    pcbdron: Pcbdron,
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
pub struct MultiPcbdronError(String);
impl Error for MultiPcbdronError {}

impl From<String> for MultiPcbdronError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl MultiPcbdron {
    pub fn pcbdrons(&self) -> impl Iterator<Item = &Pcbdron> {
        std::iter::once(&self.pcbdron)
    }

    pub fn debug_path(&self) -> &Gm<InstancedMesh, ColorMaterial> {
        &self.path_gm
    }

    pub fn pcbdrons_mut(&mut self) -> impl Iterator<Item = &mut Pcbdron> {
        std::iter::once(&mut self.pcbdron)
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
                // There is something bla going on here, where in case of multiple pcbdrons, we'd be chaining them together
                // and because of that, the below is already kinda correct
                return self.pcbdron.iter_variant(*pcb_id).nth(instance_id as usize);
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
        self.pcbdron.set_poly(polyhedron, variant_map);
        // so we do set new faces here, but not change/update old ones?
        self.update_instances().or_raise(|| {
            MultiPcbdronError(format!(
                "failed changing to {:?}",
                self.pcbdron.polyhedron.name
            ))
        })?;
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
            pcbdron,
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
        let LampDesign::SinglePoly(mut d) = design;
        let current_design = self.pcbdron.get_design();
        if d.polyhedron != current_design.polyhedron {
            self.pcbdron.set_poly(
                Polyhedron::load(sqlite, &d.polyhedron).or_raise(|| {
                    format!("could not apply design for poly {}", d.polyhedron).into()
                })?,
                &d.variant_map,
            );
        } else if d.variant_map != current_design.variant_map {
            self.pcbdron.apply_variant_map(&d.variant_map);
        }
        let res = if d.path != current_design.path {
            match self.pcbdron.update_path(&d.path) {
                Err(path_len) => {
                    d.path.turns.truncate(path_len);
                    Ok(Some(LampDesign::SinglePoly(d)))
                }
                Ok(_) => Ok(None),
            }
        } else {
            Ok(None)
        };
        self.update_instances()?;
        self.update_debug_path();
        res
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
        let transformations = self
            .pcbdron
            .iter_variant(pcb_id)
            .map(|idx| self.pcbdron.polyhedron.face_transforms[idx])
            .collect();
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
    pub fn update_instances(&mut self) -> exn::Result<(), MultiPcbdronError> {
        // clear-and-rebuild for now, would be better to remove-insert later
        // first build own instances, then upload to GPU by changing pcb_models
        for (i, pcb_model) in self.pcb_models.iter_mut().enumerate() {
            // a pcb_model is a single pcb, but contains more than one mesh for different parts
            let pcb_id = self.instance_map[i];
            // set own instances to face transforms
            self.instances[i].transformations = self
                .pcbdron
                .iter_variant(pcb_id)
                .map(|idx| self.pcbdron.polyhedron.face_transforms[idx])
                .collect();
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
        Ok(())
    }

    pub fn update_debug_path(&mut self) {
        // so here we basically want to have arrows that point in the right directions or something
        // maybe we can also do that with an instancedmodel of an arrow?
        let hedron = &self.pcbdron.polyhedron;

        let instances = &mut self.path_instances;
        instances.transformations.clear();
        let colors = instances.colors.as_mut().unwrap();
        colors.clear();

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
            if i == 0 && VarFlags::Controller.has(self.pcbdron.variant_map[*face_idx]) {
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
            let c = colorous::MAGMA.eval_rational(i, imax);
            colors.push(Srgba::new_opaque(c.r, c.g, c.b));
            if i == imax {
                let edge_n = hedron.edge_n_on_face(*face_idx, *exit).unwrap();
                let n_face_idx = hedron.other_face(*face_idx, edge_n);
                // for the last, there is no crossing, so we add the enter arrow from the last one
                // (this made more sense wrt. serializing a path)
                instances.transformations.push(from_to_transform(
                    hedron.edge_centroid(*exit),
                    hedron.face_centroid(n_face_idx),
                    hedron.face_normal(n_face_idx),
                ));
                let c = colorous::MAGMA.eval_rational(i, imax);
                colors.push(Srgba::new_opaque(c.r, c.g, c.b));
            }
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

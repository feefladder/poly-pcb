use std::{error::Error, iter::FlatMap};

use derive_more::Display;
use exn::ResultExt;
use log::info;
use three_d::{
    Context, CpuMaterial, CpuModel, Gm, InstancedModel, Instances, Mat4, Mesh, Object, One,
    PhysicalMaterial, Srgba,
};

use crate::{VariantMap, polyhedron::Polyhedron, ui::PcbId};

/// A PcbGon knows where which Pcb variant is on a polyhedron
///
/// this allows to have multiple polyhedra in one scene
///
/// pcb variants are still globally instanced, so it adds considerable bookkeeping
/// hencewhy I've added this here small struct.
pub struct Pcbdron {
    /// the polyhedron
    pub polyhedron: Polyhedron,
    // the transform applied to self (if any, otherwise just identity)
    pub transform: Mat4,
    /// which variant lives on which face
    ///
    /// This _needs_ to cover all of polyhedron.faces, so it's different from
    /// possibly-incomplete VariantMap
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
}

/// [`MultiPcbdron`] can be rendered as self-contained something
///
/// but doesn't know how to construct itself
///
/// importantly, it can go geometryid + instanceid -> faceid
pub struct MultiPcbdron {
    pcbdron: Pcbdron,
    /// The actual pcbs, including their transforms
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
}

#[derive(Debug, Display, Clone)]
pub struct MultiPcbdronError(String);
impl Error for MultiPcbdronError {}

impl MultiPcbdron {
    pub fn pcbdrons(&self) -> impl Iterator<Item = &Pcbdron> {
        std::iter::once(&self.pcbdron)
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
        self.pcbdron.variant_map.clear();
        self.pcbdron.variant_map.resize(polyhedron.faces.len(), 0);
        for (n, vars) in &variant_map.0 {
            for (i, var) in polyhedron
                .faces
                .iter()
                .enumerate()
                .filter_map(|(i, f)| if f.len() == *n { Some(i) } else { None })
                .zip(vars)
            {
                self.pcbdron.variant_map[i] = *var;
            }
        }

        self.pcbdron.polyhedron = polyhedron;
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
            &context,
            &CpuMaterial {
                albedo: Srgba::WHITE,
                ..Default::default()
            },
        );
        // so we have a per-ngon variant map, and that's nice but have to translate to per-face
        // and not too sure how to do that?
        let mut vmap = vec![0usize; polyhedron.faces.len()];
        for (ngon, vars) in &variant_map.0 {
            for (var, idx) in vars.iter().zip(polyhedron.iter_ngon(*ngon)) {
                vmap[idx] = *var;
            }
        }
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
        };
        for (n_gon, pcb_vars) in pcbs.iter().enumerate() {
            for (variant, pcb) in pcb_vars.iter().enumerate().filter_map(|(i, p)| {
                if let Some(pp) = p {
                    Some((i, pp))
                } else {
                    None
                }
            }) {
                res.add_pcb(&context, PcbId { n_gon, variant }, pcb)?;
            }
        }
        Ok(res)
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
        //
        let instances = Instances {
            transformations: self
                .pcbdron
                .iter_variant(pcb_id)
                .map(|idx| self.pcbdron.polyhedron.face_transforms[idx])
                .collect(),
            ..Default::default()
        };
        let instanced_model = InstancedModel::new(&context, &instances, &model)
            .or_raise(|| MultiPcbdronError(format!("could not add {pcb_id:?} to multihedron")))?;
        self.pcb_models.push(instanced_model);
        self.instance_map.push(pcb_id);
        self.instances.push(Instances::default());
        Ok(())
    }

    fn update_instances(&mut self) -> exn::Result<(), MultiPcbdronError> {
        // clear-and-rebuild for now, would be better to remove-insert later
        // first build own instances, then upload to GPU by changing pcb_models
        for (i, pcb_model) in self.pcb_models.iter_mut().enumerate() {
            // a pcb_model is a single pcb, but contains more than one mesh for different parts
            let pcb_id = self.instance_map[i];
            self.instances[i].transformations = self
                .pcbdron
                .iter_variant(pcb_id)
                .map(|idx| self.pcbdron.polyhedron.face_transforms[idx])
                .collect();
            self.instances[i].colors = None;
            pcb_model
                .iter_mut()
                .for_each(|pm| pm.geometry.set_instances(&self.instances[i]));
        }
        Ok(())
    }
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

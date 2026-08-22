use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A [`PcbDesign`] is a minimal description from which a Pcbdron can be built deterministically
///
/// As such it serves as the interface I guess?
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct PcbDesign {
    pub polyhedron: String,
    pub variant_map: BTreeMap<usize, Vec<usize>>,
    pub path: Vec<usize>,
}

pub enum LampDesign {
    SinglePoly(PcbDesign),
}

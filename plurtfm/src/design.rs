use serde::{Deserialize, Serialize};
use tsify::{Tsify, declare};

#[declare]
pub type VariantMap = Vec<(usize, Vec<usize>)>;

#[derive(Tsify, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct PcbPath {
    pub start_ngon: usize,
    pub start_nth: usize,
    /// A turn unambiguously means "number of solder jumpers to close"
    ///
    /// So for first time visit, it means `clockwise from enter + 1`
    /// for next time visit it means `counterclockwise from enter` (no +-1)
    pub turns: Vec<usize>,
}

impl Default for PcbPath {
    fn default() -> Self {
        PcbPath {
            start_ngon: 3,
            start_nth: 0,
            turns: Vec::new(),
        }
    }
}

/// A [`PcbDesign`] is a minimal description from which a Pcbdron can be built deterministically
///
/// As such it serves as the interface I guess?
#[derive(Tsify, Serialize, Deserialize, Default, Debug)]
pub struct PcbDesign {
    pub polyhedron: String,
    /// The variant map
    /// In js/ts this should actually be a
    /// `[number,number[]][];`
    /// since dicts don't do keys
    pub variant_map: VariantMap,
    pub path: Option<PcbPath>,
}

/// A lamp could theoretically have multiple like different types of designs,
///
/// For now, there's only a single version in the enum, but that's ok
#[derive(Tsify, Serialize, Deserialize, Debug)]
pub enum LampDesign {
    SinglePoly(PcbDesign),
}

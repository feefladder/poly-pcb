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

#[derive(Tsify, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct PcbPaths(Vec<PcbPath>);
impl From<Vec<PcbPath>> for PcbPaths {
    fn from(value: Vec<PcbPath>) -> Self {
        Self(value)
    }
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

/// A [`PcBorsign`] is a minimal description from which a [`Pcboron`] can be built deterministically
///
/// As such it serves as the interface I guess?
#[derive(Tsify, Serialize, Deserialize, Default, Debug)]
pub struct PcBorsign {
    /// The polyhedra that this pcboron contains
    pub polyhedra: Vec<String>,
    /// The variant map
    /// In js/ts this should actually be a
    /// `[number,number[]][];`
    /// since dicts don't do keys
    pub variant_map: VariantMap,

    pub path: Vec<PcbPath>,
}

impl PcBorsign {
    pub fn add(
        mut self,
        PcbDrosign {
            polyhedron,
            variant_map,
            path,
        }: PcbDrosign,
    ) -> Self {
        self.polyhedra.push(polyhedron);
        for (ngon, vars) in variant_map {
            if let Some((_, v)) = self.variant_map.iter_mut().find(|(ng, _)| *ng == ngon) {
                v.extend_from_slice(&vars);
            } else {
                self.variant_map.push((ngon, vars));
            }
        }
        if let Some(p) = path {
            self.path.push(p);
        }
        self
    }
}

/// A design of a Pcbdron
///
/// This is what a Pcbdron emits, and is collected into a PcBorsign.
pub struct PcbDrosign {
    pub polyhedron: String,
    pub variant_map: VariantMap,
    /// The (optional) PcbPath
    ///
    /// Since in js it has a nth_ngon start face,
    /// it's an option, whereas in rust it can just be an empty vec
    pub path: Option<PcbPath>,
}

impl From<PcbDrosign> for PcBorsign {
    fn from(value: PcbDrosign) -> Self {
        let path = value.path.map(|v| vec![v]).unwrap_or_default();
        Self {
            polyhedra: vec![value.polyhedron],
            variant_map: value.variant_map,
            path,
        }
    }
}

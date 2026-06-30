//! Noise + value generators: Perlin/Simplex/Worley/Ridged FBM, Constant,
//! Voronoi, Gradient. Source nodes (no required input).

pub mod shared;

pub mod constant;
pub mod gradient;
pub mod perlin;
pub mod ridged;
pub mod simplex;
pub mod voronoi;
pub mod worley;

use crate::nodes::def::NodeDef;

/// This family's descriptors -- the family index + exhaustiveness checkpoint.
pub static NODES: &[&NodeDef] = &[
    &perlin::DEF,
    &simplex::DEF,
    &worley::DEF,
    &ridged::DEF,
    &constant::DEF,
    &voronoi::DEF,
    &gradient::DEF,
];

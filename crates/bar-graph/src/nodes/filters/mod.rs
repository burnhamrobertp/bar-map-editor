//! Filters: erosion, smoothing, value remaps, and spatial transforms applied
//! to a single heightmap input.

pub mod shared;

pub mod bias_gain;
pub mod blur;
pub mod clamp;
pub mod curve;
pub mod displacement;
pub mod hydraulic_erosion;
pub mod invert;
pub mod mirror;
pub mod normalize;
pub mod sharpen;
pub mod stratify;
pub mod terrace;
pub mod thermal_erosion;
pub mod transform;
pub mod warp;

use crate::nodes::def::NodeDef;

pub static NODES: &[&'static NodeDef] = &[
    &hydraulic_erosion::DEF,
    &thermal_erosion::DEF,
    &blur::DEF,
    &sharpen::DEF,
    &clamp::DEF,
    &terrace::DEF,
    &invert::DEF,
    &mirror::DEF,
    &curve::DEF,
    &normalize::DEF,
    &bias_gain::DEF,
    &displacement::DEF,
    &transform::DEF,
    &warp::DEF,
    &stratify::DEF,
];

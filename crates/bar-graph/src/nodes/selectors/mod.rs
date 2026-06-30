//! Selectors + mask-analysis nodes: slope/height/flow/convexity/aspect
//! selection, plus the mask threshold / morphology ops. Each filters an input
//! into a mask or analysis field.

pub mod shared;

pub mod flow_select;
pub mod height_select;
pub mod mask;
pub mod mask_expand;
pub mod mask_shrink;
pub mod mask_threshold;
pub mod select_aspect;
pub mod select_convexity;
pub mod slope_map;
pub mod slope_select;

use crate::nodes::def::NodeDef;

pub static NODES: &[&NodeDef] = &[
    &slope_map::DEF,
    &height_select::DEF,
    &slope_select::DEF,
    &flow_select::DEF,
    &select_convexity::DEF,
    &select_aspect::DEF,
    &mask_threshold::DEF,
    &mask::DEF,
    &mask_expand::DEF,
    &mask_shrink::DEF,
];

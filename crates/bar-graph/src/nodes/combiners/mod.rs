//! Two-input combiners: the universal Combine (Blend) + arithmetic ops, plus
//! the mask-driven selectors.

pub mod shared;

pub mod add;
pub mod blend;
pub mod mask_apply;
pub mod mask_select;
pub mod max;
pub mod min;
pub mod multiply;
pub mod subtract;

use crate::nodes::def::NodeDef;

pub static NODES: &[&NodeDef] = &[
    &blend::DEF,
    &add::DEF,
    &subtract::DEF,
    &multiply::DEF,
    &max::DEF,
    &min::DEF,
    &mask_select::DEF,
    &mask_apply::DEF,
];

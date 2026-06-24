//! Scalar-parameter-graph nodes: scalar values wired INTO node params.
//!
//! These produce a single `PortKind::Scalar` number rather than a spatial
//! field. Wire a scalar output into any param-named Scalar input (auto-appended
//! for each `scalar_bindable` key) to drive that param at eval time.

pub mod int_value;
pub mod scalar_math;
pub mod scalar_value;

use crate::nodes::def::NodeDef;

pub static NODES: &[&'static NodeDef] = &[
    &scalar_value::DEF,
    &scalar_math::DEF,
    &int_value::DEF,
];

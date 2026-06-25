//! Per-pixel expression evaluation: the Equation node.

pub mod equation;

use crate::nodes::def::NodeDef;

pub static NODES: &[&NodeDef] = &[&equation::DEF];

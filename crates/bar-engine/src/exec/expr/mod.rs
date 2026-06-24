//! Executor for the per-pixel Equation node.

use std::collections::HashMap;

use bar_graph::NodeType;

use super::ExecFn;

pub mod equation;

pub fn register(m: &mut HashMap<NodeType, ExecFn>) {
    m.insert(NodeType::Equation, equation::exec);
}

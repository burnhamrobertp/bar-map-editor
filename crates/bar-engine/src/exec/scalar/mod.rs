//! Executors for the scalar-parameter-graph nodes.

use std::collections::HashMap;

use bar_graph::NodeType;

use super::ExecFn;

pub mod int_value;
pub mod scalar_math;
pub mod scalar_value;

pub fn register(m: &mut HashMap<NodeType, ExecFn>) {
    m.insert(NodeType::ScalarValue, scalar_value::exec);
    m.insert(NodeType::ScalarMath, scalar_math::exec);
    m.insert(NodeType::IntValue, int_value::exec);
}

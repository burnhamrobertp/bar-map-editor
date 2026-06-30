//! Executor for the terminal node (FinalComposition).

use std::collections::HashMap;

use bar_graph::NodeType;

use super::ExecFn;

pub mod final_composition;

pub fn register(m: &mut HashMap<NodeType, ExecFn>) {
    m.insert(NodeType::FinalComposition, final_composition::exec);
}

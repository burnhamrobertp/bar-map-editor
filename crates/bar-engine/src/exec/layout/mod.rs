//! Executor for the Layout node.

use std::collections::HashMap;

use bar_graph::NodeType;

use super::ExecFn;

pub mod layout;
pub mod raster;

pub fn register(m: &mut HashMap<NodeType, ExecFn>) {
    m.insert(NodeType::Layout, layout::exec);
}

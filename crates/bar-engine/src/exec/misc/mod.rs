//! Executors for the misc nodes: Checkpoint, Switch, CoastErosion.

use std::collections::HashMap;

use bar_graph::NodeType;

use super::ExecFn;

pub mod checkpoint;
pub mod coast_erosion;
pub mod switch;

pub fn register(m: &mut HashMap<NodeType, ExecFn>) {
    m.insert(NodeType::Checkpoint, checkpoint::exec);
    m.insert(NodeType::Switch, switch::exec);
    m.insert(NodeType::CoastErosion, coast_erosion::exec);
}

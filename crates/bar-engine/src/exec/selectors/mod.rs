//! Executors for the selector / mask-analysis nodes.

use std::collections::HashMap;

use bar_graph::NodeType;

use super::ExecFn;

pub mod flow_select;
pub mod height_select;
pub mod mask;
pub mod mask_expand;
pub mod mask_shrink;
pub mod mask_threshold;
pub mod select_aspect;
pub mod select_convexity;
pub mod shared;
pub mod slope_map;
pub mod slope_select;

pub fn register(m: &mut HashMap<NodeType, ExecFn>) {
    m.insert(NodeType::SlopeMap, slope_map::exec);
    m.insert(NodeType::HeightSelect, height_select::exec);
    m.insert(NodeType::SlopeSelect, slope_select::exec);
    m.insert(NodeType::FlowSelect, flow_select::exec);
    m.insert(NodeType::SelectConvexity, select_convexity::exec);
    m.insert(NodeType::SelectAspect, select_aspect::exec);
    m.insert(NodeType::MaskThreshold, mask_threshold::exec);
    m.insert(NodeType::Mask, mask::exec);
    m.insert(NodeType::MaskExpand, mask_expand::exec);
    m.insert(NodeType::MaskShrink, mask_shrink::exec);
}

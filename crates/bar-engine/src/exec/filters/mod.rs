//! Executors for the filter nodes.

use std::collections::HashMap;

use bar_graph::NodeType;

use super::ExecFn;

pub mod bias_gain;
pub mod blur;
pub mod clamp;
pub mod curve;
pub mod displacement;
pub mod hydraulic_erosion;
pub mod invert;
pub mod mirror;
pub mod normalize;
pub mod shared;
pub mod sharpen;
pub mod stratify;
pub mod terrace;
pub mod thermal_erosion;
pub mod transform;
pub mod warp;

pub fn register(m: &mut HashMap<NodeType, ExecFn>) {
    m.insert(NodeType::HydraulicErosion, hydraulic_erosion::exec);
    m.insert(NodeType::ThermalErosion, thermal_erosion::exec);
    m.insert(NodeType::Blur, blur::exec);
    m.insert(NodeType::Sharpen, sharpen::exec);
    m.insert(NodeType::Clamp, clamp::exec);
    m.insert(NodeType::Terrace, terrace::exec);
    m.insert(NodeType::Invert, invert::exec);
    m.insert(NodeType::Mirror, mirror::exec);
    m.insert(NodeType::Curve, curve::exec);
    m.insert(NodeType::Normalize, normalize::exec);
    m.insert(NodeType::BiasGain, bias_gain::exec);
    m.insert(NodeType::Displacement, displacement::exec);
    m.insert(NodeType::Transform, transform::exec);
    m.insert(NodeType::Warp, warp::exec);
    m.insert(NodeType::Stratify, stratify::exec);
}

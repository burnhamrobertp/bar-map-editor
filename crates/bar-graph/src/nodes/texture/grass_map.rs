use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[
    PortDef::one("input", "Heightmap", PortKind::Heightmap),
    PortDef::one("slope", "Slope Map", PortKind::Heightmap),
    PortDef::one("control", "Control", PortKind::Control),
    PortDef::one("density", "Density", PortKind::Density),
    PortDef::one("mask", "Mask", PortKind::Mask),
];
static OUTPUTS: &[PortDef] = &[PortDef::one("output", "Grass Density", PortKind::Heightmap)];

static PARAMS: &[ParamDef] = &[
    ParamDef { key: "min_height", default: || ParamValue::Float(0.15), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "max_height", default: || ParamValue::Float(0.7), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "max_slope", default: || ParamValue::Float(0.4), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "density", default: || ParamValue::Float(1.0), ui: ParamUi::FloatRange { min: 0.0, max: 2.0 } },
    ParamDef { key: "falloff", default: || ParamValue::Float(0.05), ui: ParamUi::FloatRange { min: 0.0, max: 0.5 } },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::GrassMap,
    "Grass Map",
    NodeCategory::SplatMap,
    INPUTS,
    OUTPUTS,
    PARAMS,
    NodeCaps::NONE,
);

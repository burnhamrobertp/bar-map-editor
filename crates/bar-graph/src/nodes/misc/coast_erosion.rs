use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[
    PortDef::one("input", "Input", PortKind::Heightmap),
    PortDef::one("control", "Control", PortKind::Control),
    PortDef::one("mask", "Mask", PortKind::Mask),
];
static OUTPUTS: &[PortDef] = &[PortDef::one("output", "Output", PortKind::Heightmap)];

static PARAMS: &[ParamDef] = &[
    ParamDef { key: "sea_level", default: || ParamValue::Float(0.3), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "beach_size", default: || ParamValue::Float(0.05), ui: ParamUi::FloatRange { min: 0.0, max: 0.5 } },
    ParamDef { key: "inland_height_influence", default: || ParamValue::Float(0.3), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "underwater_smoothing", default: || ParamValue::UInt(3), ui: ParamUi::UIntRange { min: 0, max: 20 } },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::CoastErosion,
    "Coast Erosion",
    NodeCategory::Filter,
    INPUTS,
    OUTPUTS,
    PARAMS,
    NodeCaps::NONE,
);

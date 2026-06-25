use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[
    PortDef::one("input", "Input", PortKind::Heightmap),
    PortDef::one("control", "Control", PortKind::Control),
];
static OUTPUT: &[PortDef] = &[PortDef::one("output", "Mask", PortKind::Heightmap)];

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "threshold",
        default: || ParamValue::Float(0.5),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "smoothness",
        default: || ParamValue::Float(0.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::MaskThreshold,
    "Mask Threshold",
    NodeCategory::Mask,
    INPUTS,
    OUTPUT,
    PARAMS,
    NodeCaps::NONE,
);

use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[
    PortDef::one("input", "Input", PortKind::Heightmap),
    PortDef::one("displacement", "Displacement", PortKind::Heightmap),
    PortDef::one("control", "Control", PortKind::Control),
    PortDef::one("mask", "Mask", PortKind::Mask),
];
static PARAMS: &[ParamDef] = &[
    ParamDef { key: "strength", default: || ParamValue::Float(0.1), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::Displacement,
    "Displacement",
    NodeCategory::Filter,
    INPUTS,
    super::shared::OUTPUT,
    PARAMS,
    NodeCaps::NONE,
);

use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[
    PortDef::one("input", "Input", PortKind::Heightmap),
    PortDef::one("warp_x", "Warp X", PortKind::Heightmap),
    PortDef::one("warp_y", "Warp Y", PortKind::Heightmap),
];
static PARAMS: &[ParamDef] = &[ParamDef {
    key: "strength",
    default: || ParamValue::Float(0.1),
    ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
}];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::Warp,
    "Warp",
    NodeCategory::Filter,
    INPUTS,
    super::shared::OUTPUT,
    PARAMS,
    NodeCaps::NONE,
);

use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static OUTPUTS: &[PortDef] = &[PortDef::one("output", "Normal Map", PortKind::Color)];

static PARAMS: &[ParamDef] = &[ParamDef {
    key: "strength",
    default: || ParamValue::Float(1.0),
    ui: ParamUi::FloatRange { min: 0.0, max: 4.0 },
}];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::NormalMap,
    "Normal Map",
    NodeCategory::SplatMap,
    super::shared::INPUT_MASK_IN,
    OUTPUTS,
    PARAMS,
    NodeCaps::NONE,
);

use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static OUTPUT: &[PortDef] = &[PortDef::one("output", "Scalar", PortKind::Scalar)];
static PARAMS: &[ParamDef] = &[ParamDef {
    key: "value",
    default: || ParamValue::UInt(1),
    ui: ParamUi::UIntFree,
}];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::IntValue,
    "Int Value",
    NodeCategory::Generator,
    &[],
    OUTPUT,
    PARAMS,
    NodeCaps::source(),
);

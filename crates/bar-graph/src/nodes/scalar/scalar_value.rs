use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static OUTPUT: &[PortDef] = &[PortDef::one("output", "Scalar", PortKind::Scalar)];
static PARAMS: &[ParamDef] = &[ParamDef {
    key: "value",
    default: || ParamValue::Float(0.5),
    ui: ParamUi::FloatFree,
}];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::ScalarValue,
    "Scalar Value",
    NodeCategory::Generator,
    &[],
    OUTPUT,
    PARAMS,
    NodeCaps::source(),
);

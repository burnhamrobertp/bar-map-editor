use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static OUTPUT: &[PortDef] = &[PortDef::one("output", "Value", PortKind::Heightmap)];
static PARAMS: &[ParamDef] = &[ParamDef {
    key: "value",
    default: || ParamValue::Float(0.5),
    ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
}];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::Constant,
    "Constant",
    NodeCategory::Generator,
    &[],
    OUTPUT,
    PARAMS,
    NodeCaps::source(),
);

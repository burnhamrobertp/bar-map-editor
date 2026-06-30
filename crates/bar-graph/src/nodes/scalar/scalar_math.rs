use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

const OPS: &[&str] = &[
    "add", "subtract", "multiply", "divide", "min", "max", "average", "power",
];

static INPUTS: &[PortDef] = &[
    PortDef::one("a", "A", PortKind::Scalar),
    PortDef::one("b", "B", PortKind::Scalar),
];
static OUTPUT: &[PortDef] = &[PortDef::one("output", "Scalar", PortKind::Scalar)];
static PARAMS: &[ParamDef] = &[ParamDef {
    key: "op",
    default: || ParamValue::String("add".to_string()),
    ui: ParamUi::Choices(OPS),
}];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::ScalarMath,
    "Scalar Math",
    NodeCategory::Combiner,
    INPUTS,
    OUTPUT,
    PARAMS,
    NodeCaps::NONE,
);

use crate::node::NodeType;
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[
    PortDef::one("input", "Input", PortKind::Heightmap),
    PortDef::one("control", "Control", PortKind::Control),
];
static OUTPUT: &[PortDef] = &[PortDef::one("mask", "Mask", PortKind::Mask)];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::Mask,
    "Mask",
    NodeCategory::Mask,
    INPUTS,
    OUTPUT,
    &[],
    NodeCaps::NONE,
);

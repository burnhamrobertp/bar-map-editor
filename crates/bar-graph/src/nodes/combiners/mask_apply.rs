use crate::node::NodeType;
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[
    PortDef::one("input", "Input", PortKind::Heightmap),
    PortDef::one("background", "Background", PortKind::Heightmap),
    PortDef::one("mask", "Mask", PortKind::Mask),
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::MaskApply,
    "Mask Apply",
    NodeCategory::Mask,
    INPUTS,
    super::shared::OUTPUT,
    &[],
    NodeCaps::NONE,
);

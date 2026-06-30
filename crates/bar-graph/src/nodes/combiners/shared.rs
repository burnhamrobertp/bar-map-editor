//! Shared bits for the two-input combiner nodes.

use crate::node::NodeType;
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, PortDef};
use crate::port::PortKind;

/// Two heightmap inputs + a mask (the arithmetic combiners' port set).
pub static AB_MASK_IN: &[PortDef] = &[
    PortDef::one("a", "Input A", PortKind::Heightmap),
    PortDef::one("b", "Input B", PortKind::Heightmap),
    PortDef::one("mask", "Mask", PortKind::Mask),
];
pub static OUTPUT: &[PortDef] = &[PortDef::one("output", "Output", PortKind::Heightmap)];

/// A paramless binary combiner (Add/Subtract/Multiply/Max/Min/MaskSelect).
pub const fn binop_def(node_type: NodeType, label: &'static str) -> NodeDef {
    NodeDef::basic(
        node_type,
        label,
        NodeCategory::Combiner,
        AB_MASK_IN,
        OUTPUT,
        &[],
        NodeCaps::NONE,
    )
}

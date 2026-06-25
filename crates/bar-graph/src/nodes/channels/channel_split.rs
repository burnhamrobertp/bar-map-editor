use crate::node::NodeType;
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[PortDef::one("color", "Color", PortKind::Color)];
static OUTPUTS: &[PortDef] = &[
    PortDef::one("r", "Red", PortKind::Heightmap),
    PortDef::one("g", "Green", PortKind::Heightmap),
    PortDef::one("b", "Blue", PortKind::Heightmap),
    PortDef::one("a", "Alpha", PortKind::Heightmap),
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::ChannelSplit,
    "Channel Split",
    NodeCategory::Colorizer,
    INPUTS,
    OUTPUTS,
    &[],
    NodeCaps::NONE,
);

use crate::node::NodeType;
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, PortDef};
use crate::port::PortKind;

static OUTPUT: &[PortDef] = &[PortDef::one("output", "Heightmap", PortKind::Heightmap)];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::FileInput,
    "File Input",
    NodeCategory::Source,
    &[],
    OUTPUT,
    &[],
    NodeCaps { is_source: true, ..NodeCaps::NONE },
);

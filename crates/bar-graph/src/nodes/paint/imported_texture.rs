use crate::node::NodeType;
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, PortDef};
use crate::port::PortKind;

static OUTPUT: &[PortDef] = &[PortDef::one("output", "Texture", PortKind::Color)];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::ImportedTexture,
    "Imported Texture",
    NodeCategory::Source,
    &[],
    OUTPUT,
    &[],
    NodeCaps { is_source: true, holds_assets: true, ..NodeCaps::NONE },
);

use crate::node::NodeType;
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, PortDef};
use crate::port::PortKind;

static OUTPUT: &[PortDef] = &[PortDef::one("output", "Slope", PortKind::Heightmap)];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::SlopeMap,
    "Slope Map",
    NodeCategory::SplatMap,
    super::shared::INPUT_CONTROL_IN,
    OUTPUT,
    &[],
    NodeCaps::NONE,
);

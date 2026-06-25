use crate::node::NodeType;
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[PortDef::one("input", "Input", PortKind::Heightmap)];
static OUTPUTS: &[PortDef] = &[PortDef::one("output", "Output", PortKind::Heightmap)];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::Checkpoint,
    "Checkpoint",
    NodeCategory::Filter,
    INPUTS,
    OUTPUTS,
    &[],
    NodeCaps::NONE,
);

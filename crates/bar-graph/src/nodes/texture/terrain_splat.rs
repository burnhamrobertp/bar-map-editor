use crate::node::NodeType;
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[
    PortDef::one("slope", "Slope Map", PortKind::Heightmap),
    PortDef::one("band0", "Band 0", PortKind::Heightmap),
    PortDef::one("band1", "Band 1", PortKind::Heightmap),
    PortDef::one("band2", "Band 2", PortKind::Heightmap),
    PortDef::one("control", "Control", PortKind::Control),
    PortDef::one("mask", "Mask", PortKind::Mask),
];
static OUTPUTS: &[PortDef] = &[PortDef::one("output", "Splat", PortKind::Heightmap)];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::TerrainSplat,
    "Terrain Splat",
    NodeCategory::SplatMap,
    INPUTS,
    OUTPUTS,
    &[],
    NodeCaps::NONE,
);

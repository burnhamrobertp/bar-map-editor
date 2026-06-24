use crate::node::NodeType;
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef};

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::Normalize,
    "Normalize",
    NodeCategory::Filter,
    super::shared::INPUT_MASK_IN,
    super::shared::OUTPUT,
    &[],
    NodeCaps::NONE,
);

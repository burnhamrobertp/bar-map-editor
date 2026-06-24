use crate::node::NodeType;
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef};

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::Curve,
    "Curve",
    NodeCategory::Filter,
    super::shared::INPUT_CONTROL_MASK_IN,
    super::shared::OUTPUT,
    &[],
    NodeCaps::NONE,
);

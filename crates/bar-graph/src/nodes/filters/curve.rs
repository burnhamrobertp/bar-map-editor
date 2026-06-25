use crate::node::NodeType;
use crate::nodes::def::{CustomPanel, NodeCaps, NodeCategory, NodeDef};

pub static DEF: NodeDef = NodeDef {
    custom_panel: Some(CustomPanel::Curve),
    ..NodeDef::basic(
        NodeType::Curve,
        "Curve",
        NodeCategory::Filter,
        super::shared::INPUT_CONTROL_MASK_IN,
        super::shared::OUTPUT,
        &[],
        NodeCaps::NONE,
    )
};

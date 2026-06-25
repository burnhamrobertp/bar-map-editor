use crate::node::NodeType;
use crate::nodes::def::{CustomPanel, NodeCaps, NodeCategory, NodeDef, PortDef};
use crate::port::PortKind;

static OUTPUT: &[PortDef] = &[PortDef::one("files", "Files", PortKind::FileList)];

pub static DEF: NodeDef = NodeDef {
    node_type: NodeType::PassThrough,
    label: "Pass Through",
    category: NodeCategory::Source,
    inputs: &[],
    outputs: OUTPUT,
    params: &[],
    caps: NodeCaps { is_source: true, holds_assets: true, ..NodeCaps::NONE },
    dynamic_params: None,
    dynamic_param_ui: None,
    param_side_effects: None,
    post_build: None,
    scalar_bindable: &[],
    custom_panel: Some(CustomPanel::PassThrough),
};

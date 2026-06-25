use crate::node::{Node, NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static VALUE_IN: &[PortDef] = &[PortDef::one("value", "Value", PortKind::Heightmap)];
static VALUE_OUT: &[PortDef] = &[PortDef::one("value", "Value", PortKind::Heightmap)];

static KIND_CHOICES: &[&str] = &["Heightmap", "Color", "Mask", "Scalar", "File", "FileList"];

static PARAMS: &[ParamDef] = &[
    ParamDef { key: "name", default: || ParamValue::String(String::new()), ui: ParamUi::Text },
    ParamDef { key: "kind", default: || ParamValue::String("Heightmap".to_string()), ui: ParamUi::Choices(KIND_CHOICES) },
];

fn sync_kind(node: &mut Node) {
    node.sync_subgraph_io_kind();
}

pub static DEF: NodeDef = NodeDef {
    node_type: NodeType::SubgraphOutput,
    label: "Subgraph Output",
    category: NodeCategory::Io,
    inputs: VALUE_IN,
    outputs: VALUE_OUT,
    params: PARAMS,
    caps: NodeCaps { is_subgraph_only: true, ..NodeCaps::NONE },
    dynamic_params: None,
    dynamic_param_ui: None,
    param_side_effects: None,
    post_build: Some(sync_kind),
    scalar_bindable: &[],
    custom_panel: None,
};

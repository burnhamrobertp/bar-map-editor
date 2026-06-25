use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static OUTPUT: &[PortDef] = &[PortDef::one("file", "File", PortKind::File)];

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "path",
        default: || ParamValue::String(String::new()),
        ui: ParamUi::Text,
    },
    ParamDef {
        key: "bundle_path",
        default: || ParamValue::String(String::new()),
        ui: ParamUi::Text,
    },
];

pub static DEF: NodeDef = NodeDef {
    node_type: NodeType::FileReference,
    label: "File Reference",
    category: NodeCategory::Source,
    inputs: &[],
    outputs: OUTPUT,
    params: PARAMS,
    caps: NodeCaps {
        is_source: true,
        holds_assets: true,
        ..NodeCaps::NONE
    },
    dynamic_params: None,
    dynamic_param_ui: None,
    param_side_effects: None,
    post_build: None,
    scalar_bindable: &[],
    custom_panel: None,
};

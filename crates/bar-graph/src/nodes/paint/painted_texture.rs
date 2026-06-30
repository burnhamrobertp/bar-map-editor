use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{CustomPanel, NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static OUTPUT: &[PortDef] = &[PortDef::one("output", "Texture", PortKind::Color)];

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "asset_id",
        default: || ParamValue::String(String::new()),
        ui: ParamUi::Hidden,
    },
    ParamDef {
        key: "asset_path",
        default: || ParamValue::String(String::new()),
        ui: ParamUi::Hidden,
    },
    ParamDef {
        key: "brush_color",
        default: || ParamValue::String("8B7355".to_string()),
        ui: ParamUi::Color,
    },
];

pub static DEF: NodeDef = NodeDef {
    node_type: NodeType::PaintedTexture,
    label: "Painted Texture",
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
    custom_panel: Some(CustomPanel::PaintedTexture),
};

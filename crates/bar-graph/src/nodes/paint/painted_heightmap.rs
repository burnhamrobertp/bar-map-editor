use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{CustomPanel, NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static OUTPUT: &[PortDef] = &[PortDef::one("output", "Heightmap", PortKind::Heightmap)];

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
        key: "resolution",
        default: || ParamValue::UInt(256),
        ui: ParamUi::UIntFree,
    },
    ParamDef {
        key: "sampling",
        default: || ParamValue::String("smooth".to_string()),
        ui: ParamUi::Text,
    },
];

pub static DEF: NodeDef = NodeDef {
    node_type: NodeType::PaintedHeightmap,
    label: "Painted Heightmap",
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
    custom_panel: Some(CustomPanel::PaintedHeightmap),
};

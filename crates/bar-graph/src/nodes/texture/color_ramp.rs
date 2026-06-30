use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{CustomPanel, NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static OUTPUTS: &[PortDef] = &[PortDef::one("output", "Color", PortKind::Color)];

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "stop_count",
        default: || ParamValue::UInt(2),
        ui: ParamUi::UIntRange { min: 2, max: 8 },
    },
    ParamDef {
        key: "pos_0",
        default: || ParamValue::Float(0.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "color_0",
        default: || ParamValue::String("000000".to_string()),
        ui: ParamUi::Color,
    },
    ParamDef {
        key: "pos_1",
        default: || ParamValue::Float(1.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "color_1",
        default: || ParamValue::String("FFFFFF".to_string()),
        ui: ParamUi::Color,
    },
    ParamDef {
        key: "pos_2",
        default: || ParamValue::Float(0.25),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "color_2",
        default: || ParamValue::String("404040".to_string()),
        ui: ParamUi::Color,
    },
    ParamDef {
        key: "pos_3",
        default: || ParamValue::Float(0.375),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "color_3",
        default: || ParamValue::String("606060".to_string()),
        ui: ParamUi::Color,
    },
    ParamDef {
        key: "pos_4",
        default: || ParamValue::Float(0.5),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "color_4",
        default: || ParamValue::String("808080".to_string()),
        ui: ParamUi::Color,
    },
    ParamDef {
        key: "pos_5",
        default: || ParamValue::Float(0.625),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "color_5",
        default: || ParamValue::String("A0A0A0".to_string()),
        ui: ParamUi::Color,
    },
    ParamDef {
        key: "pos_6",
        default: || ParamValue::Float(0.75),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "color_6",
        default: || ParamValue::String("C0C0C0".to_string()),
        ui: ParamUi::Color,
    },
    ParamDef {
        key: "pos_7",
        default: || ParamValue::Float(0.875),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "color_7",
        default: || ParamValue::String("E0E0E0".to_string()),
        ui: ParamUi::Color,
    },
];

pub static DEF: NodeDef = NodeDef {
    node_type: NodeType::ColorRamp,
    label: "Color Ramp",
    category: NodeCategory::Colorizer,
    inputs: super::shared::INPUT_MASK_IN,
    outputs: OUTPUTS,
    params: PARAMS,
    caps: NodeCaps::NONE,
    dynamic_params: None,
    dynamic_param_ui: None,
    param_side_effects: None,
    post_build: None,
    scalar_bindable: &[],
    custom_panel: Some(CustomPanel::ColorRamp),
};

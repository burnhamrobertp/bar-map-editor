use crate::node::{Node, NodeType, ParamValue};
use crate::nodes::def::{CustomPanel, NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

// Default 2 texture inputs; `post_build` / `resize_texture_weightmap_ports`
// adjusts the count to `layer_count` after construction.
static INPUTS: &[PortDef] = &[
    PortDef::one("texture_0", "Texture 0", PortKind::Color),
    PortDef::one("texture_1", "Texture 1", PortKind::Color),
];

static PRIORITY_TYPES: &[&str] = &["weighted_blend", "priority"];

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "layer_count",
        default: || ParamValue::UInt(2),
        ui: ParamUi::UIntRange { min: 2, max: 8 },
    },
    ParamDef {
        key: "priority_type",
        default: || ParamValue::String("weighted_blend".to_string()),
        ui: ParamUi::Choices(PRIORITY_TYPES),
    },
    // Slot 0 = highest default priority (7), slot 7 = lowest (0).
    ParamDef {
        key: "priority_0",
        default: || ParamValue::Float(7.0),
        ui: ParamUi::FloatRange {
            min: 0.0,
            max: 16.0,
        },
    },
    ParamDef {
        key: "exclusion_0",
        default: || ParamValue::Float(0.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "priority_1",
        default: || ParamValue::Float(6.0),
        ui: ParamUi::FloatRange {
            min: 0.0,
            max: 16.0,
        },
    },
    ParamDef {
        key: "exclusion_1",
        default: || ParamValue::Float(0.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "priority_2",
        default: || ParamValue::Float(5.0),
        ui: ParamUi::FloatRange {
            min: 0.0,
            max: 16.0,
        },
    },
    ParamDef {
        key: "exclusion_2",
        default: || ParamValue::Float(0.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "priority_3",
        default: || ParamValue::Float(4.0),
        ui: ParamUi::FloatRange {
            min: 0.0,
            max: 16.0,
        },
    },
    ParamDef {
        key: "exclusion_3",
        default: || ParamValue::Float(0.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "priority_4",
        default: || ParamValue::Float(3.0),
        ui: ParamUi::FloatRange {
            min: 0.0,
            max: 16.0,
        },
    },
    ParamDef {
        key: "exclusion_4",
        default: || ParamValue::Float(0.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "priority_5",
        default: || ParamValue::Float(2.0),
        ui: ParamUi::FloatRange {
            min: 0.0,
            max: 16.0,
        },
    },
    ParamDef {
        key: "exclusion_5",
        default: || ParamValue::Float(0.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "priority_6",
        default: || ParamValue::Float(1.0),
        ui: ParamUi::FloatRange {
            min: 0.0,
            max: 16.0,
        },
    },
    ParamDef {
        key: "exclusion_6",
        default: || ParamValue::Float(0.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "priority_7",
        default: || ParamValue::Float(0.0),
        ui: ParamUi::FloatRange {
            min: 0.0,
            max: 16.0,
        },
    },
    ParamDef {
        key: "exclusion_7",
        default: || ParamValue::Float(0.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
];

pub static DEF: NodeDef = NodeDef {
    node_type: NodeType::TextureWeightmap,
    label: "Texture Weightmap",
    category: NodeCategory::Colorizer,
    inputs: INPUTS,
    outputs: super::shared::TEXTURE_OUT,
    params: PARAMS,
    caps: NodeCaps::NONE,
    dynamic_params: None,
    dynamic_param_ui: None,
    param_side_effects: None,
    post_build: Some(resize_ports),
    scalar_bindable: &[],
    custom_panel: Some(CustomPanel::TextureWeightmap),
};

fn resize_ports(node: &mut Node) {
    let n = match node.params.get("layer_count") {
        Some(ParamValue::UInt(c)) => *c,
        _ => 2,
    };
    node.resize_texture_weightmap_ports(n);
}

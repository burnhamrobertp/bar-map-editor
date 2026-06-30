use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};

use super::shared;

static RIDGED_PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "character",
        default: || ParamValue::String("ridges".to_string()),
        ui: ParamUi::Choices(shared::RIDGED_CHARACTERS),
    },
    ParamDef {
        key: "frequency",
        default: || ParamValue::Float(2.0),
        ui: ParamUi::FloatRange {
            min: 0.1,
            max: 128.0,
        },
    },
    ParamDef {
        key: "octaves",
        default: || ParamValue::UInt(6),
        ui: ParamUi::UIntRange { min: 1, max: 12 },
    },
    ParamDef {
        key: "lacunarity",
        default: || ParamValue::Float(2.0),
        ui: ParamUi::FloatRange { min: 1.0, max: 4.0 },
    },
    ParamDef {
        key: "persistence",
        default: || ParamValue::Float(0.5),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "seed",
        default: || ParamValue::UInt(0),
        ui: ParamUi::UIntFree,
    },
    ParamDef {
        key: "steepness",
        default: || ParamValue::Float(0.5),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "elevation",
        default: || ParamValue::Float(0.5),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "offset",
        default: || ParamValue::Float(0.0),
        ui: ParamUi::FloatRange {
            min: -0.5,
            max: 0.5,
        },
    },
    ParamDef {
        key: "gain",
        default: || ParamValue::Float(0.5),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
];

pub static DEF: NodeDef = NodeDef {
    node_type: NodeType::RidgedNoise,
    label: "Ridged Noise",
    category: NodeCategory::Generator,
    inputs: shared::CONTROL_IN,
    outputs: shared::HEIGHTMAP_OUT,
    params: RIDGED_PARAMS,
    caps: NodeCaps {
        gpu_eligible: true,
        ..NodeCaps::source()
    },
    dynamic_params: None,
    dynamic_param_ui: None,
    param_side_effects: Some(shared::ridged_character_side_effects),
    post_build: None,
    scalar_bindable: &["frequency", "persistence", "lacunarity"],
    custom_panel: None,
};

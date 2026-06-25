use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "rock_color",
        default: || ParamValue::String("807870".to_string()),
        ui: ParamUi::Color,
    },
    ParamDef {
        key: "soil_color",
        default: || ParamValue::String("8B6914".to_string()),
        ui: ParamUi::Color,
    },
    ParamDef {
        key: "slope_threshold",
        default: || ParamValue::Float(0.4),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "slope_blend",
        default: || ParamValue::Float(0.3),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "ao_strength",
        default: || ParamValue::Float(0.8),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "detail_strength",
        default: || ParamValue::Float(0.25),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::RockSoil,
    "Rock and Soil",
    NodeCategory::Colorizer,
    super::shared::INPUT_SLOPE_MASK_IN,
    super::shared::TEXTURE_OUT,
    PARAMS,
    NodeCaps::NONE,
);

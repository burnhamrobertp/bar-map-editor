use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};

static PARAMS: &[ParamDef] = &[
    ParamDef { key: "vegetation_color", default: || ParamValue::String("4A7020".to_string()), ui: ParamUi::Color },
    ParamDef { key: "dry_color", default: || ParamValue::String("8B7355".to_string()), ui: ParamUi::Color },
    ParamDef { key: "altitude_max", default: || ParamValue::Float(0.6), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "slope_cutoff", default: || ParamValue::Float(0.5), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "slope_blend", default: || ParamValue::Float(0.2), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "ao_strength", default: || ParamValue::Float(0.6), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "detail_strength", default: || ParamValue::Float(0.2), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::Vegetation,
    "Vegetation",
    NodeCategory::Colorizer,
    super::shared::INPUT_SLOPE_MASK_IN,
    super::shared::TEXTURE_OUT,
    PARAMS,
    NodeCaps::NONE,
);

use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};

static PARAMS: &[ParamDef] = &[
    ParamDef { key: "min_slope", default: || ParamValue::Float(0.0), ui: ParamUi::FloatRange { min: 0.0, max: 90.0 } },
    ParamDef { key: "max_slope", default: || ParamValue::Float(30.0), ui: ParamUi::FloatRange { min: 0.0, max: 90.0 } },
    ParamDef { key: "falloff", default: || ParamValue::Float(10.0), ui: ParamUi::FloatRange { min: 0.0, max: 45.0 } },
    ParamDef { key: "falloff_type", default: || ParamValue::String("linear".to_string()), ui: ParamUi::Choices(super::shared::FALLOFF_TYPES) },
    ParamDef { key: "invert", default: || ParamValue::Bool(false), ui: ParamUi::Bool },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::SlopeSelect,
    "Slope Select",
    NodeCategory::SplatMap,
    super::shared::INPUT_CONTROL_IN,
    super::shared::MASK_OUT,
    PARAMS,
    NodeCaps::NONE,
);

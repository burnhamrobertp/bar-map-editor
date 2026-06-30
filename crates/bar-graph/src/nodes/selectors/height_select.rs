use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "low",
        default: || ParamValue::Float(0.3),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "high",
        default: || ParamValue::Float(0.7),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "falloff",
        default: || ParamValue::Float(0.1),
        ui: ParamUi::FloatRange { min: 0.0, max: 0.5 },
    },
    ParamDef {
        key: "falloff_type",
        default: || ParamValue::String("linear".to_string()),
        ui: ParamUi::Choices(super::shared::FALLOFF_TYPES),
    },
    ParamDef {
        key: "invert",
        default: || ParamValue::Bool(false),
        ui: ParamUi::Bool,
    },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::HeightSelect,
    "Height Select",
    NodeCategory::SplatMap,
    super::shared::INPUT_CONTROL_IN,
    super::shared::MASK_OUT,
    PARAMS,
    NodeCaps::NONE,
);

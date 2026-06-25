use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};

static MODES: &[&str] = &["clamp", "normalize", "soft_clip"];
static PARAMS: &[ParamDef] = &[
    ParamDef { key: "mode", default: || ParamValue::String("clamp".to_string()), ui: ParamUi::Choices(MODES) },
    ParamDef { key: "min", default: || ParamValue::Float(0.0), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "max", default: || ParamValue::Float(1.0), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
];

pub static DEF: NodeDef = NodeDef {
    scalar_bindable: &["min", "max"],
    ..NodeDef::basic(
        NodeType::Clamp,
        "Clamp",
        NodeCategory::Filter,
        super::shared::INPUT_CONTROL_MASK_IN,
        super::shared::OUTPUT,
        PARAMS,
        NodeCaps::NONE,
    )
};

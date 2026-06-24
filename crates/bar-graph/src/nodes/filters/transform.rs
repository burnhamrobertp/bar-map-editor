use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};

static PARAMS: &[ParamDef] = &[
    ParamDef { key: "translate_x", default: || ParamValue::Float(0.0), ui: ParamUi::FloatRange { min: -0.5, max: 0.5 } },
    ParamDef { key: "translate_y", default: || ParamValue::Float(0.0), ui: ParamUi::FloatRange { min: -0.5, max: 0.5 } },
    ParamDef { key: "scale", default: || ParamValue::Float(1.0), ui: ParamUi::FloatRange { min: 0.1, max: 4.0 } },
    ParamDef { key: "angle", default: || ParamValue::Float(0.0), ui: ParamUi::FloatRange { min: 0.0, max: 360.0 } },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::Transform,
    "Transform",
    NodeCategory::Filter,
    super::shared::INPUT_MASK_IN,
    super::shared::OUTPUT,
    PARAMS,
    NodeCaps::NONE,
);

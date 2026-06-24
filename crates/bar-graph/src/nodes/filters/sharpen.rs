use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};

static PARAMS: &[ParamDef] = &[
    ParamDef { key: "radius", default: || ParamValue::Float(1.0), ui: ParamUi::FloatRange { min: 0.1, max: 10.0 } },
    ParamDef { key: "strength", default: || ParamValue::Float(1.0), ui: ParamUi::FloatRange { min: 0.0, max: 4.0 } },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::Sharpen,
    "Sharpen",
    NodeCategory::Filter,
    super::shared::INPUT_CONTROL_MASK_IN,
    super::shared::OUTPUT,
    PARAMS,
    NodeCaps::NONE,
);

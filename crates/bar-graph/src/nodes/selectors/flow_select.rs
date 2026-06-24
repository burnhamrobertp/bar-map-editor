use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};

static PARAMS: &[ParamDef] = &[
    ParamDef { key: "threshold", default: || ParamValue::Float(0.2), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "falloff", default: || ParamValue::Float(0.15), ui: ParamUi::FloatRange { min: 0.0, max: 0.5 } },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::FlowSelect,
    "Select Flow",
    NodeCategory::SplatMap,
    super::shared::INPUT_ONLY_IN,
    super::shared::MASK_OUT,
    PARAMS,
    NodeCaps::NONE,
);

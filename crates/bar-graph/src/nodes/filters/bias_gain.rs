use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};

static PARAMS: &[ParamDef] = &[
    ParamDef { key: "bias", default: || ParamValue::Float(0.5), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "gain", default: || ParamValue::Float(0.5), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::BiasGain,
    "Bias / Gain",
    NodeCategory::Filter,
    super::shared::INPUT_CONTROL_MASK_IN,
    super::shared::OUTPUT,
    PARAMS,
    NodeCaps::NONE,
);

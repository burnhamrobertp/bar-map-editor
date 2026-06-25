use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "step_count",
        default: || ParamValue::UInt(4),
        ui: ParamUi::UIntRange { min: 2, max: 80 },
    },
    ParamDef {
        key: "smoothing",
        default: || ParamValue::Float(0.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::Terrace,
    "Terrace",
    NodeCategory::Filter,
    super::shared::INPUT_CONTROL_MASK_IN,
    super::shared::OUTPUT,
    PARAMS,
    NodeCaps::NONE,
);

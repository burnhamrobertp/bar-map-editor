use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "layer_count",
        default: || ParamValue::UInt(8),
        ui: ParamUi::UIntRange { min: 2, max: 32 },
    },
    ParamDef {
        key: "irregularity",
        default: || ParamValue::Float(0.3),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "hardness",
        default: || ParamValue::Float(0.8),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "noise_scale",
        default: || ParamValue::Float(0.05),
        ui: ParamUi::FloatRange {
            min: 0.01,
            max: 0.5,
        },
    },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::Stratify,
    "Stratify",
    NodeCategory::Filter,
    super::shared::INPUT_MASK_IN,
    super::shared::OUTPUT,
    PARAMS,
    NodeCaps::NONE,
);

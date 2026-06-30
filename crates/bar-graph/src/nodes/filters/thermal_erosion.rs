use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "iterations",
        default: || ParamValue::UInt(100),
        ui: ParamUi::UIntRange {
            min: 10,
            max: 1_000,
        },
    },
    ParamDef {
        key: "talus_angle",
        default: || ParamValue::Float(0.6),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::ThermalErosion,
    "Thermal Erosion",
    NodeCategory::Filter,
    super::shared::INPUT_CONTROL_MASK_IN,
    super::shared::OUTPUT,
    PARAMS,
    NodeCaps {
        gpu_eligible: true,
        ..NodeCaps::NONE
    },
);

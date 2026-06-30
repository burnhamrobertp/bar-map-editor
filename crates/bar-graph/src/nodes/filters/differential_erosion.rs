use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};

static PARAMS: &[ParamDef] = &[
    // Overall amount of material worn away.
    ParamDef {
        key: "strength",
        default: || ParamValue::Float(0.5),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    // Number of hard/soft strata across the height range.
    ParamDef {
        key: "strata_layers",
        default: || ParamValue::UInt(6),
        ui: ParamUi::UIntRange { min: 1, max: 16 },
    },
    // 0 = uniform wear (no differential), 1 = only soft rock erodes so hard
    // strata stand out as caps/benches.
    ParamDef {
        key: "strata_contrast",
        default: || ParamValue::Float(0.6),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    // How much steep faces resist (exposed bedrock); preserves cliffs/walls.
    ParamDef {
        key: "slope_hardening",
        default: || ParamValue::Float(0.5),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    // Downcutting passes; more = soft material worn further down to hard layers.
    ParamDef {
        key: "iterations",
        default: || ParamValue::UInt(40),
        ui: ParamUi::UIntRange { min: 1, max: 200 },
    },
    // Optional terracing of the exposed strata (flat shelves), 0 = off.
    ParamDef {
        key: "terrace",
        default: || ParamValue::Float(0.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::DifferentialErosion,
    "Differential Erosion",
    NodeCategory::Filter,
    super::shared::INPUT_CONTROL_MASK_IN,
    super::shared::OUTPUT,
    PARAMS,
    NodeCaps::NONE,
);

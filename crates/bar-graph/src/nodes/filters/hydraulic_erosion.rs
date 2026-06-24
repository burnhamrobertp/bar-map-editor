use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[
    PortDef::one("input", "Input", PortKind::Heightmap),
    PortDef::one("control", "Control", PortKind::Control),
    PortDef::one("mask", "Mask", PortKind::Mask),
    PortDef::one("hardness", "Hardness", PortKind::Heightmap),
];

static METHODS: &[&str] = &["droplet"];
static OUTPUTS: &[PortDef] = &[
    PortDef::one("output", "Output", PortKind::Heightmap),
    PortDef::one("flow", "Flow", PortKind::Heightmap),
    PortDef::one("wear", "Wear", PortKind::Heightmap),
    PortDef::one("deposit", "Deposit", PortKind::Heightmap),
];
static PARAMS: &[ParamDef] = &[
    ParamDef { key: "iterations", default: || ParamValue::UInt(50_000), ui: ParamUi::UIntRange { min: 1_000, max: 500_000 } },
    ParamDef { key: "erosion_rate", default: || ParamValue::Float(0.01), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "deposition_rate", default: || ParamValue::Float(0.01), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "capacity_factor", default: || ParamValue::Float(4.0), ui: ParamUi::FloatRange { min: 0.5, max: 16.0 } },
    ParamDef { key: "inertia", default: || ParamValue::Float(0.05), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "evaporation_rate", default: || ParamValue::Float(0.01), ui: ParamUi::FloatRange { min: 0.0, max: 0.2 } },
    ParamDef { key: "gravity", default: || ParamValue::Float(4.0), ui: ParamUi::FloatRange { min: 1.0, max: 20.0 } },
    ParamDef { key: "erosion_radius", default: || ParamValue::UInt(3), ui: ParamUi::UIntRange { min: 1, max: 16 } },
    ParamDef { key: "max_lifetime", default: || ParamValue::UInt(30), ui: ParamUi::UIntRange { min: 5, max: 200 } },
    ParamDef { key: "river_depth", default: || ParamValue::Float(0.0), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "method", default: || ParamValue::String("droplet".to_string()), ui: ParamUi::Choices(METHODS) },
    ParamDef { key: "seed", default: || ParamValue::UInt(0), ui: ParamUi::UIntFree },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::HydraulicErosion,
    "Hydraulic Erosion",
    NodeCategory::Filter,
    INPUTS,
    OUTPUTS,
    PARAMS,
    NodeCaps { gpu_eligible: true, ..NodeCaps::NONE },
);

use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[PortDef::one("heightmap", "Heightmap", PortKind::Heightmap)];
static OUTPUTS: &[PortDef] = &[PortDef::one("lightmap", "Lightmap", PortKind::Color)];

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "ao_strength",
        default: || ParamValue::Float(1.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "ao_radius",
        default: || ParamValue::Float(0.1),
        ui: ParamUi::FloatRange { min: 0.0, max: 0.5 },
    },
    ParamDef {
        key: "num_directions",
        default: || ParamValue::UInt(16),
        ui: ParamUi::UIntRange { min: 4, max: 32 },
    },
    ParamDef {
        key: "max_steps",
        default: || ParamValue::UInt(24),
        ui: ParamUi::UIntRange { min: 4, max: 64 },
    },
    ParamDef {
        key: "sun_azimuth",
        default: || ParamValue::Float(315.0),
        ui: ParamUi::FloatRange {
            min: 0.0,
            max: 360.0,
        },
    },
    ParamDef {
        key: "sun_elevation",
        default: || ParamValue::Float(45.0),
        ui: ParamUi::FloatRange {
            min: 0.0,
            max: 90.0,
        },
    },
    ParamDef {
        key: "sun_softness",
        default: || ParamValue::Float(0.2),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
];

pub static DEF: NodeDef = NodeDef {
    node_type: NodeType::LightmapBake,
    label: "Lightmap Bake",
    category: NodeCategory::Colorizer,
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    caps: NodeCaps {
        gpu_eligible: true,
        ..NodeCaps::NONE
    },
    dynamic_params: None,
    dynamic_param_ui: None,
    param_side_effects: None,
    post_build: None,
    scalar_bindable: &[],
    custom_panel: None,
};

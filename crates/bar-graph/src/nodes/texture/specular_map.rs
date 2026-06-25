use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static OUTPUTS: &[PortDef] = &[PortDef::one("output", "Specular", PortKind::Heightmap)];

static PARAMS: &[ParamDef] = &[
    ParamDef { key: "rock_specular", default: || ParamValue::Float(0.6), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "flat_specular", default: || ParamValue::Float(0.2), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "water_specular", default: || ParamValue::Float(0.9), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "water_height", default: || ParamValue::Float(0.2), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "snow_specular", default: || ParamValue::Float(0.7), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
    ParamDef { key: "snow_height", default: || ParamValue::Float(0.85), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::SpecularMap,
    "Specular Map",
    NodeCategory::SplatMap,
    super::shared::INPUT_SLOPE_CONTROL_MASK_IN,
    OUTPUTS,
    PARAMS,
    NodeCaps::NONE,
);

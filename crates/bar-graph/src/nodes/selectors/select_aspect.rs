use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static OUTPUT: &[PortDef] = &[PortDef::one("output", "Aspect Mask", PortKind::Heightmap)];

static PARAMS: &[ParamDef] = &[
    ParamDef { key: "direction", default: || ParamValue::Float(0.0), ui: ParamUi::FloatRange { min: 0.0, max: 360.0 } },
    ParamDef { key: "width", default: || ParamValue::Float(90.0), ui: ParamUi::FloatRange { min: 0.0, max: 180.0 } },
    ParamDef { key: "falloff", default: || ParamValue::Float(30.0), ui: ParamUi::FloatRange { min: 0.0, max: 90.0 } },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::SelectAspect,
    "Select Aspect",
    NodeCategory::SplatMap,
    super::shared::INPUT_ONLY_IN,
    OUTPUT,
    PARAMS,
    NodeCaps::NONE,
);

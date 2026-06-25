use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static OUTPUT: &[PortDef] = &[PortDef::one("output", "Curvature", PortKind::Heightmap)];

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "mode",
        default: || ParamValue::String("ridges".to_string()),
        ui: ParamUi::Choices(super::shared::CONVEXITY_MODES),
    },
    ParamDef {
        key: "strength",
        default: || ParamValue::Float(1.0),
        ui: ParamUi::FloatRange { min: 0.1, max: 8.0 },
    },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::SelectConvexity,
    "Select Convexity",
    NodeCategory::SplatMap,
    super::shared::INPUT_ONLY_IN,
    OUTPUT,
    PARAMS,
    NodeCaps::NONE,
);

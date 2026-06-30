use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static OUTPUT: &[PortDef] = &[PortDef::one("output", "Heightmap", PortKind::Heightmap)];
static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "direction",
        default: || ParamValue::String("linear_y".to_string()),
        ui: ParamUi::Choices(&["linear_x", "linear_y", "radial", "angular"]),
    },
    ParamDef {
        key: "invert",
        default: || ParamValue::Bool(false),
        ui: ParamUi::Bool,
    },
    ParamDef {
        key: "center_x",
        default: || ParamValue::Float(0.5),
        ui: ParamUi::FloatFree,
    },
    ParamDef {
        key: "center_y",
        default: || ParamValue::Float(0.5),
        ui: ParamUi::FloatFree,
    },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::Gradient,
    "Gradient",
    NodeCategory::Generator,
    super::shared::CONTROL_IN,
    OUTPUT,
    PARAMS,
    NodeCaps::source(),
);

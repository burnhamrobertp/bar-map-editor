use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static OUTPUT: &[PortDef] = &[PortDef::one("output", "Output", PortKind::Heightmap)];

static PARAMS: &[ParamDef] = &[ParamDef {
    key: "radius",
    default: || ParamValue::Float(4.0),
    ui: ParamUi::FloatRange {
        min: 0.5,
        max: 20.0,
    },
}];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::MaskShrink,
    "Mask Shrink",
    NodeCategory::Mask,
    super::shared::INPUT_ONLY_IN,
    OUTPUT,
    PARAMS,
    NodeCaps::NONE,
);

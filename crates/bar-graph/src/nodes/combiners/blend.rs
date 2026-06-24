use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

/// Combine method (WM Combiner "Method"); `blend` = lerp(a,b).
const MODES: &[&str] = &[
    "blend", "add", "subtract", "multiply", "divide", "average", "screen", "power",
    "difference", "max", "min",
];

static INPUTS: &[PortDef] = &[
    PortDef::one("a", "Input A", PortKind::Heightmap),
    PortDef::one("b", "Input B", PortKind::Heightmap),
    PortDef::one("control", "Control", PortKind::Control),
    PortDef::one("mask", "Mask", PortKind::Mask),
];
static PARAMS: &[ParamDef] = &[
    ParamDef { key: "mode", default: || ParamValue::String("blend".to_string()), ui: ParamUi::Choices(MODES) },
    ParamDef { key: "factor", default: || ParamValue::Float(0.5), ui: ParamUi::FloatRange { min: 0.0, max: 1.0 } },
];

pub static DEF: NodeDef = NodeDef {
    scalar_bindable: &["factor"],
    ..NodeDef::basic(
        NodeType::Blend,
        "Combine",
        NodeCategory::Combiner,
        INPUTS,
        super::shared::OUTPUT,
        PARAMS,
        NodeCaps::NONE,
    )
};

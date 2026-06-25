use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[
    PortDef::one("base", "Base", PortKind::Color),
    PortDef::one("overlay", "Overlay", PortKind::Color),
    PortDef::one("distribution", "Distribution", PortKind::Heightmap),
];

static BLEND_MODES: &[&str] = &["over", "multiply", "screen", "add"];

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "blend_mode",
        default: || ParamValue::String("over".to_string()),
        ui: ParamUi::Choices(BLEND_MODES),
    },
    ParamDef {
        key: "opacity",
        default: || ParamValue::Float(1.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::LayerBlend,
    "Layer Blend",
    NodeCategory::Colorizer,
    INPUTS,
    super::shared::TEXTURE_OUT,
    PARAMS,
    NodeCaps::NONE,
);

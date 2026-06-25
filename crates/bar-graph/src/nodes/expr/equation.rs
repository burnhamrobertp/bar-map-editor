use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{CustomPanel, NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[
    PortDef::one("a", "A", PortKind::Heightmap),
    PortDef::one("b", "B", PortKind::Heightmap),
    PortDef::one("c", "C", PortKind::Heightmap),
    PortDef::one("d", "D", PortKind::Heightmap),
];

static OUTPUTS: &[PortDef] = &[PortDef::one("output", "Output", PortKind::Heightmap)];

static PARAMS: &[ParamDef] = &[ParamDef {
    key: "formula",
    default: || ParamValue::String("a".to_string()),
    ui: ParamUi::Text,
}];

pub static DEF: NodeDef = NodeDef {
    custom_panel: Some(CustomPanel::Equation),
    ..NodeDef::basic(
        NodeType::Equation,
        "Equation",
        NodeCategory::Filter,
        INPUTS,
        OUTPUTS,
        PARAMS,
        NodeCaps::NONE,
    )
};

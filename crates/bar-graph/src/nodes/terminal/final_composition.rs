use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[
    PortDef::one("heightmap", "Heightmap", PortKind::Heightmap),
    PortDef::one("texture", "Texture", PortKind::Color),
    PortDef::one("normalmap", "Normal Map", PortKind::Color),
    PortDef::one("metalmap", "Metal Map", PortKind::Heightmap),
    PortDef::one("typemap", "Type Map", PortKind::Heightmap),
    PortDef::one("grassmap", "Grass Map", PortKind::Heightmap),
    PortDef::one("specular", "Specular", PortKind::Heightmap),
    PortDef::many("files", "Files", PortKind::FileList),
];

static OUTPUTS: &[PortDef] = &[];

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "heightmap_layer_asset_id",
        default: || ParamValue::String(String::new()),
        ui: ParamUi::Hidden,
    },
    ParamDef {
        key: "heightmap_layer_asset_path",
        default: || ParamValue::String(String::new()),
        ui: ParamUi::Hidden,
    },
    ParamDef {
        key: "color_layer_asset_id",
        default: || ParamValue::String(String::new()),
        ui: ParamUi::Hidden,
    },
    ParamDef {
        key: "color_layer_asset_path",
        default: || ParamValue::String(String::new()),
        ui: ParamUi::Hidden,
    },
    ParamDef {
        key: "metalmap_layer_asset_id",
        default: || ParamValue::String(String::new()),
        ui: ParamUi::Hidden,
    },
    ParamDef {
        key: "metalmap_layer_asset_path",
        default: || ParamValue::String(String::new()),
        ui: ParamUi::Hidden,
    },
    ParamDef {
        key: "typemap_layer_asset_id",
        default: || ParamValue::String(String::new()),
        ui: ParamUi::Hidden,
    },
    ParamDef {
        key: "typemap_layer_asset_path",
        default: || ParamValue::String(String::new()),
        ui: ParamUi::Hidden,
    },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::FinalComposition,
    "Final Composition",
    NodeCategory::Terminal,
    INPUTS,
    OUTPUTS,
    PARAMS,
    NodeCaps {
        is_terminal: true,
        holds_assets: true,
        ..NodeCaps::NONE
    },
);

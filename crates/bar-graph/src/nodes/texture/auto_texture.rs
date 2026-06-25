use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

static INPUTS: &[PortDef] = &[
    PortDef::one("input", "Heightmap", PortKind::Heightmap),
    PortDef::one("slope", "Slope Map", PortKind::Heightmap),
    PortDef::one("control", "Control", PortKind::Control),
    PortDef::one("mask", "Mask", PortKind::Mask),
];

static BIOMES: &[&str] = &[
    "temperate",
    "grassland",
    "mountainous",
    "tropical",
    "desert",
    "tundra",
    "lunar",
];

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "biome",
        default: || ParamValue::String("temperate".to_string()),
        ui: ParamUi::Choices(BIOMES),
    },
    ParamDef {
        key: "slope_power",
        default: || ParamValue::Float(0.7),
        ui: ParamUi::FloatRange { min: 0.0, max: 4.0 },
    },
    ParamDef {
        key: "slope_blend",
        default: || ParamValue::Float(1.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "rock_color",
        default: || ParamValue::String("736B61".to_string()),
        ui: ParamUi::Color,
    },
    ParamDef {
        key: "ao_strength",
        default: || ParamValue::Float(1.0),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "detail_strength",
        default: || ParamValue::Float(0.15),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
];

pub static DEF: NodeDef = NodeDef {
    node_type: NodeType::AutoTexture,
    label: "Auto Texture",
    category: NodeCategory::Colorizer,
    inputs: INPUTS,
    outputs: super::shared::TEXTURE_OUT,
    params: PARAMS,
    caps: NodeCaps::NONE,
    dynamic_params: None,
    dynamic_param_ui: None,
    param_side_effects: Some(biome_side_effects),
    post_build: None,
    scalar_bindable: &[],
    custom_panel: None,
};

/// Picking a `biome` rewrites rock_color + slope_power from the biome table.
fn biome_side_effects(key: &str, v: &ParamValue) -> Vec<(String, ParamValue)> {
    if key == "biome" {
        if let ParamValue::String(b) = v {
            let bd = crate::defaults::biome_defaults(b);
            return vec![
                (
                    "rock_color".into(),
                    ParamValue::String(bd.rock_color.into()),
                ),
                ("slope_power".into(), ParamValue::Float(bd.slope_power)),
            ];
        }
    }

    Vec::new()
}

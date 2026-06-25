//! Data + helpers shared across the noise generators.

use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

/// `character` presets for the FBM generators (Perlin / Simplex / Worley).
pub const FBM_CHARACTERS: &[&str] = &[
    "rolling_hills",
    "rugged",
    "broad_waves",
    "fine_detail",
    "wispy",
];
/// `character` presets for RidgedNoise (its `|2x-1|` fold needs its own tuning).
pub const RIDGED_CHARACTERS: &[&str] = &[
    "ridges",
    "jagged_peaks",
    "broken_terrain",
    "broad_ridges",
    "spires",
];

/// A generator's one control input + one heightmap output (shared by the
/// FBM nodes, Voronoi, Gradient).
pub static CONTROL_IN: &[PortDef] = &[PortDef::one("control", "Control", PortKind::Control)];
pub static HEIGHTMAP_OUT: &[PortDef] = &[PortDef::one("output", "Heightmap", PortKind::Heightmap)];

/// The six FBM params, identical for Perlin / Simplex / Worley.
pub static FBM_PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "character",
        default: || ParamValue::String("rolling_hills".to_string()),
        ui: ParamUi::Choices(FBM_CHARACTERS),
    },
    ParamDef {
        key: "frequency",
        default: || ParamValue::Float(4.0),
        ui: ParamUi::FloatRange {
            min: 0.1,
            max: 128.0,
        },
    },
    ParamDef {
        key: "octaves",
        default: || ParamValue::UInt(6),
        ui: ParamUi::UIntRange { min: 1, max: 12 },
    },
    ParamDef {
        key: "lacunarity",
        default: || ParamValue::Float(2.0),
        ui: ParamUi::FloatRange { min: 1.0, max: 4.0 },
    },
    ParamDef {
        key: "persistence",
        default: || ParamValue::Float(0.5),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "seed",
        default: || ParamValue::UInt(0),
        ui: ParamUi::UIntFree,
    },
    ParamDef {
        key: "steepness",
        default: || ParamValue::Float(0.5),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "elevation",
        default: || ParamValue::Float(0.5),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
    ParamDef {
        key: "offset",
        default: || ParamValue::Float(0.0),
        ui: ParamUi::FloatRange {
            min: -0.5,
            max: 0.5,
        },
    },
    ParamDef {
        key: "gain",
        default: || ParamValue::Float(0.5),
        ui: ParamUi::FloatRange { min: 0.0, max: 1.0 },
    },
];

/// Build the descriptor for an FBM noise generator (Perlin / Simplex / Worley).
pub const fn fbm_def(node_type: NodeType, label: &'static str) -> NodeDef {
    NodeDef {
        node_type,
        label,
        category: NodeCategory::Generator,
        inputs: CONTROL_IN,
        outputs: HEIGHTMAP_OUT,
        params: FBM_PARAMS,
        caps: NodeCaps {
            gpu_eligible: true,
            ..NodeCaps::source()
        },
        dynamic_params: None,
        dynamic_param_ui: None,
        param_side_effects: Some(fbm_character_side_effects),
        post_build: None,
        scalar_bindable: &["frequency", "persistence", "lacunarity"],
        custom_panel: None,
    }
}

/// Picking a `character` rewrites frequency/octaves/lacunarity/persistence.
/// Perlin / Simplex / Worley share the FBM table.
pub fn fbm_character_side_effects(key: &str, v: &ParamValue) -> Vec<(String, ParamValue)> {
    character_effects(&NodeType::PerlinNoise, key, v)
}

pub fn ridged_character_side_effects(key: &str, v: &ParamValue) -> Vec<(String, ParamValue)> {
    character_effects(&NodeType::RidgedNoise, key, v)
}

fn character_effects(nt: &NodeType, key: &str, v: &ParamValue) -> Vec<(String, ParamValue)> {
    if key == "character" {
        if let ParamValue::String(c) = v {
            let cd = crate::defaults::character_defaults(nt, c);
            return vec![
                ("frequency".into(), ParamValue::Float(cd.frequency)),
                ("octaves".into(), ParamValue::UInt(cd.octaves)),
                ("lacunarity".into(), ParamValue::Float(cd.lacunarity)),
                ("persistence".into(), ParamValue::Float(cd.persistence)),
            ];
        }
    }
    Vec::new()
}

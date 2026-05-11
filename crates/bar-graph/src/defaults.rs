//! Default parameter values for each `NodeType`.
//!
//! Every node type carries a documented set of parameters. Returning the full
//! default set from this module ensures that:
//!
//! 1. New nodes added via the GUI palette are immediately runnable — a Bundler
//!    has its `target` field set, an erosion node has sensible iteration
//!    counts, etc.
//! 2. Recipes that omit params (or use older serialised formats with new
//!    params introduced later) load with reasonable values rather than
//!    failing at evaluation time.
//! 3. The property editor knows which keys to surface for each type.
//!
//! `Node::new` calls this function to populate `params` at construction.
//! `Recipe::build_graph` merges recipe-supplied params on top of these
//! defaults so explicit values always win.

use std::collections::HashMap;

use crate::node::{NodeType, ParamValue};

/// Return the full set of default parameter values for a node type.
pub fn default_params(node_type: &NodeType) -> HashMap<String, ParamValue> {
    let entries: Vec<(&str, ParamValue)> = match node_type {
        NodeType::PerlinNoise | NodeType::SimplexNoise | NodeType::WorleyNoise => vec![
            // Named character preset that drives frequency / octaves /
            // lacunarity / persistence in one click. The four params
            // below are still individually editable — character is a
            // starting point, not a lock. Surfaced as a dropdown via
            // `param_choices`.
            ("character", ParamValue::String("rolling_hills".to_string())),
            ("frequency", ParamValue::Float(4.0)),
            ("octaves", ParamValue::UInt(6)),
            ("lacunarity", ParamValue::Float(2.0)),
            ("persistence", ParamValue::Float(0.5)),
            ("seed", ParamValue::UInt(0)),
        ],
        NodeType::RidgedNoise => vec![
            ("character", ParamValue::String("ridges".to_string())),
            ("frequency", ParamValue::Float(2.0)),
            ("octaves", ParamValue::UInt(6)),
            ("lacunarity", ParamValue::Float(2.0)),
            ("persistence", ParamValue::Float(0.5)),
            ("seed", ParamValue::UInt(0)),
        ],
        NodeType::Constant => vec![("value", ParamValue::Float(0.5))],
        NodeType::Blur => vec![("radius", ParamValue::Float(1.0))],
        NodeType::Clamp => vec![
            ("min", ParamValue::Float(0.0)),
            ("max", ParamValue::Float(1.0)),
        ],
        NodeType::Blend => vec![("factor", ParamValue::Float(0.5))],
        NodeType::HydraulicErosion => vec![
            ("iterations", ParamValue::UInt(50_000)),
            ("erosion_rate", ParamValue::Float(0.01)),
            ("deposition_rate", ParamValue::Float(0.01)),
        ],
        NodeType::ThermalErosion => vec![
            ("iterations", ParamValue::UInt(100)),
            ("talus_angle", ParamValue::Float(0.6)),
        ],
        NodeType::HeightSelect => vec![
            ("low", ParamValue::Float(0.3)),
            ("high", ParamValue::Float(0.7)),
            ("falloff", ParamValue::Float(0.1)),
        ],
        NodeType::MaskThreshold => vec![
            ("threshold", ParamValue::Float(0.5)),
            ("smoothness", ParamValue::Float(0.0)),
        ],
        NodeType::MaskBlur => vec![("radius", ParamValue::Float(2.0))],
        NodeType::BiasGain => vec![
            ("bias", ParamValue::Float(0.5)),
            ("gain", ParamValue::Float(0.5)),
        ],
        NodeType::Mirror => vec![("mode", ParamValue::String("mirror_x".to_string()))],
        NodeType::Terrace => vec![
            ("step_count", ParamValue::UInt(4)),
            ("smoothing", ParamValue::Float(0.0)),
        ],
        NodeType::Sharpen => vec![
            ("radius", ParamValue::Float(1.0)),
            ("strength", ParamValue::Float(1.0)),
        ],
        NodeType::Displacement => vec![("strength", ParamValue::Float(0.1))],
        NodeType::NormalMap => vec![("strength", ParamValue::Float(1.0))],
        NodeType::GrassMap => vec![
            ("min_height", ParamValue::Float(0.15)),
            ("max_height", ParamValue::Float(0.7)),
            ("max_slope", ParamValue::Float(0.4)),
            ("density", ParamValue::Float(1.0)),
            ("falloff", ParamValue::Float(0.05)),
        ],
        NodeType::AutoTexture => vec![
            // Named biome gradient. Each biome is a complete colour
            // palette + threshold table — desert places sand and red
            // rock at heights where temperate places forest and grass.
            // Surfaced as a dropdown via `param_choices`.
            ("biome", ParamValue::String("temperate".to_string())),
            // How aggressively the rock colour takes over on steep
            // slopes. 0.0 → linear (gentle slopes already rocky),
            // higher values push the transition to only the steepest
            // pixels. 0.7 matches the original hardcoded behaviour.
            ("slope_power", ParamValue::Float(0.7)),
            // Overall scale of the slope→rock blend. 0 disables the
            // rock blend entirely (pure elevation gradient); 1.0 keeps
            // the full effect.
            ("slope_blend", ParamValue::Float(1.0)),
            // Hex RGB of the rock tint mixed in on steep slopes.
            ("rock_color", ParamValue::String("736B61".to_string())),
            // Strength of the local-variation ambient occlusion
            // darkening. 0 disables AO; 1.0 keeps the full effect.
            ("ao_strength", ParamValue::Float(1.0)),
        ],
        NodeType::RockSoil => vec![
            ("rock_color", ParamValue::String("807870".to_string())),
            ("soil_color", ParamValue::String("8B6914".to_string())),
            ("slope_threshold", ParamValue::Float(0.4)),
            ("slope_blend", ParamValue::Float(0.3)),
            ("ao_strength", ParamValue::Float(0.8)),
            ("detail_strength", ParamValue::Float(0.25)),
        ],
        NodeType::Vegetation => vec![
            ("vegetation_color", ParamValue::String("4A7020".to_string())),
            ("dry_color", ParamValue::String("8B7355".to_string())),
            ("altitude_max", ParamValue::Float(0.6)),
            ("slope_cutoff", ParamValue::Float(0.5)),
            ("slope_blend", ParamValue::Float(0.2)),
            ("ao_strength", ParamValue::Float(0.6)),
            ("detail_strength", ParamValue::Float(0.2)),
        ],
        NodeType::LayerBlend => vec![
            ("blend_mode", ParamValue::String("over".to_string())),
            ("opacity", ParamValue::Float(1.0)),
        ],
        NodeType::TextureWeightmap => vec![
            ("layer_count", ParamValue::UInt(2)),
            (
                "priority_type",
                ParamValue::String("weighted_blend".to_string()),
            ),
            // Slot 0 = highest default priority (7), slot 7 = lowest (0).
            ("priority_0", ParamValue::Float(7.0)),
            ("exclusion_0", ParamValue::Float(0.0)),
            ("priority_1", ParamValue::Float(6.0)),
            ("exclusion_1", ParamValue::Float(0.0)),
            ("priority_2", ParamValue::Float(5.0)),
            ("exclusion_2", ParamValue::Float(0.0)),
            ("priority_3", ParamValue::Float(4.0)),
            ("exclusion_3", ParamValue::Float(0.0)),
            ("priority_4", ParamValue::Float(3.0)),
            ("exclusion_4", ParamValue::Float(0.0)),
            ("priority_5", ParamValue::Float(2.0)),
            ("exclusion_5", ParamValue::Float(0.0)),
            ("priority_6", ParamValue::Float(1.0)),
            ("exclusion_6", ParamValue::Float(0.0)),
            ("priority_7", ParamValue::Float(0.0)),
            ("exclusion_7", ParamValue::Float(0.0)),
        ],
        NodeType::ColorRamp => vec![
            ("stop_count", ParamValue::UInt(2)),
            ("pos_0", ParamValue::Float(0.0)),
            ("color_0", ParamValue::String("000000".to_string())),
            ("pos_1", ParamValue::Float(1.0)),
            ("color_1", ParamValue::String("FFFFFF".to_string())),
            ("pos_2", ParamValue::Float(0.25)),
            ("color_2", ParamValue::String("404040".to_string())),
            ("pos_3", ParamValue::Float(0.375)),
            ("color_3", ParamValue::String("606060".to_string())),
            ("pos_4", ParamValue::Float(0.5)),
            ("color_4", ParamValue::String("808080".to_string())),
            ("pos_5", ParamValue::Float(0.625)),
            ("color_5", ParamValue::String("A0A0A0".to_string())),
            ("pos_6", ParamValue::Float(0.75)),
            ("color_6", ParamValue::String("C0C0C0".to_string())),
            ("pos_7", ParamValue::Float(0.875)),
            ("color_7", ParamValue::String("E0E0E0".to_string())),
        ],
        NodeType::SpecularMap => vec![
            ("rock_specular", ParamValue::Float(0.6)),
            ("flat_specular", ParamValue::Float(0.2)),
            ("water_specular", ParamValue::Float(0.9)),
            ("water_height", ParamValue::Float(0.2)),
            ("snow_specular", ParamValue::Float(0.7)),
            ("snow_height", ParamValue::Float(0.85)),
        ],
        NodeType::Sculpt => vec![
            // Hex-encoded flat u8 delta buffer (one byte per pixel).
            // 128 = no change; 0 = maximum subtract; 255 = maximum add.
            // Empty string means no deltas applied -- node is a pure passthrough.
            // Format and encoding identical to PaintedHeightmap.
            ("data", ParamValue::String(String::new())),
            // Canvas resolution. Same power-of-two choices as PaintedHeightmap.
            // Locked once the user has painted (non-empty data).
            ("resolution", ParamValue::UInt(256)),
            // Max delta magnitude: delta_applied = (v - 128) / 128 * scale.
            // 0.5 = max +-50% change relative to the input value.
            ("scale", ParamValue::Float(0.5)),
        ],
        NodeType::Bundler => vec![
            // bar-editor only ever exports spring-smf packaged as
            // 7z (the BAR map format). Those format choices used to
            // be exposed as `target` / `archive_format` params; they
            // were dropped because there's nothing to vary. Leave
            // map_name + output_path: those genuinely differ per map.
            ("map_name", ParamValue::String("my_map".to_string())),
            ("output_path", ParamValue::String("{name}.sd7".to_string())),
        ],
        NodeType::FileReference => vec![
            ("path", ParamValue::String(String::new())),
            ("bundle_path", ParamValue::String(String::new())),
        ],
        NodeType::SmfImport => vec![
            ("path", ParamValue::String(String::new())),
            ("load_metalmap", ParamValue::Bool(true)),
            ("load_typemap", ParamValue::Bool(true)),
        ],
        NodeType::SmtImport => vec![
            ("path", ParamValue::String(String::new())),
            ("smf_path", ParamValue::String(String::new())),
            ("tiles_x", ParamValue::UInt(0)),
            ("tiles_y", ParamValue::UInt(0)),
            // 4096 covers typical BAR maps (8×8 to 32×32 squares) at full
            // native texture resolution. Larger maps are still capped, but
            // 4096² × 4 bytes RGBA = 64 MB — well within GPU memory.
            // The cap was previously 512 which lost ~6× detail on
            // kolmog-class maps (1280-px native textures).
            ("max_preview_size", ParamValue::UInt(4096)),
        ],
        NodeType::PaintedHeightmap => vec![
            // Hex-encoded greyscale pixel grid (each pixel is one u8).
            // Empty until the user paints. The buffer is sized to
            // `resolution × resolution` on first paint.
            ("data", ParamValue::String(String::new())),
            // Canvas resolution. Power-of-two values map cleanly to
            // BAR's 64-px square grid. 256 is the practical default —
            // smaller (64, 128) for masks, larger (512) for primary
            // hand-drawn terrain.
            ("resolution", ParamValue::UInt(256)),
        ],
        NodeType::PaintedTexture => vec![
            // Hex-encoded RGB pixel grid (3 bytes per pixel). Empty
            // until the user paints. Resolution is fixed at 256.
            ("data", ParamValue::String(String::new())),
            // Current brush colour as packed 0xRRGGBB. Lives in the
            // node so the most-recent colour persists across edits.
            ("brush_color", ParamValue::String("8B7355".to_string())),
        ],
        NodeType::Voronoi => vec![
            ("frequency", ParamValue::Float(8.0)),
            ("seed", ParamValue::UInt(0)),
            // Enum: f1 / f2 / f2_f1 / cell. The properties UI
            // surfaces this as a dropdown via `param_choices`.
            ("mode", ParamValue::String("f1".to_string())),
        ],
        NodeType::Gradient => vec![
            // Enum: linear_x / linear_y / radial / angular.
            ("direction", ParamValue::String("linear_y".to_string())),
            ("invert", ParamValue::Bool(false)),
            ("center_x", ParamValue::Float(0.5)),
            ("center_y", ParamValue::Float(0.5)),
        ],
        NodeType::SubgraphInput => vec![
            // External port name shown on the collapsed subgraph
            // block. Empty by default — the wrapper auto-generates
            // a label from the kind (with a numeric suffix when the
            // subgraph has multiple ports of the same kind), and the
            // user fills this in only when they want a custom label.
            ("name", ParamValue::String(String::new())),
            // Port kind for both the input and the output side. The
            // engine flips both ports' `PortKind` when this changes
            // — see `apply_subgraph_io_kind` in `bar-graph::node`.
            ("kind", ParamValue::String("Heightmap".to_string())),
        ],
        NodeType::SubgraphOutput => vec![
            ("name", ParamValue::String(String::new())),
            ("kind", ParamValue::String("Heightmap".to_string())),
        ],
        // PassThrough manages its files via a custom UI.
        // Other node types intentionally have no default params.
        _ => Vec::new(),
    };
    entries
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

/// Fixed set of valid values for an enum-like string parameter, or
/// `None` if the param accepts free-form text. The properties panel
/// uses this to render a ComboBox dropdown instead of a TextEdit
/// for params with constrained choices — a `mode` of "voom" should
/// be impossible to type.
pub fn param_choices(node_type: &NodeType, key: &str) -> Option<&'static [&'static str]> {
    match (node_type, key) {
        (NodeType::Mirror, "mode") => Some(&[
            "mirror_x",
            "mirror_y",
            "mirror_xy",
            "rotate_180",
            "rotate_90_4way",
        ]),
        (NodeType::Voronoi, "mode") => Some(&["f1", "f2", "f2_f1", "cell"]),
        (NodeType::Gradient, "direction") => Some(&["linear_x", "linear_y", "radial", "angular"]),
        (NodeType::AutoTexture, "biome") => Some(&[
            "temperate",
            "grassland",
            "mountainous",
            "tropical",
            "desert",
            "tundra",
            "lunar",
        ]),
        (NodeType::LayerBlend, "blend_mode") => Some(&["over", "multiply", "screen", "add"]),
        (NodeType::TextureWeightmap, "priority_type") => Some(&["weighted_blend", "priority"]),
        (NodeType::PerlinNoise | NodeType::SimplexNoise | NodeType::WorleyNoise, "character") => {
            Some(&[
                "rolling_hills",
                "rugged",
                "broad_waves",
                "fine_detail",
                "wispy",
            ])
        }
        (NodeType::RidgedNoise, "character") => Some(&[
            "ridges",
            "jagged_peaks",
            "broken_terrain",
            "broad_ridges",
            "spires",
        ]),
        (NodeType::SubgraphInput | NodeType::SubgraphOutput, "kind") => {
            Some(&["Heightmap", "Color", "Mask", "Scalar", "File", "FileList"])
        }
        _ => None,
    }
}

/// True if a String-typed param holds a 6-digit `RRGGBB` colour. The
/// properties panel uses this to render a colour-picker swatch
/// instead of a free-form text field.
pub fn param_is_color(node_type: &NodeType, key: &str) -> bool {
    matches!(
        (node_type, key),
        (NodeType::AutoTexture, "rock_color")
            | (NodeType::RockSoil, "rock_color")
            | (NodeType::RockSoil, "soil_color")
            | (NodeType::Vegetation, "vegetation_color")
            | (NodeType::Vegetation, "dry_color")
            | (NodeType::PaintedTexture, "brush_color")
    ) || (node_type == &NodeType::ColorRamp
        && key.starts_with("color_")
        && key[6..].parse::<u8>().is_ok())
}

/// Per-biome defaults for the AutoTexture params that are biome-
/// sensitive. Applied by the GUI when the user picks a new biome
/// from the dropdown — re-selecting the same biome is a no-op so
/// the user's tweaks aren't blown away on every render.
///
/// `rock_color` shifts because each biome has a natural rock tint
/// (red sandstone for desert, grey-blue for tundra, dark regolith
/// for lunar). `slope_power` shifts because the threshold for "the
/// slope is now rock" is biome-dependent — desert sand clings to
/// gentler slopes (high power), mountainous rock dominates earlier
/// (low power).
pub struct BiomeDefaults {
    pub rock_color: &'static str,
    pub slope_power: f32,
}

/// Per-character defaults for the noise-node FBM params. Picking a
/// new character from the dropdown swaps frequency / octaves /
/// lacunarity / persistence in one click — the user can then tune
/// individually. Re-selecting the same character is a no-op so
/// tweaks survive UI redraws.
///
/// Values are tuned via `om preview-macro` against a single-Perlin
/// recipe; see `assets/macros/character-test.json`.
pub struct CharacterDefaults {
    pub frequency: f32,
    pub octaves: u32,
    pub lacunarity: f32,
    pub persistence: f32,
}

pub fn character_defaults(node_type: &NodeType, character: &str) -> CharacterDefaults {
    match node_type {
        NodeType::RidgedNoise => ridged_character_defaults(character),
        // Perlin / Simplex / Worley share the same FBM machinery and
        // get the same value set. Worley's cellular post-process makes
        // the result texturally different but the param ranges that
        // produce 'rolling hills' vs 'fine detail' are the same.
        _ => perlin_character_defaults(character),
    }
}

fn perlin_character_defaults(character: &str) -> CharacterDefaults {
    match character {
        // Pulled toward larger, smoother features so it sits well
        // below `rugged` on the chaos axis.
        "rolling_hills" => CharacterDefaults {
            frequency: 2.5,
            octaves: 5,
            lacunarity: 2.0,
            persistence: 0.45,
        },
        // Much higher frequency + persistence than rolling_hills so
        // the difference is obvious at first glance.
        "rugged" => CharacterDefaults {
            frequency: 7.0,
            octaves: 7,
            lacunarity: 2.4,
            persistence: 0.72,
        },
        // Long wavelengths dominate; almost no fine detail.
        "broad_waves" => CharacterDefaults {
            frequency: 1.2,
            octaves: 3,
            lacunarity: 2.0,
            persistence: 0.4,
        },
        // Dense small-scale variation. Pushed even higher so it's
        // unmistakably finer than `rugged`.
        "fine_detail" => CharacterDefaults {
            frequency: 10.0,
            octaves: 8,
            lacunarity: 2.0,
            persistence: 0.5,
        },
        // Upper octaves heavily attenuated → a soft smoky field.
        "wispy" => CharacterDefaults {
            frequency: 4.0,
            octaves: 6,
            lacunarity: 2.0,
            persistence: 0.2,
        },
        // Fallback for unknown values. Matches `rolling_hills`.
        _ => CharacterDefaults {
            frequency: 2.5,
            octaves: 5,
            lacunarity: 2.0,
            persistence: 0.45,
        },
    }
}

/// RidgedNoise's `|2x-1|` post-process inverts and folds the FBM
/// output, so the same constants that make Perlin "fine detail" make
/// RidgedNoise look like static. These values are tuned so each
/// character name actually reads like its name when the |2x-1|
/// transform is applied.
fn ridged_character_defaults(character: &str) -> CharacterDefaults {
    match character {
        "ridges" => CharacterDefaults {
            frequency: 2.0,
            octaves: 6,
            lacunarity: 2.0,
            persistence: 0.5,
        },
        // Higher frequency + persistence — sharper, denser ridge
        // network, like a heavily fractured massif.
        "jagged_peaks" => CharacterDefaults {
            frequency: 3.5,
            octaves: 7,
            lacunarity: 2.3,
            persistence: 0.65,
        },
        // High frequency, low persistence — many small ridges, no
        // dominant structure. Reads as broken-up rubble.
        "broken_terrain" => CharacterDefaults {
            frequency: 5.0,
            octaves: 6,
            lacunarity: 2.0,
            persistence: 0.35,
        },
        // Low frequency — a few wide ridges across the whole map.
        "broad_ridges" => CharacterDefaults {
            frequency: 1.2,
            octaves: 4,
            lacunarity: 2.0,
            persistence: 0.5,
        },
        // High lacunarity + high persistence — needle-like vertical
        // features rising sharply from the floor.
        "spires" => CharacterDefaults {
            frequency: 2.5,
            octaves: 7,
            lacunarity: 2.6,
            persistence: 0.8,
        },
        // Fallback. Matches `ridges`.
        _ => CharacterDefaults {
            frequency: 2.0,
            octaves: 6,
            lacunarity: 2.0,
            persistence: 0.5,
        },
    }
}

/// Compute the side-effect param writes that should follow setting
/// `(node_type).(key) = new_val`. Returns a list of `(param_key,
/// new_value)` pairs the caller applies to the same node.
///
/// Examples:
/// - `(AutoTexture, "biome", "desert")` → reset rock_color and
///   slope_power to desert's defaults.
/// - `(PerlinNoise, "character", "rugged")` → reset frequency,
///   octaves, lacunarity, persistence to rugged's defaults.
///
/// Both the inner-node properties editor and the collapsed-macro
/// knob panel call this so the behaviour is consistent regardless
/// of which surface the user edits the param from.
pub fn param_side_effects(
    node_type: &NodeType,
    key: &str,
    new_val: &ParamValue,
) -> Vec<(String, ParamValue)> {
    match (node_type, key, new_val) {
        (NodeType::AutoTexture, "biome", ParamValue::String(b)) => {
            let bd = biome_defaults(b);
            vec![
                (
                    "rock_color".into(),
                    ParamValue::String(bd.rock_color.into()),
                ),
                ("slope_power".into(), ParamValue::Float(bd.slope_power)),
            ]
        }
        (
            NodeType::PerlinNoise
            | NodeType::SimplexNoise
            | NodeType::WorleyNoise
            | NodeType::RidgedNoise,
            "character",
            ParamValue::String(c),
        ) => {
            let cd = character_defaults(node_type, c);
            vec![
                ("frequency".into(), ParamValue::Float(cd.frequency)),
                ("octaves".into(), ParamValue::UInt(cd.octaves)),
                ("lacunarity".into(), ParamValue::Float(cd.lacunarity)),
                ("persistence".into(), ParamValue::Float(cd.persistence)),
            ]
        }
        _ => Vec::new(),
    }
}

pub fn biome_defaults(biome: &str) -> BiomeDefaults {
    match biome {
        "grassland" => BiomeDefaults {
            rock_color: "8C7858",
            slope_power: 0.9,
        },
        "mountainous" => BiomeDefaults {
            rock_color: "6E6862",
            slope_power: 0.5,
        },
        "tropical" => BiomeDefaults {
            rock_color: "8C5C44",
            slope_power: 0.8,
        },
        "desert" => BiomeDefaults {
            rock_color: "9C5A38",
            slope_power: 1.2,
        },
        "tundra" => BiomeDefaults {
            rock_color: "9CA0A4",
            slope_power: 0.6,
        },
        "lunar" => BiomeDefaults {
            rock_color: "4A4A4A",
            slope_power: 0.5,
        },
        _ => BiomeDefaults {
            rock_color: "736B61",
            slope_power: 0.7,
        },
    }
}

/// Returns the `[min, max]` range for slider display of the given Float
/// param, or `None` if the param should use a free-form drag-value.
pub fn param_float_range(node_type: &NodeType, key: &str) -> Option<(f32, f32)> {
    use NodeType::*;
    Some(match (node_type, key) {
        // Noise
        (PerlinNoise | SimplexNoise | WorleyNoise | RidgedNoise, "frequency") => (0.1, 32.0),
        (PerlinNoise | SimplexNoise | WorleyNoise | RidgedNoise, "lacunarity") => (1.0, 4.0),
        (PerlinNoise | SimplexNoise | WorleyNoise | RidgedNoise, "persistence") => (0.0, 1.0),
        // Utility
        (Constant, "value") => (0.0, 1.0),
        // Filters
        (Blur | MaskBlur, "radius") => (0.1, 20.0),
        (Sharpen, "radius") => (0.1, 10.0),
        (Sharpen, "strength") => (0.0, 4.0),
        (Clamp, "min") | (Clamp, "max") => (0.0, 1.0),
        (Terrace, "smoothing") => (0.0, 1.0),
        (BiasGain, "bias") | (BiasGain, "gain") => (0.0, 1.0),
        (Displacement, "strength") => (0.0, 1.0),
        (Blend, "factor") => (0.0, 1.0),
        (Sculpt, "scale") => (0.0, 1.0),
        // Erosion
        (HydraulicErosion, "erosion_rate") | (HydraulicErosion, "deposition_rate") => (0.0, 0.1),
        (ThermalErosion, "talus_angle") => (0.0, 1.0),
        // Select/Mask
        (HeightSelect, "low") | (HeightSelect, "high") => (0.0, 1.0),
        (HeightSelect, "falloff") => (0.0, 0.5),
        (MaskThreshold, "threshold") | (MaskThreshold, "smoothness") => (0.0, 1.0),
        // Texture
        (AutoTexture, "water_level") => (0.0, 0.5),
        (AutoTexture, "beach_width") => (0.0, 0.3),
        (AutoTexture, "snow_height") => (0.5, 1.0),
        (AutoTexture, "slope_power") => (0.0, 4.0),
        (AutoTexture, "slope_blend") | (AutoTexture, "ao_strength") => (0.0, 1.0),
        (RockSoil, "slope_threshold")
        | (RockSoil, "slope_blend")
        | (RockSoil, "ao_strength")
        | (RockSoil, "detail_strength") => (0.0, 1.0),
        (Vegetation, "altitude_max")
        | (Vegetation, "slope_cutoff")
        | (Vegetation, "slope_blend")
        | (Vegetation, "ao_strength")
        | (Vegetation, "detail_strength") => (0.0, 1.0),
        (LayerBlend, "opacity") => (0.0, 1.0),
        (NormalMap, "strength") => (0.0, 4.0),
        (GrassMap, "min_height") | (GrassMap, "max_height") | (GrassMap, "max_slope") => (0.0, 1.0),
        (GrassMap, "density") => (0.0, 2.0),
        (GrassMap, "falloff") => (0.0, 0.5),
        (SpecularMap, "rock_specular")
        | (SpecularMap, "flat_specular")
        | (SpecularMap, "water_specular")
        | (SpecularMap, "snow_specular")
        | (SpecularMap, "water_height")
        | (SpecularMap, "snow_height") => (0.0, 1.0),
        // TextureWeightmap indexed slots
        (TextureWeightmap, k) if k.starts_with("priority_") => (0.0, 16.0),
        (TextureWeightmap, k) if k.starts_with("exclusion_") => (0.0, 1.0),
        // ColorRamp stop positions
        (ColorRamp, k) if k.starts_with("pos_") => (0.0, 1.0),
        _ => return None,
    })
}

/// Returns the `[min, max]` range for slider display of the given UInt
/// param, or `None` if the param should use a free-form drag-value.
pub fn param_uint_range(node_type: &NodeType, key: &str) -> Option<(u32, u32)> {
    use NodeType::*;
    Some(match (node_type, key) {
        (PerlinNoise | SimplexNoise | WorleyNoise | RidgedNoise, "octaves") => (1, 12),
        (Terrace, "step_count") => (2, 32),
        (ThermalErosion, "iterations") => (10, 1_000),
        (TextureWeightmap, "layer_count") => (2, 8),
        (ColorRamp, "stop_count") => (2, 8),
        _ => return None,
    })
}

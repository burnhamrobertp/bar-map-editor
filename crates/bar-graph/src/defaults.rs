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
    crate::nodes::def(node_type)
        .map(crate::nodes::build_params)
        .unwrap_or_default()
}

/// Fixed set of valid values for an enum-like string parameter, or
/// `None` if the param accepts free-form text. The properties panel
/// uses this to render a ComboBox dropdown instead of a TextEdit
/// for params with constrained choices — a `mode` of "voom" should
/// be impossible to type.
pub fn param_choices(node_type: &NodeType, key: &str) -> Option<&'static [&'static str]> {
    crate::nodes::def(node_type).and_then(|d| crate::nodes::param_choices(d, key))
}

/// True if a String-typed param holds a 6-digit `RRGGBB` colour. The
/// properties panel uses this to render a colour-picker swatch
/// instead of a free-form text field.
pub fn param_is_color(node_type: &NodeType, key: &str) -> bool {
    crate::nodes::def(node_type)
        .map(|d| crate::nodes::param_is_color(d, key))
        .unwrap_or(false)
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
/// Values are tuned via `bar-cli preview-macro` against a single-Perlin
/// recipe.
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
    crate::nodes::def(node_type)
        .and_then(|d| crate::nodes::param_side_effects(d, key, new_val))
        .unwrap_or_default()
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
    crate::nodes::def(node_type).and_then(|d| crate::nodes::param_float_range(d, key))
}

/// Returns the `[min, max]` range for slider display of the given UInt
/// param, or `None` if the param should use a free-form drag-value.
pub fn param_uint_range(node_type: &NodeType, key: &str) -> Option<(u32, u32)> {
    crate::nodes::def(node_type).and_then(|d| crate::nodes::param_uint_range(d, key))
}

/// One-line tooltip describing a node parameter, or `None` for params whose
/// meaning is clear from the key (not every param needs help text). Keyed on
/// the param name for terms that mean the same thing across every node;
/// node-specific entries can match on `node_type` as they're written.
pub fn param_description(_node_type: &NodeType, key: &str) -> Option<&'static str> {
    Some(match key {
        "frequency" => {
            "Base spatial frequency of the noise -- higher values pack more detail into the same area."
        }
        "octaves" => {
            "Number of noise layers summed together; more octaves add finer detail at shrinking amplitude."
        }
        "persistence" => {
            "Amplitude falloff per octave (0..1). Higher keeps later octaves louder, giving rougher terrain."
        }
        "lacunarity" => {
            "Frequency multiplier between octaves. ~2.0 is standard; higher spreads detail across more scales."
        }
        "seed" => "Random seed. Change it for a different pattern with the same settings.",
        _ => return None,
    })
}

//! Editor-side visual markers for BAR's per-feature-def point lights.
//!
//! BAR's `gfx_deferred_rendering_GL4` widget attaches point lights to
//! features at runtime, configured from `luaui/configs/DeferredLightsGL4config.lua`
//! in the game archive. BME does not run that widget (per the
//! "no game widget porting" policy), but it does want to give map
//! authors a visual cue for where lights will appear in-game. This
//! module is the canonical lookup table; the rendering layer reads
//! it during feature-instance build and emits small marker
//! instances at the light positions, tinted by colour.
//!
//! The data is hand-mirrored from BAR's config (currently `1.0.x`
//! era values). When BAR's config changes meaningfully, re-mirror;
//! BME deliberately does not parse the config at runtime so editor
//! and game-content schemas stay decoupled.
//!
//! Coverage is intentionally narrow: only feature defs that BAR's
//! config actually configures lights for. Map-side custom feature
//! defs get no entries -- BAR's widget wouldn't render lights for
//! them either, so BME mirrors that.
//!
//! Each entry's units mirror BAR exactly:
//! - `offset` is `(posx, posy, posz)` from the feature's world
//!   position, in Spring elmos.
//! - `color` is the light's diffuse RGB (the `a` multiplier from
//!   BAR's config is folded into the marker tint alpha so brighter
//!   lights show as more saturated markers).
//! - `radius` is the light's influence radius in Spring elmos. The
//!   renderer doesn't currently visualise it, but it round-trips
//!   through the lookup so a future "show light radius" toggle has
//!   the data.

/// A single per-feature-def light marker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeatureLightConfig {
    /// Offset from the feature's world position, in Spring elmos.
    pub offset: [f32; 3],
    /// Light diffuse colour, 0-1 per channel.
    pub color: [f32; 3],
    /// Intensity multiplier from BAR's `lightConfig.a`. Folded into
    /// the marker's tint alpha so visually-stronger lights read as
    /// more opaque markers.
    pub intensity: f32,
    /// Influence radius in Spring elmos.
    pub radius: f32,
}

/// Crystal light variants from `DeferredLightsGL4config.lua`'s
/// `crystalColors` table. Each colour x each size (1, 2, 3) produces
/// one entry. The widget computes the per-entry config the same way:
///
/// ```text
/// radius = (size + 0.2) * (72 * 0.6)
/// posy   = (size + 1.5) * 12
/// posx, posz = 0
/// ```
///
/// Mirrored verbatim in the table below.
const CRYSTAL_COLOURS: &[(&str, [f32; 3], f32)] = &[
    // (suffix, rgb, intensity)
    ("", [0.78, 0.46, 0.94], 0.11), // default = violet-ish
    ("_violet", [0.80, 0.50, 0.95], 0.33),
    ("_blue", [0.10, 0.20, 0.90], 0.33),
    ("_green", [0.10, 0.80, 0.10], 0.15),
    ("_lime", [0.40, 1.00, 0.20], 0.15),
    ("_obsidian", [0.30, 0.20, 0.20], 0.33),
    ("_quartz", [0.30, 0.30, 0.50], 0.33),
    ("_orange", [1.00, 0.50, 0.00], 0.11),
    ("_red", [1.00, 0.20, 0.20], 0.067),
    ("_teal", [0.00, 1.00, 1.00], 0.15),
    ("_team", [1.00, 1.00, 1.00], 0.15),
];

/// Look up the marker configs for a given feature def name. Returns
/// an empty slice for any feature def BAR's widget wouldn't attach a
/// light to.
pub fn lights_for_feature_def(name: &str) -> Vec<FeatureLightConfig> {
    let lower = name.to_ascii_lowercase();
    // BAR's widget builds `pilha_crystal{colour}{size}` -- e.g.
    // `pilha_crystal_violet2`. Decode and look up.
    if let Some(rest) = lower.strip_prefix("pilha_crystal") {
        for (suffix, rgb, intensity) in CRYSTAL_COLOURS {
            if let Some(size_str) = rest.strip_prefix(suffix) {
                if let Ok(size) = size_str.parse::<u32>() {
                    if (1..=3).contains(&size) {
                        let size_f = size as f32;
                        let radius = (size_f + 0.2) * (72.0 * 0.6);
                        let posy = (size_f + 1.5) * 12.0;
                        return vec![FeatureLightConfig {
                            offset: [0.0, posy, 0.0],
                            color: *rgb,
                            intensity: *intensity,
                            radius,
                        }];
                    }
                }
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crystal_violet2_matches_widget_config() {
        // From DeferredLightsGL4config.lua: size=2, _violet ->
        //   radius = (2 + 0.2) * 72 * 0.6 = 95.04
        //   posy   = (2 + 1.5) * 12       = 42.0
        //   rgb    = (0.8, 0.5, 0.95)
        //   alpha  = 0.33
        let lights = lights_for_feature_def("pilha_crystal_violet2");
        assert_eq!(lights.len(), 1);
        let l = lights[0];
        assert!((l.radius - 95.04).abs() < 1e-3);
        assert!((l.offset[1] - 42.0).abs() < 1e-3);
        assert_eq!(l.color, [0.8, 0.5, 0.95]);
        assert!((l.intensity - 0.33).abs() < 1e-6);
    }

    #[test]
    fn crystal_teal1_lookup_works() {
        let lights = lights_for_feature_def("pilha_crystal_teal1");
        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].color, [0.0, 1.0, 1.0]);
        assert!((lights[0].offset[1] - 30.0).abs() < 1e-3); // (1+1.5)*12
    }

    #[test]
    fn case_insensitive_lookup() {
        let upper = lights_for_feature_def("PILHA_CRYSTAL_VIOLET2");
        let lower = lights_for_feature_def("pilha_crystal_violet2");
        assert_eq!(upper.len(), 1);
        assert_eq!(upper[0].color, lower[0].color);
    }

    #[test]
    fn unknown_feature_returns_empty() {
        assert!(lights_for_feature_def("arborreal").is_empty());
        assert!(lights_for_feature_def("rock_random").is_empty());
        assert!(lights_for_feature_def("").is_empty());
    }

    #[test]
    fn invalid_size_returns_empty() {
        assert!(lights_for_feature_def("pilha_crystal_violet9").is_empty());
        assert!(lights_for_feature_def("pilha_crystal_violet0").is_empty());
        assert!(lights_for_feature_def("pilha_crystal_violet").is_empty());
    }

    #[test]
    fn all_three_sizes_have_distinct_radii() {
        let s1 = lights_for_feature_def("pilha_crystal_blue1")[0].radius;
        let s2 = lights_for_feature_def("pilha_crystal_blue2")[0].radius;
        let s3 = lights_for_feature_def("pilha_crystal_blue3")[0].radius;
        assert!(s1 < s2 && s2 < s3);
    }
}

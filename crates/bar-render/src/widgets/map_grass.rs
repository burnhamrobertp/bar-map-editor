//! Map grass widget: instanced rendering of animated grass blades.
//!
//! Visible in-game via BAR's `map_grass_gl4` LuaUI widget
//! (`bar-game/luaui/Widgets/map_grass_gl4.lua`). The widget reads
//! `mapinfo.custom.grassConfig`, samples a per-map distribution mask
//! to pick patch positions, instances grass-blade quads at those
//! positions, and animates them via a wind-perturbation noise
//! texture.
//!
//! The configuration half lives here. The rendering half (instance
//! generation, GPU pipeline, vertex / fragment shaders) is **not yet
//! implemented** -- see `docs/recoil-shader-ports.md` for the
//! widget-port status. Landing the renderer is the follow-up to this
//! foundation commit; the data-flow is wired up so the next commit
//! can plug in instances + draw calls without touching the recipe or
//! mapinfo paths.
//!
//! Shader half (when it lands): `shaders/widgets/map_grass_vs.wgsl`
//! plus `shaders/widgets/map_grass_fs.wgsl`. Separate files because a
//! widget that has its own render pipeline needs both vertex and
//! fragment entry points, unlike `custom_fog` which is just a helper
//! function concatenated into the terrain composer.

use bar_project::MapSettings;

/// Resolved configuration for the grass widget. `enabled = false`
/// when the map has no `mapinfo.custom.grassConfig` block or it
/// lacks the required `grassDistTGA` distribution mask -- the
/// renderer skips the grass pass entirely in that case.
#[derive(Debug, Clone, PartialEq)]
pub struct MapGrassWidget {
    /// True iff the recipe has a grass-distribution path AND a blade
    /// colour texture. Both are required for the widget to render
    /// anything visible. Mirrors the BAR widget's own
    /// "early-out if `grassDistTGA` is empty" gate
    /// (`map_grass_gl4.lua:117`).
    pub enabled: bool,
    /// Distribution mask filename (relative to the map archive).
    /// The widget reads this at load time; non-zero texels seed
    /// grass-blade instances at the corresponding world positions.
    pub dist_tga: String,
    /// Blade-color texture filename. Sampled by the fragment shader.
    pub blade_color_tex: String,
    /// Maximum blade size for a distribution-mask byte of 254.
    /// Linearly interpolated against `min_size` based on the byte
    /// value (per the widget's `byteToSize` helper).
    pub max_size: f32,
    pub min_size: f32,
    /// Patch grid resolution in elmos. Spacing between candidate
    /// blade positions before jitter.
    pub patch_resolution: u32,
    /// Per-patch random XZ offset (fraction of `patch_resolution`).
    pub patch_placement_jitter: f32,
    /// `grassShaderParams.MAPCOLORFACTOR` -- multiplicative blend
    /// strength between blade colour and terrain albedo.
    pub map_color_factor: f32,
    /// `grassShaderParams.MAPCOLORBASE` -- additional albedo blend
    /// at the blade base (creates a smooth transition where the
    /// blade meets the terrain).
    pub map_color_base: f32,
}

impl Default for MapGrassWidget {
    fn default() -> Self {
        Self {
            enabled: false,
            dist_tga: String::new(),
            blade_color_tex: String::new(),
            max_size: 1.7,
            min_size: 0.4,
            patch_resolution: 32,
            patch_placement_jitter: 0.66,
            map_color_factor: 0.6,
            map_color_base: 1.0,
        }
    }
}

impl MapGrassWidget {
    /// Build from a recipe's `MapSettings.custom_grass` block. The
    /// `enabled` flag follows the BAR widget's own gate -- a grass
    /// configuration with no distribution-mask path produces a
    /// disabled widget (renderer never spawns the grass pass).
    pub fn from_settings(ms: &MapSettings) -> Self {
        let g = &ms.custom_grass;
        let enabled = !g.dist_tga.is_empty() && !g.blade_color_tex.is_empty();
        Self {
            enabled,
            dist_tga: g.dist_tga.clone(),
            blade_color_tex: g.blade_color_tex.clone(),
            max_size: g.max_size,
            min_size: g.min_size,
            patch_resolution: g.patch_resolution,
            patch_placement_jitter: g.patch_placement_jitter,
            map_color_factor: g.map_color_factor,
            map_color_base: g.map_color_base,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_project::recipe::CustomGrassSettings;

    #[test]
    fn default_is_disabled() {
        let w = MapGrassWidget::default();
        assert!(!w.enabled);
    }

    #[test]
    fn missing_dist_tga_disables_widget() {
        // BAR's widget mirrors this: no distribution mask -> no
        // patches to spawn from, so the whole pass is dead. We
        // surface that as `enabled = false` so the renderer skips
        // the grass draw call entirely.
        let ms = MapSettings {
            custom_grass: CustomGrassSettings {
                blade_color_tex: "maps/blades.dds".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let w = MapGrassWidget::from_settings(&ms);
        assert!(!w.enabled);
    }

    #[test]
    fn missing_blade_color_tex_disables_widget() {
        let ms = MapSettings {
            custom_grass: CustomGrassSettings {
                dist_tga: "maps/dist.tga".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let w = MapGrassWidget::from_settings(&ms);
        assert!(!w.enabled);
    }

    #[test]
    fn full_config_enables_and_round_trips() {
        let ms = MapSettings {
            custom_grass: CustomGrassSettings {
                dist_tga: "maps/dist.tga".to_string(),
                blade_color_tex: "maps/blades.dds".to_string(),
                max_size: 2.0,
                min_size: 0.5,
                patch_resolution: 16,
                patch_placement_jitter: 0.4,
                map_color_factor: 0.2,
                map_color_base: 0.6,
            },
            ..Default::default()
        };
        let w = MapGrassWidget::from_settings(&ms);
        assert!(w.enabled);
        assert_eq!(w.dist_tga, "maps/dist.tga");
        assert_eq!(w.blade_color_tex, "maps/blades.dds");
        assert!((w.max_size - 2.0).abs() < 1e-6);
        assert!((w.map_color_factor - 0.2).abs() < 1e-6);
    }
}

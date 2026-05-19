//! Custom fog widget: per-map height-based tint driven by BAR's
//! `mapinfo.custom.fog = { color, height, fogatten }` block.
//!
//! Visible effect: fragments below `height` are tinted toward
//! `color`, with strength `clamp((height - y) * fogatten, 0, 1)`.
//! Applied as a final post-pass in the terrain composer shader
//! after engine lighting + emission stages.
//!
//! Shader half: `shaders/widgets/custom_fog.wgsl` (the
//! `apply_custom_fog(color, world_pos)` function).
//!
//! The uniform slots packed by `pack_color_atten` and `pack_params_xy`
//! must match what the shader reads from `camera.custom_fog_color_atten`
//! and `camera.custom_fog_params.xy`. The `.zw` lanes of the params
//! slot are shared with other widget gates (grass-shading tex,
//! light-emission tex) and are not owned by this widget.

use bar_project::MapSettings;

/// State for the `custom.fog` widget. Defaults are "disabled / no
/// tint" so a recipe with no `custom.fog` block produces a no-op
/// uniform packing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CustomFogWidget {
    /// `mapinfo.custom.fog.enabled`. When false the shader bypasses
    /// the apply-fog stage entirely.
    pub enabled: bool,
    /// Multiplicative tint colour applied within the fog band.
    /// Source: `mapinfo.custom.fog.color`.
    pub color: [f32; 3],
    /// Ceiling height in elmos. Fragments with `world_y > height`
    /// receive no fog. Source: `mapinfo.custom.fog.height_elmos`
    /// (the importer resolves mapinfo's `"40%"` syntax against the
    /// map's max height before storing here).
    pub height_elmos: f32,
    /// Attenuation rate per elmo below the ceiling. Source:
    /// `mapinfo.custom.fog.atten`.
    pub atten: f32,
}

impl Default for CustomFogWidget {
    fn default() -> Self {
        Self {
            enabled: false,
            color: [0.0, 0.0, 0.0],
            height_elmos: 0.0,
            atten: 0.0,
        }
    }
}

impl CustomFogWidget {
    /// Build from a recipe's `MapSettings.custom_fog` block. Pure
    /// field copy -- the importer has already resolved the height
    /// syntax and clamped values.
    pub fn from_settings(ms: &MapSettings) -> Self {
        Self {
            enabled: ms.custom_fog.enabled,
            color: ms.custom_fog.color,
            height_elmos: ms.custom_fog.height_elmos,
            atten: ms.custom_fog.atten,
        }
    }

    /// Pack the `(color.r, color.g, color.b, atten)` quad. Matches
    /// `camera.custom_fog_color_atten` in `terrain.wgsl`.
    pub fn pack_color_atten(&self) -> [f32; 4] {
        [self.color[0], self.color[1], self.color[2], self.atten]
    }

    /// Pack the (enabled, height_elmos) pair for the .xy lanes of
    /// the shared widget-gates uniform slot.
    pub fn pack_params_xy(&self) -> [f32; 2] {
        [if self.enabled { 1.0 } else { 0.0 }, self.height_elmos]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_packs_to_no_op() {
        let w = CustomFogWidget::default();
        assert_eq!(w.pack_color_atten(), [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(w.pack_params_xy(), [0.0, 0.0]);
    }

    #[test]
    fn enabled_pack_round_trip() {
        let w = CustomFogWidget {
            enabled: true,
            color: [0.1, 0.2, 0.3],
            height_elmos: 42.0,
            atten: 0.05,
        };
        assert_eq!(w.pack_color_atten(), [0.1, 0.2, 0.3, 0.05]);
        assert_eq!(w.pack_params_xy(), [1.0, 42.0]);
    }
}

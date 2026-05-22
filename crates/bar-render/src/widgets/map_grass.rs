//! Map grass widget: instanced rendering of animated grass blades.
//!
//! Visible in-game via BAR's `map_grass_gl4` LuaUI widget
//! (`bar-game/luaui/Widgets/map_grass_gl4.lua`). The widget reads
//! `mapinfo.custom.grassConfig`, samples a per-map distribution mask
//! to pick patch positions, instances grass-blade quads at those
//! positions, and animates them via a wind-perturbation noise
//! texture.
//!
//! This module exposes three things:
//! - `MapGrassWidget`: per-map config (`from_settings`).
//! - `generate_instances`: CPU scan of the distribution mask
//!   producing per-blade transforms.
//! - `MapGrassPipeline`: GPU resources (pipeline, blade mesh,
//!   instance buffer, blade-colour texture, bind group). Owned by
//!   `TerrainRenderer`; render integration in
//!   `renderer.rs::render_internal`.
//!
//! Shader half: `shaders/widgets/map_grass_vs.wgsl` plus
//! `shaders/widgets/map_grass_fs.wgsl`. Separate files because a
//! widget with its own render pipeline needs both vertex and
//! fragment entry points; the runtime concats them at pipeline-
//! build time into a single WGSL module per stage.

use crate::samplers::make_filtered_sampler;
use bar_project::MapSettings;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// Engine widget's hard cap on per-axis wind speed
/// (`map_grass_gl4.lua:115`, `maxWindSpeed = 20`). Spring wind can
/// blow harder than this; the engine widget caps the dominant axis at
/// `maxWindSpeed` and scales the other axis proportionally to
/// preserve direction (see `map_grass_gl4.lua:1149-1156`). Replicating
/// the proportional cap rather than a per-axis clamp avoids rotating
/// the apparent wind direction at high speed.
const ENGINE_MAX_WIND_SPEED: f32 = 20.0;
/// Engine widget's floor on the magnitude scalar that drives
/// per-blade sway amplitude (`map_grass_gl4.lua:1288`,
/// `mathMax(4.0, |wx| + |wz|)`). Below this floor calm wind would
/// freeze grass entirely; the floor guarantees a minimal idle sway
/// that matches in-engine "no wind" appearance.
const ENGINE_WIND_STRENGTH_FLOOR: f32 = 4.0;
/// User-config persisted by BAR's map-grass widget; the widget source
/// defaults to 0.45 but installs commonly land on 0.4 (observed in
/// runtime debug logs). Multiplies BOTH fade endpoints (vert.glsl
/// :201). The value can be exposed through editor settings later;
/// for now it lives as a constant matching the widget source default.
const ENGINE_GRASS_DISTANCE_MULT: f32 = 0.45;
/// Direction unit vector used to seed the wind drift accumulator in
/// editor preview. BAR's `Spring.GetWind` returns a randomised vector
/// that oscillates per game frame; BME has no equivalent source, so
/// we synthesise a steady-state cardinal direction that produces the
/// same visual character. Roughly east-by-south, picked to look
/// "natural" against typical maps.
const DEFAULT_WIND_DIR: [f32; 2] = [0.857, 0.515];

/// Apply BAR's proportional wind cap. Returns `(capped_x, capped_z)`
/// such that whichever input axis was dominant is clamped to
/// `max_speed`, and the other axis is scaled by the same ratio so the
/// vector's *direction* is preserved. Mirrors
/// `map_grass_gl4.lua:1149-1156` verbatim.
pub fn cap_wind_proportional(x: f32, z: f32, max_speed: f32) -> (f32, f32) {
    let ax = x.abs();
    let az = z.abs();
    if ax > max_speed && ax >= az {
        let scaled_z = (z / ax) * max_speed;
        (x.signum() * max_speed, scaled_z)
    } else if az > max_speed && az > ax {
        let scaled_x = (x / az) * max_speed;
        (scaled_x, z.signum() * max_speed)
    } else {
        (x, z)
    }
}

#[cfg(test)]
mod cap_wind_tests {
    use super::cap_wind_proportional;
    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-4, "expected {b}, got {a}");
    }
    #[test]
    fn passes_through_below_cap() {
        let (x, z) = cap_wind_proportional(5.0, 3.0, 20.0);
        approx(x, 5.0);
        approx(z, 3.0);
    }
    #[test]
    fn clamps_dominant_x_and_scales_z() {
        let (x, z) = cap_wind_proportional(30.0, 6.0, 20.0);
        approx(x, 20.0);
        approx(z, 4.0); // 6 * (20/30)
    }
    #[test]
    fn clamps_dominant_z_and_scales_x() {
        let (x, z) = cap_wind_proportional(5.0, 50.0, 20.0);
        approx(x, 2.0); // 5 * (20/50)
        approx(z, 20.0);
    }
    #[test]
    fn preserves_signs() {
        let (x, z) = cap_wind_proportional(-30.0, -6.0, 20.0);
        approx(x, -20.0);
        approx(z, -4.0);
    }
}

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
    /// `grassShaderParams.MAPCOLORFACTOR`.
    pub map_color_factor: f32,
    /// `grassShaderParams.MAPCOLORBASE`.
    pub map_color_base: f32,
    /// `grassShaderParams.ALPHATHRESHOLD`.
    pub alpha_threshold: f32,
    /// `grassShaderParams.SHADOWFACTOR`.
    pub shadow_factor: f32,
    /// `grassShaderParams.GRASSBRIGHTNESS`.
    pub grass_brightness: f32,
    /// `grassShaderParams.FADESTART` in elmos.
    pub fade_start: f32,
    /// `grassShaderParams.FADEEND` in elmos.
    pub fade_end: f32,
    /// `grassShaderParams.WINDSTRENGTH`.
    pub wind_strength: f32,
    /// `grassShaderParams.WINDSCALE` -- noise drift rate.
    pub wind_scale: f32,
    /// `grassShaderParams.WINDSAMPLESCALE` -- noise sample tiling.
    pub wind_sample_scale: f32,
    /// `grassWindMult` -- drift advance magnitude.
    pub grass_wind_mult: f32,
}

impl Default for MapGrassWidget {
    fn default() -> Self {
        // Defaults verbatim from `map_grass_gl4.lua:87-110`.
        Self {
            enabled: false,
            dist_tga: String::new(),
            blade_color_tex: String::new(),
            max_size: 1.7,
            min_size: 0.3,
            patch_resolution: 32,
            patch_placement_jitter: 0.66,
            map_color_factor: 0.6,
            map_color_base: 1.0,
            alpha_threshold: 0.01,
            shadow_factor: 0.25,
            grass_brightness: 1.0,
            fade_start: 5000.0,
            fade_end: 8000.0,
            wind_strength: 0.1,
            wind_scale: 0.33,
            wind_sample_scale: 0.001,
            grass_wind_mult: 4.5,
        }
    }
}

impl MapGrassWidget {
    /// Build from a recipe's `MapSettings.custom_grass` block. The
    /// `enabled` flag follows the BAR widget's gate -- a grass
    /// configuration with no distribution-mask path produces a
    /// disabled widget (renderer never spawns the grass pass).
    ///
    /// At import time, `bar-engine`'s `import_sd7_to_project`
    /// auto-populates `custom_grass.dist_tga = "grassmap.png"` when
    /// the SMF carries a `MEH_Vegetation` extra header, so maps that
    /// rely on the engine widget's `Spring.GetGrass` fallback
    /// (`map_grass_gl4.lua:856-892`) work transparently without
    /// extra wiring on this side.
    pub fn from_settings(ms: &MapSettings) -> Self {
        let g = ms.custom_grass.resolved();
        let enabled = !g.dist_tga.is_empty() && !g.blade_color_tex.is_empty();
        Self {
            enabled,
            dist_tga: g.dist_tga,
            blade_color_tex: g.blade_color_tex,
            max_size: g.max_size,
            min_size: g.min_size,
            patch_resolution: g.patch_resolution,
            patch_placement_jitter: g.patch_placement_jitter,
            map_color_factor: g.map_color_factor,
            map_color_base: g.map_color_base,
            alpha_threshold: g.alpha_threshold,
            shadow_factor: g.shadow_factor,
            grass_brightness: g.grass_brightness,
            fade_start: g.fade_start,
            fade_end: g.fade_end,
            wind_strength: g.wind_strength,
            wind_sample_scale: g.wind_sample_scale,
            wind_scale: g.wind_scale,
            grass_wind_mult: g.grass_wind_mult,
        }
    }
}

/// Per-blade instance data uploaded to the grass pipeline's instance
/// buffer. Layout mirrors BAR widget's `instancePosRotSize` vertex
/// attribute (`map_grass_gl4.vert.glsl:24`).
///
/// XZ positions are in **render space** (generated against the
/// playable half-extents on the CPU). `size` is in **elmos**, raw
/// from mapinfo `grassMaxSize`: the vertex shader multiplies the
/// elmo-authored mesh by it and then applies `elmo_to_render` (read
/// from the camera uniform, always current) at the view-proj
/// boundary. Putting the conversion in the shader rather than on
/// the CPU avoids a stale instance buffer when `update_heightmap`
/// arrives after this gets generated -- same pattern as splat-
/// detail sampling and custom_fog.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GrassInstance {
    /// World X position, render space (`[-x_extent, +x_extent]`).
    pub world_x: f32,
    /// Random Y rotation in radians.
    pub rotation: f32,
    /// World Z position, render space.
    pub world_z: f32,
    /// Blade size in elmos (mapinfo `grassMaxSize` scaled by the
    /// distribution-mask byte). Converted to render units in the
    /// vertex shader via `camera.height_scale /
    /// camera.height_range_elmos`.
    pub size: f32,
}

/// Static blade mesh vertex layout. Two crossed quads in object
/// space (origin at blade base). The vertex shader scales by
/// instance.size and rotates around Y, then anchors to the
/// heightmap at the instance's world XZ.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BladeVertex {
    pos: [f32; 3],
    uv: [f32; 2],
}

/// Mesh vertex positions in **elmo units**, copied verbatim from
/// BAR's `bar-game/luaui/Include/grassPatches.lua` (the
/// `geometrydata[4]` table -- `patchSize = 4` is the engine widget's
/// default per `map_grass_gl4.lua:90`). 144 vertices arranged as a
/// triangle list: 48 triangles = 24 quads = 12 crossed double-quads,
/// each oriented at a different angle to form a 6-blade cluster
/// patch. Bounding box is approximately:
///   X in [-18, +19] elmos
///   Y in [-0.0012, +9.04] elmos
///   Z in [-19, +18] elmos
///
/// Per-instance `size` is `mapinfo.grassMaxSize` (default 1.7) --
/// scales the patch to ~15 elmos tall × ~63 elmos wide. The pipeline
/// renders these with no index buffer (data is already a flat
/// triangle list).
///
/// Each face uses the full blade-colour texture UV space; the
/// silhouettes are baked into the texture's alpha (the BC3 alpha
/// block carries the cutouts, decoded by `bar-data/src/skybox.rs`).
const BLADE_VERTICES: &[BladeVertex] = &[
    BladeVertex {
        pos: [-14.8090, -0.0012, 1.0914],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-5.5186, -0.0012, -17.9567],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [-8.6393, 9.0392, -19.4787],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-8.6393, 9.0392, -19.4787],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-17.9296, 9.0392, -0.4307],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [-14.8090, -0.0012, 1.0914],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-5.6729, -0.0012, -18.0320],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-14.9633, -0.0012, 1.0161],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [-11.8427, 9.0392, 2.5381],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-11.8427, 9.0392, 2.5381],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-2.5523, 9.0392, -16.5100],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [-5.6729, -0.0012, -18.0320],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-17.7854, -0.0012, -3.4063],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [3.3559, -0.0012, -4.8847],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [3.1137, 9.0392, -8.3482],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [3.1137, 9.0392, -8.3482],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-18.0276, 9.0392, -6.8699],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [-17.7854, -0.0012, -3.4063],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [3.3439, -0.0012, -5.0559],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-17.7974, -0.0012, -3.5776],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [-17.5552, 9.0392, -0.1140],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-17.5552, 9.0392, -0.1140],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [3.5861, 9.0392, -1.5924],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [3.3439, -0.0012, -5.0559],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-11.1297, -0.0012, -17.9539],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [0.7212, -0.0012, -0.3842],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [3.5996, 9.0392, -2.3257],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [3.5996, 9.0392, -2.3257],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-8.2513, 9.0392, -19.8954],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [-11.1297, -0.0012, -17.9539],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [0.8635, -0.0012, -0.4802],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-10.9874, -0.0012, -18.0499],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [-13.8658, 9.0392, -16.1084],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-13.8658, 9.0392, -16.1084],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-2.0149, 9.0392, 1.4613],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [0.8635, -0.0012, -0.4802],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [3.6502, -0.0012, 12.7368],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-17.2208, -0.0012, 9.0567],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [-17.8237, 9.0392, 12.4759],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-17.8237, 9.0392, 12.4759],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [3.0473, 9.0392, 16.1560],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [3.6502, -0.0012, 12.7368],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-17.2506, -0.0012, 9.2257],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [3.6204, -0.0012, 12.9058],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [4.2233, 9.0392, 9.4866],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [4.2233, 9.0392, 9.4866],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-16.6477, 9.0392, 5.8065],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [-17.2506, -0.0012, 9.2257],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [0.1471, -0.0012, 16.8376],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-7.1013, -0.0012, -3.0772],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [-10.3639, 9.0392, -1.8897],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-10.3639, 9.0392, -1.8897],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-3.1155, 9.0392, 18.0251],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [0.1471, -0.0012, 16.8376],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-7.2626, -0.0012, -3.0185],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-0.0142, -0.0012, 16.8963],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [3.2484, 9.0392, 15.7088],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [3.2484, 9.0392, 15.7088],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-4.0000, 9.0392, -4.2060],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [-7.2626, -0.0012, -3.0185],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-15.6714, -0.0012, 14.4496],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-2.0489, -0.0012, -1.7851],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [-4.7086, 9.0392, -4.0168],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-4.7086, 9.0392, -4.0168],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-18.3311, 9.0392, 12.2179],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [-15.6714, -0.0012, 14.4496],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-2.1804, -0.0012, -1.8954],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-15.8029, -0.0012, 14.3393],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [-13.1432, 9.0392, 16.5711],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-13.1432, 9.0392, 16.5711],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [0.4793, 9.0392, 0.3363],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [-2.1804, -0.0012, -1.8954],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [14.9866, -0.0012, -17.5038],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [5.0372, -0.0012, 1.2084],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [8.1028, 9.0392, 2.8384],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [8.1028, 9.0392, 2.8384],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [18.0522, 9.0392, -15.8738],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [14.9866, -0.0012, -17.5038],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [5.1887, -0.0012, 1.2890],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [15.1382, -0.0012, -17.4233],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [12.0726, 9.0392, -19.0533],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [12.0726, 9.0392, -19.0533],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [2.1231, 9.0392, -0.3410],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [5.1887, -0.0012, 1.2890],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [17.8042, -0.0012, -12.9050],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-3.3758, -0.0012, -12.1654],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [-3.2546, 9.0392, -8.6955],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-3.2546, 9.0392, -8.6955],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [17.9254, 9.0392, -9.4351],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [17.8042, -0.0012, -12.9050],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-3.3698, -0.0012, -11.9938],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [17.8102, -0.0012, -12.7335],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [17.6891, 9.0392, -16.2033],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [17.6891, 9.0392, -16.2033],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-3.4909, 9.0392, -15.4637],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [-3.3698, -0.0012, -11.9938],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [10.6450, -0.0012, 1.4014],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-0.5856, -0.0012, -16.5712],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [-3.5300, 9.0392, -14.7313],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-3.5300, 9.0392, -14.7313],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [7.7005, 9.0392, 3.2413],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [10.6450, -0.0012, 1.4014],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-0.7312, -0.0012, -16.4802],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [10.4994, -0.0012, 1.4924],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [13.4438, 9.0392, -0.3475],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [13.4438, 9.0392, -0.3475],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [2.2133, 9.0392, -18.3201],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [-0.7312, -0.0012, -16.4802],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [14.4134, -0.0012, 16.0154],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-0.3084, -0.0012, 0.7705],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [-2.8060, 9.0392, 3.1823],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-2.8060, 9.0392, 3.1823],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [11.9158, 9.0392, 18.4272],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [14.4134, -0.0012, 16.0154],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-0.4319, -0.0012, 0.8897],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [14.2899, -0.0012, 16.1346],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [16.7875, 9.0392, 13.7228],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [16.7875, 9.0392, 13.7228],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [2.0656, 9.0392, -1.5221],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [-0.4319, -0.0012, 0.8897],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [9.1690, -0.0012, 17.2740],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [15.0105, -0.0012, -3.0980],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [11.6730, 9.0392, -4.0550],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [11.6730, 9.0392, -4.0550],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [5.8315, 9.0392, 16.3170],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [9.1690, -0.0012, 17.2740],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [14.8455, -0.0012, -3.1453],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [9.0039, -0.0012, 17.2267],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [12.3414, 9.0392, 18.1837],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [12.3414, 9.0392, 18.1837],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [18.1830, 9.0392, -2.1883],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [14.8455, -0.0012, -3.1453],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-2.2249, -0.0012, 6.0441],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [18.3385, -0.0012, 0.9171],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [17.4985, 9.0392, -2.4517],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [17.4985, 9.0392, -2.4517],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-3.0649, 9.0392, 2.6753],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [-2.2249, -0.0012, 6.0441],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [18.2970, -0.0012, 0.7505],
        uv: [0.9984, 0.0028],
    },
    BladeVertex {
        pos: [-2.2665, -0.0012, 5.8776],
        uv: [0.0019, 0.0028],
    },
    BladeVertex {
        pos: [-1.4265, 9.0392, 9.2464],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [-1.4265, 9.0392, 9.2464],
        uv: [0.0019, 0.9976],
    },
    BladeVertex {
        pos: [19.1369, 9.0392, 4.1194],
        uv: [0.9984, 0.9976],
    },
    BladeVertex {
        pos: [18.2970, -0.0012, 0.7505],
        uv: [0.9984, 0.0028],
    },
];

// All tuning constants now live on `MapGrassWidget` (parsed from
// the map's `grassShaderParams` block in `mapinfo.lua`) and flow
// into the shader through the params buffer. Defaults match the
// BAR widget's hardcoded defaults in `map_grass_gl4.lua:87-110`.

/// GPU-resident grass renderer. Owned by `TerrainRenderer`; lives
/// across map switches (the pipeline is static, only the instance
/// buffer + blade-colour texture get replaced when a new map's
/// `mapinfo.custom.grassConfig` lands).
pub struct MapGrassPipeline {
    pipeline: wgpu::RenderPipeline,
    blade_bgl: wgpu::BindGroupLayout,
    /// Static patch mesh (144 vertices, triangle list -- BAR's
    /// `geometrydata[4]` from `grassPatches.lua`). Allocated once
    /// at construction; the instance buffer is what changes per
    /// map. No index buffer: the source data is already a flat
    /// triangle list with redundant shared vertices.
    vertex_buffer: wgpu::Buffer,
    /// Per-pipeline tuning constants (wind strength, blend factors,
    /// fade distance).
    params_buffer: wgpu::Buffer,
    /// Default 1x1 blade-color texture. Retained for the lifetime
    /// of the pipeline so `clear_blade_color` can reset to it
    /// cheaply, and so it stays alive even if `blade_color_texture`
    /// is swapped out mid-frame.
    #[allow(dead_code)]
    blade_color_default: wgpu::Texture,
    blade_color_texture: wgpu::Texture,
    blade_color_sampler: wgpu::Sampler,
    /// BAR's `grassWindPerturbTex` -- bundled perlin noise sampled by
    /// the VS for wind perturbation + shadeamount.
    perlin_texture: wgpu::Texture,
    perlin_sampler: wgpu::Sampler,
    /// Bind group for the grass-specific resources (blade tex,
    /// heightmap, params uniform, grass color mod, perlin noise).
    /// Rebuilt whenever the blade or grass-shading texture changes.
    bind_group: Option<wgpu::BindGroup>,
    /// Per-map instance buffer. None until the distribution mask
    /// has been processed via `update_instances`.
    instance_buffer: Option<wgpu::Buffer>,
    instance_count: u32,
    /// Cached widget config; carries the per-map shader-blend
    /// factors that get re-packed into `params_buffer` after any
    /// recipe change.
    widget: MapGrassWidget,
    /// Cached distribution-mask data plus the playable extents used
    /// when it was last generated. Held so `set_config` can detect
    /// changes to fields that affect the GPU instance buffer
    /// (`grassMinSize`, `grassMaxSize`, `patchPlacementJitter`) and
    /// regenerate the buffer live without round-tripping through the
    /// async asset loader. `None` until the first successful
    /// `sync_grass_assets` populates it.
    cached_mask: Option<CachedMask>,
    /// Running wind-drift accumulator, mirrors the engine widget's
    /// `(offsetX, offsetZ)` integral. Advances every `tick` call by
    /// `wind_dir_capped * grassWindMult * dt`. NOT derived from
    /// `camera.time` -- the engine widget pauses this on game pause
    /// (Lua `if not isPaused then ...`), so deriving from a steady
    /// engine clock would diverge from in-engine appearance on every
    /// pause.
    drift_offset: [f32; 2],
    /// Wallclock of the previous `tick` call. `None` before the first
    /// call; after that, each tick computes its own `dt` from this so
    /// callers don't have to track a delta.
    last_tick_at: Option<std::time::Instant>,
}

/// Snapshot of everything `generate_instances` needs to rebuild the
/// per-patch GPU buffer from scratch.
struct CachedMask {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    x_extent: f32,
    z_extent: f32,
}

impl MapGrassPipeline {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera_bgl: &wgpu::BindGroupLayout,
        shadow_receiver_bgl: &wgpu::BindGroupLayout,
        output_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let vs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("map_grass_vs"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../../shaders/widgets/map_grass_vs.wgsl").into(),
            ),
        });
        let fs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("map_grass_fs"),
            source: wgpu::ShaderSource::Wgsl({
                // Concat the VS source again so the FS module sees
                // the shared `blade_color_tex` / `blade_color_sam`
                // bindings declared in the VS file. WGSL modules
                // are single-translation-units; the simplest way to
                // share bindings between two entry points in
                // wgpu is to compile them as one module each, both
                // including the binding declarations.
                let vs_src = include_str!("../../../../shaders/widgets/map_grass_vs.wgsl");
                let fs_src = include_str!("../../../../shaders/widgets/map_grass_fs.wgsl");
                format!("{vs_src}\n{fs_src}").into()
            }),
        });

        let blade_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map_grass_bgl"),
            entries: &[
                // blade color texture
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // heightmap (non-filterable, R32Float).
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // grass params uniform.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // `mapGrassColorModTex` -- BAR's `$grass` token, which
                // the engine resolves to `grassShadingTex` (per-map
                // override) or the minimap as a fallback
                // (`SMFReadMap.cpp:313`). NOT the high-resolution
                // terrain albedo: sampling the albedo over-saturates
                // the `* 2.0` multiplicative blend in the fragment
                // shader because tile detail bleeds into a path that
                // assumes a downgraded colour map.
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // `grassWindPerturbTex` -- BAR's bundled perlin noise
                // (`bitmaps/gpl/perlin_noise.jpg`). Read by the VS for
                // wind-noise perturbation and shadeamount. Embedded
                // at build time so we don't depend on a local BAR
                // install for wind animation.
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("map_grass_pipeline_layout"),
            bind_group_layouts: &[
                camera_bgl,          // group 0: camera + skybox
                &blade_bgl,          // group 1: blade tex + heightmap + params + albedo
                shadow_receiver_bgl, // group 2: shadow light_vp + tex + comparison sampler
            ],
            push_constant_ranges: &[],
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("map_grass_vertices"),
            contents: bytemuck::cast_slice(BLADE_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Five vec4s = 80 bytes. Slots 0-2 carry the per-map static
        // tuning copied from `MapGrassWidget` by `set_config`. Slots
        // 3-4 are runtime/diagnostic state written separately:
        //   slot 3 `dynamic` = (drift_x, drift_z, effective_wind_strength,
        //         distance_mult). Refreshed every frame via `tick`.
        //   slot 4 `dbg`     = (grass_debug_output, alpha_test_mode,
        //         _pad, _pad). Refreshed when the user toggles a
        //         diagnostic from the viewport gear menu.
        // Defaults below match the BAR widget defaults so an
        // enabled-but-unconfigured map renders the engine-stock look.
        let params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("map_grass_params"),
            size: 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let default_params: [f32; 20] = [
            0.6,
            1.0,
            0.1,
            1.0, // MAPCOLORFACTOR, MAPCOLORBASE, WINDSTRENGTH, GRASSBRIGHTNESS
            5000.0,
            8000.0,
            0.25,
            0.01, // FADESTART, FADEEND, SHADOWFACTOR, ALPHATHRESHOLD
            0.33,
            0.001,
            4.5,
            0.0, // WINDSCALE, WINDSAMPLESCALE, grassWindMult, unused
            0.0,
            0.0,
            ENGINE_WIND_STRENGTH_FLOOR,
            ENGINE_GRASS_DISTANCE_MULT,
            // ^ dynamic: (drift_x, drift_z, effective_wind_strength, distance_mult)
            0.0,
            0.0,
            0.0,
            0.0, // dbg: (grass_debug_output, alpha_test_mode, _pad, _pad)
        ];
        queue.write_buffer(&params_buffer, 0, bytemuck::bytes_of(&default_params));

        // 1x1 white default blade-colour texture. Inert until the
        // map upload replaces it -- with `enabled = false` on the
        // widget config the renderer doesn't draw anyway.
        let default_pixel: [u8; 4] = [255, 255, 255, 255];
        let blade_color_default = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("map_grass_blade_color_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &default_pixel,
        );
        // The active texture starts as a clone of the default.
        let blade_color_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("map_grass_blade_color"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &default_pixel,
        );
        let blade_color_sampler =
            make_filtered_sampler(device, "map_grass_blade_sampler", wgpu::AddressMode::Repeat);

        // BAR's `grassWindPerturbTex` -- `bitmaps/gpl/perlin_noise.jpg`
        // from the bar-game archive, bundled into BME at build time
        // so wind animation doesn't require a local BAR install.
        // Engine widget samples this in the VS at
        // `(grassVertWorldPos.xz + drift) * WINDSAMPLESCALE` with
        // Repeat addressing -- the perlin tile wraps endlessly across
        // the map. JPEG decodes to RGB; we synthesise an opaque alpha
        // and upload as RGBA8 to match the bind-group layout.
        let perlin_bytes = include_bytes!("../../../../assets/perlin_noise.jpg");
        let perlin_img = image::load_from_memory(perlin_bytes)
            .expect("bundled perlin_noise.jpg must decode")
            .to_rgba8();
        let perlin_w = perlin_img.width();
        let perlin_h = perlin_img.height();
        let perlin_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("map_grass_wind_perturb_tex"),
                size: wgpu::Extent3d {
                    width: perlin_w,
                    height: perlin_h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &perlin_img.into_raw(),
        );
        let perlin_sampler = make_filtered_sampler(
            device,
            "map_grass_wind_perturb_sampler",
            wgpu::AddressMode::Repeat,
        );

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("map_grass_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vs,
                entry_point: Some("vs_grass"),
                compilation_options: Default::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<BladeVertex>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            wgpu::VertexAttribute {
                                offset: 0,
                                shader_location: 0,
                                format: wgpu::VertexFormat::Float32x3,
                            },
                            wgpu::VertexAttribute {
                                offset: 12,
                                shader_location: 1,
                                format: wgpu::VertexFormat::Float32x2,
                            },
                        ],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<GrassInstance>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32x4,
                        }],
                    },
                ],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // `map_grass_gl4.lua:1271` -- engine widget explicitly
                // enables back-face culling. The 144-vertex patch mesh
                // (12 cross-quad pairs at different rotations) is
                // authored with consistent CCW winding so each blade's
                // front side faces outward; back-cull halves the
                // fragment cost and prevents the doubled-blade
                // appearance that came from drawing both sides.
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                // `map_grass_gl4.lua:1270` -- engine widget writes
                // depth. Hashed alpha test produces fully opaque
                // kept fragments, so they should occlude geometry
                // drawn afterwards (matches the engine widget which
                // draws into the world depth buffer like any opaque
                // pass).
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fs,
                entry_point: Some("fs_grass"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            blade_bgl,
            vertex_buffer,
            params_buffer,
            blade_color_default,
            blade_color_texture,
            blade_color_sampler,
            perlin_texture,
            perlin_sampler,
            bind_group: None,
            instance_buffer: None,
            instance_count: 0,
            widget: MapGrassWidget::default(),
            cached_mask: None,
            drift_offset: [0.0, 0.0],
            last_tick_at: None,
        }
    }

    /// Update the per-map widget config + repack the params uniform.
    /// Returns the new `enabled` state so callers can branch on
    /// whether to even bother loading assets.
    ///
    /// `fade_start` / `fade_end` stay in elmos here; the vertex
    /// shader applies the camera-uniform elmo->render factor at the
    /// distance check, matching how it converts the per-instance
    /// size. All other params are dimensionless multipliers.
    pub fn set_config(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        widget: MapGrassWidget,
    ) -> bool {
        let params: [f32; 12] = [
            // blend vec4: MAPCOLORFACTOR, MAPCOLORBASE, WINDSTRENGTH,
            // GRASSBRIGHTNESS
            widget.map_color_factor,
            widget.map_color_base,
            widget.wind_strength,
            widget.grass_brightness,
            // fade vec4: FADESTART, FADEEND, SHADOWFACTOR,
            // ALPHATHRESHOLD
            widget.fade_start,
            widget.fade_end,
            widget.shadow_factor,
            widget.alpha_threshold,
            // wind vec4: WINDSCALE, WINDSAMPLESCALE, grassWindMult,
            // unused
            widget.wind_scale,
            widget.wind_sample_scale,
            widget.grass_wind_mult,
            0.0,
        ];
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));

        // Detect changes to fields that live IN the GPU instance
        // buffer (per-patch position + per-instance size). The blend
        // factors above flow through the uniform every frame, but
        // these three are baked into instances by `generate_instances`,
        // so regenerate the buffer if any of them moved.
        let geometry_changed = self.widget.max_size != widget.max_size
            || self.widget.min_size != widget.min_size
            || self.widget.patch_placement_jitter != widget.patch_placement_jitter
            || self.widget.patch_resolution != widget.patch_resolution;

        let enabled = widget.enabled;
        self.widget = widget;
        if !enabled {
            // Drop any stale instance buffer so a previous map's
            // grass doesn't ghost into the next one.
            self.instance_buffer = None;
            self.instance_count = 0;
            return false;
        }
        if geometry_changed {
            if let Some(cache) = &self.cached_mask {
                let instances = generate_instances(
                    &self.widget,
                    &cache.bytes,
                    cache.width,
                    cache.height,
                    cache.x_extent,
                    cache.z_extent,
                );
                self.update_instances(device, &instances);
            }
        }
        enabled
    }

    /// Store the per-map distribution mask + playable extents so a
    /// later `set_config` can rebuild the instance buffer in place
    /// when geometry-affecting fields change (`grassMin/MaxSize`,
    /// `patchPlacementJitter`, `patchResolution`). Called by the
    /// renderer right after it uploads a fresh instance buffer from
    /// async-loaded grass assets.
    pub fn cache_mask_for_regen(
        &mut self,
        bytes: &[u8],
        width: u32,
        height: u32,
        x_extent: f32,
        z_extent: f32,
    ) {
        self.cached_mask = Some(CachedMask {
            bytes: bytes.to_vec(),
            width,
            height,
            x_extent,
            z_extent,
        });
    }

    /// Drop the cached mask data -- used on `clear_grass_assets` so
    /// stale mask bytes from a previous map don't leak into the next
    /// project's instance regeneration.
    pub fn invalidate_cached_mask(&mut self) {
        self.cached_mask = None;
    }

    /// Replace the blade-colour texture with a freshly-decoded mip
    /// chain. Each entry is `(rgba_bytes, mip_width, mip_height)`;
    /// the first entry is the base mip, subsequent entries are the
    /// pre-built minification chain (DDS files ship this baked in;
    /// non-DDS images get a CPU-generated chain). Without mips,
    /// blades alias heavily at minification distance and look
    /// blocky / "downsampled". The bind group must be rebuilt
    /// afterwards via `rebuild_bind_group`.
    pub fn update_blade_color(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mips: &[(Vec<u8>, u32, u32)],
    ) {
        if mips.is_empty() {
            return;
        }
        let (_, base_w, base_h) = mips[0];
        self.blade_color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("map_grass_blade_color"),
            size: wgpu::Extent3d {
                width: base_w,
                height: base_h,
                depth_or_array_layers: 1,
            },
            mip_level_count: mips.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, (data, w, h)) in mips.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.blade_color_texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(*w * 4),
                    rows_per_image: Some(*h),
                },
                wgpu::Extent3d {
                    width: *w,
                    height: *h,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    /// Reset the blade-colour texture to the inert white default
    /// (e.g. on map switch when no grass widget is configured).
    pub fn clear_blade_color(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let default_pixel: [u8; 4] = [255, 255, 255, 255];
        self.blade_color_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("map_grass_blade_color_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &default_pixel,
        );
    }

    /// Upload (or replace) the instance buffer. An empty `instances`
    /// vector clears the buffer.
    pub fn update_instances(&mut self, device: &wgpu::Device, instances: &[GrassInstance]) {
        if instances.is_empty() {
            self.instance_buffer = None;
            self.instance_count = 0;
            return;
        }
        self.instance_buffer = Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("map_grass_instances"),
                contents: bytemuck::cast_slice(instances),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        );
        self.instance_count = instances.len() as u32;
    }

    /// Set the grass-shader debug output mode. Written into the
    /// `dbg.x` slot of the params uniform. Driven from
    /// `ViewportDebug::grass_debug_output` per-frame; see the FS
    /// comment block above the `debug_mode` read for the meaning of
    /// the integer values.
    pub fn set_debug_output(&self, queue: &wgpu::Queue, mode: i32) {
        // Offset 64 = 16 floats in = `dbg.x` slot.
        let value = mode as f32;
        queue.write_buffer(&self.params_buffer, 64, bytemuck::bytes_of(&value));
    }

    /// Set the alpha-test technique. `0` = hashed (Wronski 2017
    /// stochastic discard, BME default); `1` = binary discard at
    /// `ALPHATHRESHOLD` only (matches the engine widget's discard
    /// gate without the AtoC sub-pixel coverage path BME can't
    /// produce at sample_count=1). Driven from
    /// `ViewportDebug::grass_alpha_test_mode` so the user can A/B
    /// against the in-engine look.
    pub fn set_alpha_test_mode(&self, queue: &wgpu::Queue, mode: u32) {
        // Offset 68 = 17 floats in = `dbg.y` slot.
        let value = mode as f32;
        queue.write_buffer(&self.params_buffer, 68, bytemuck::bytes_of(&value));
    }

    /// Advance the wind-drift accumulator and push the updated
    /// `dynamic` vec4 (drift xy, effective wind strength, distance
    /// mult) to the params buffer. Called per-render-frame by the
    /// host (`crates/bar-app/src/viewport.rs`).
    ///
    /// `dt` is derived from the wallclock interval since the previous
    /// call -- matches BAR Lua widget's `os.clock` integration, NOT
    /// `Spring.GetGameSeconds`. Capped at 0.25s to absorb hitches
    /// (alt-tab pause, GPU stalls) without a giant single-step jump
    /// in the noise pattern.
    ///
    /// `mapinfo_avg_wind` is the `(min_wind + max_wind) / 2` value
    /// from `MapSettings.atmosphere` -- a static stand-in for the
    /// engine's randomised `Spring.GetWind()` since BME has no
    /// equivalent gameplay state.
    pub fn tick(&mut self, queue: &wgpu::Queue, mapinfo_avg_wind: f32) {
        let now = std::time::Instant::now();
        let dt_seconds = match self.last_tick_at {
            Some(prev) => now.duration_since(prev).as_secs_f32().min(0.25),
            None => 0.0,
        };
        self.last_tick_at = Some(now);
        // Project the stand-in wind magnitude onto the editor's
        // fixed default direction, then apply the engine widget's
        // proportional cap.
        let raw_x = DEFAULT_WIND_DIR[0] * mapinfo_avg_wind;
        let raw_z = DEFAULT_WIND_DIR[1] * mapinfo_avg_wind;
        let (wind_x, wind_z) = cap_wind_proportional(raw_x, raw_z, ENGINE_MAX_WIND_SPEED);

        // Engine `grassuniforms.z`: `clamp(|wx|+|wz|, floor, cap)`.
        // Floor matters when wind is calm -- without it the per-blade
        // vertex offset goes to zero and grass freezes (the engine
        // explicitly avoids that with the 4.0 floor).
        let effective_strength =
            (wind_x.abs() + wind_z.abs()).clamp(ENGINE_WIND_STRENGTH_FLOOR, ENGINE_MAX_WIND_SPEED);

        // Integrate the drift accumulator. Engine widget sign:
        // `offsetX -= windDirX * grassWindMult * dt` (`map_grass_gl4
        // .lua:1264`), so positive wind drifts the noise sample
        // position negatively -- gusts blow noise across the world
        // in the wind direction.
        let wind_mult = self.widget.grass_wind_mult;
        self.drift_offset[0] -= wind_x * wind_mult * dt_seconds;
        self.drift_offset[1] -= wind_z * wind_mult * dt_seconds;

        let dynamic: [f32; 4] = [
            self.drift_offset[0],
            self.drift_offset[1],
            effective_strength,
            ENGINE_GRASS_DISTANCE_MULT,
        ];
        // Offset 48 = 12 floats in = start of `dynamic` slot.
        queue.write_buffer(&self.params_buffer, 48, bytemuck::bytes_of(&dynamic));
    }

    /// Rebuild the grass bind group against the current blade-colour
    /// + heightmap views. Called every time either texture changes.
    pub fn rebuild_bind_group(
        &mut self,
        device: &wgpu::Device,
        heightmap_view: &wgpu::TextureView,
        grass_color_mod_view: &wgpu::TextureView,
    ) {
        let blade_view = self
            .blade_color_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let perlin_view = self
            .perlin_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("map_grass_bind_group"),
            layout: &self.blade_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&blade_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.blade_color_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(heightmap_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(grass_color_mod_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&perlin_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&self.perlin_sampler),
                },
            ],
        }));
    }

    /// Whether the renderer should issue the draw call this frame.
    /// True only when: config is enabled, AND we have instances,
    /// AND the bind group has been built (heightmap available).
    pub fn ready_to_draw(&self) -> bool {
        self.widget.enabled
            && self.instance_count > 0
            && self.instance_buffer.is_some()
            && self.bind_group.is_some()
    }

    /// Emit the grass draw inside an already-active render pass.
    /// Caller is responsible for setting the camera bind group
    /// (group 0); we bind the grass bind group at group 1 and the
    /// shadow receiver group at group 2 here.
    pub fn draw(
        &self,
        render_pass: &mut wgpu::RenderPass<'_>,
        shadow_receiver_bg: &wgpu::BindGroup,
    ) {
        if !self.ready_to_draw() {
            return;
        }
        let bg = match &self.bind_group {
            Some(bg) => bg,
            None => return,
        };
        let inst = match &self.instance_buffer {
            Some(b) => b,
            None => return,
        };
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(1, bg, &[]);
        render_pass.set_bind_group(2, shadow_receiver_bg, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_vertex_buffer(1, inst.slice(..));
        render_pass.draw(0..BLADE_VERTICES.len() as u32, 0..self.instance_count);
    }

    /// Read-only access to the cached widget config (used by
    /// callers that need to know `widget.enabled` after a
    /// `set_config` call without holding their own copy).
    pub fn widget(&self) -> &MapGrassWidget {
        &self.widget
    }
}

/// Tiny deterministic hash producing `[0, 1)` floats. Matches the
/// widget's per-patch jitter pattern (deterministic-per-position so
/// blades don't dance between frames).
fn hash01(x: i32, z: i32, salt: u32) -> f32 {
    // xorshift32-ish mix. Repeatable across runs, no random-state.
    let mut h: u32 = (x as u32).wrapping_mul(0x9E3779B1);
    h ^= (z as u32).wrapping_mul(0x85EBCA77);
    h ^= salt.wrapping_mul(0xC2B2AE3D);
    h ^= h.rotate_left(13);
    h = h.wrapping_mul(0x85EBCA6B);
    h ^= h >> 13;
    (h & 0x00FF_FFFF) as f32 / 0x0100_0000 as f32
}

/// Scan an 8-bit distribution mask and produce one instance per
/// non-zero texel. The mask is sized
/// `(map_width_elmos / patch_resolution) x (map_height_elmos /
/// patch_resolution)`; each texel byte controls the blade size for
/// that patch (0 = no blade, 254 = max_size).
///
/// Positions are in **render space** (centred on the origin, scaled
/// against `x_extent_render` / `z_extent_render`). Sizes are in
/// **elmos** -- the vertex shader applies the camera-uniform
/// elmo->render factor at the multiply, so this buffer doesn't go
/// stale when `update_heightmap` later changes the conversion.
pub fn generate_instances(
    widget: &MapGrassWidget,
    mask: &[u8],
    mask_w: u32,
    mask_h: u32,
    x_extent_render: f32,
    z_extent_render: f32,
) -> Vec<GrassInstance> {
    if !widget.enabled || mask.is_empty() {
        return Vec::new();
    }
    let stride_x = (2.0 * x_extent_render) / mask_w.max(1) as f32;
    let stride_z = (2.0 * z_extent_render) / mask_h.max(1) as f32;
    // Jitter as a fraction of the per-texel patch stride. Engine
    // uses `patch_placement_jitter * patch_resolution_elmos`; we
    // mirror that in render space because the mask was sized so
    // that `stride_elmos == patch_resolution`.
    let jitter_amount_x = widget.patch_placement_jitter * stride_x;
    let jitter_amount_z = widget.patch_placement_jitter * stride_z;
    let size_range = (widget.max_size - widget.min_size).max(0.0);

    let mut instances = Vec::new();
    for z in 0..mask_h {
        for x in 0..mask_w {
            let byte = mask[(z * mask_w + x) as usize];
            if byte == 0 {
                continue;
            }
            // Patch centre + per-patch jitter (deterministic from
            // grid position so two loads of the same map produce
            // identical placements).
            let jitter_x = (hash01(x as i32, z as i32, 0) - 0.5) * 2.0 * jitter_amount_x;
            let jitter_z = (hash01(x as i32, z as i32, 1) - 0.5) * 2.0 * jitter_amount_z;
            let rotation = hash01(x as i32, z as i32, 2) * std::f32::consts::TAU;
            let size_elmos = widget.min_size + (byte as f32 / 254.0) * size_range;
            let world_x = -x_extent_render + (x as f32 + 0.5) * stride_x + jitter_x;
            let world_z = -z_extent_render + (z as f32 + 0.5) * stride_z + jitter_z;
            instances.push(GrassInstance {
                world_x,
                rotation,
                world_z,
                size: size_elmos,
            });
        }
    }
    instances
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
                blade_color_tex: Some("maps/blades.dds".to_string()),
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
                dist_tga: Some("maps/dist.tga".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        let w = MapGrassWidget::from_settings(&ms);
        assert!(!w.enabled);
    }

    #[test]
    fn disabled_widget_produces_no_instances() {
        let widget = MapGrassWidget::default();
        let inst = generate_instances(&widget, &[1, 1, 1, 1], 2, 2, 1.0, 1.0);
        assert!(inst.is_empty());
    }

    #[test]
    fn non_zero_mask_seeds_one_instance_per_texel() {
        let widget = MapGrassWidget {
            enabled: true,
            dist_tga: "x".to_string(),
            blade_color_tex: "y".to_string(),
            patch_resolution: 32,
            patch_placement_jitter: 0.0, // disable jitter for predictable test
            ..MapGrassWidget::default()
        };
        // 2x2 mask: top-left and bottom-right set. Render-space
        // half-extents of 1.0 give a [-1, +1] playable area.
        let mask = [254u8, 0, 0, 127];
        let inst = generate_instances(&widget, &mask, 2, 2, 1.0, 1.0);
        assert_eq!(inst.len(), 2);
        // First instance ~ patch centre (-0.5, -0.5), second ~ (+0.5, +0.5).
        assert!((inst[0].world_x - -0.5).abs() < 1e-3);
        assert!((inst[0].world_z - -0.5).abs() < 1e-3);
        assert!((inst[1].world_x - 0.5).abs() < 1e-3);
        // Size scales with byte value.
        assert!(inst[0].size > inst[1].size);
    }

    #[test]
    fn deterministic_jitter() {
        let widget = MapGrassWidget {
            enabled: true,
            dist_tga: "x".to_string(),
            blade_color_tex: "y".to_string(),
            patch_placement_jitter: 0.5,
            ..MapGrassWidget::default()
        };
        let mask = vec![100u8; 16];
        let a = generate_instances(&widget, &mask, 4, 4, 1.0, 1.0);
        let b = generate_instances(&widget, &mask, 4, 4, 1.0, 1.0);
        assert_eq!(a.len(), b.len());
        for (ai, bi) in a.iter().zip(b.iter()) {
            assert_eq!(ai.world_x, bi.world_x);
            assert_eq!(ai.world_z, bi.world_z);
            assert_eq!(ai.rotation, bi.rotation);
        }
    }

    #[test]
    fn size_passes_through_in_elmos() {
        let widget = MapGrassWidget {
            enabled: true,
            dist_tga: "x".to_string(),
            blade_color_tex: "y".to_string(),
            max_size: 2.0,
            min_size: 2.0, // pin size so byte value doesn't matter
            patch_placement_jitter: 0.0,
            ..MapGrassWidget::default()
        };
        let mask = [254u8];
        let inst = generate_instances(&widget, &mask, 1, 1, 1.0, 1.0);
        // Mapinfo `grassMaxSize` lands in the buffer unchanged; the
        // shader applies the elmo->render factor.
        assert!((inst[0].size - 2.0).abs() < 1e-6);
    }

    #[test]
    fn full_config_enables_and_round_trips() {
        let ms = MapSettings {
            custom_grass: CustomGrassSettings {
                dist_tga: Some("maps/dist.tga".to_string()),
                blade_color_tex: Some("maps/blades.dds".to_string()),
                max_size: Some(2.0),
                min_size: Some(0.5),
                patch_resolution: Some(16),
                patch_placement_jitter: Some(0.4),
                map_color_factor: Some(0.2),
                map_color_base: Some(0.6),
                ..Default::default()
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

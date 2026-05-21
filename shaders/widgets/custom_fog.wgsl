// SPDX-License-Identifier: GPL-3.0-or-later
//
// BAR-widget effect: height-based "custom fog".
//
// Origin: BAR's `mapinfo.custom.fog = { color, height, fogatten }`
// block. Driven in-game by a LuaUI widget that tints fragments below
// `height` toward `color`, attenuated by `fogatten` per elmo of depth.
// This is **not** part of the engine's core SMF / BumpWater / sky
// shaders -- it's a per-map-authored cosmetic effect.
//
// Inputs (all from `CameraUniform`):
//   custom_fog_params.x  -- enabled flag (0 / 1).
//   custom_fog_params.y  -- ceiling height in elmos.
//   custom_fog_color_atten.xyz -- fog tint colour (multiplicative).
//   custom_fog_color_atten.w   -- attenuation rate per elmo.
//   height_scale         -- render Y -> elmo Y conversion factor.
//   height_range_elmos   -- map's vertical span in elmos.
//
// Concatenated into the terrain + water shader sources by
// `bar-render::renderer.rs::new` so callers in those shaders can
// just call `apply_custom_fog(color, world_pos)`. See
// `docs/recoil-shader-ports.md` for the widget-port architectural
// rule (effects driven by `mapinfo.custom.*` get their own file
// under `shaders/widgets/`).

/// Apply the mapinfo `custom.fog` height-based tint to a fragment
/// colour. Returns `color` unchanged when the fog is disabled or the
/// fragment is above the ceiling. Inside the fog region the colour
/// is *multiplicatively tinted* toward `fog_color`: at the ceiling
/// `tint = vec3(1)` (no change), at full attenuation `tint =
/// fog_color` (dims and colour-shifts). This matches the in-game
/// widget behaviour where the fog absorbs light selectively per
/// channel rather than blending the fragment toward a bright fog
/// colour (which is what a plain `mix(...)` would do and what makes
/// a naive port look milky / cloudy at depth).
fn apply_custom_fog(color: vec3<f32>, world_pos: vec3<f32>) -> vec3<f32> {
    if (camera.custom_fog_params.x < 0.5) {
        return color;
    }
    let elmo_y = world_pos.y
        / max(camera.height_scale, 1e-4)
        * camera.height_range_elmos;
    let below = camera.custom_fog_params.y - elmo_y;
    if (below <= 0.0) {
        return color;
    }
    let f = clamp(below * camera.custom_fog_color_atten.w, 0.0, 1.0);
    let tint = mix(vec3<f32>(1.0), camera.custom_fog_color_atten.xyz, f);
    return color * tint;
}

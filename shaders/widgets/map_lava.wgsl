// SPDX-License-Identifier: MIT
// Port of bar-game's `shaders/GLSL/lava/lava.frag.glsl`
// (Copyright (c) 2024 Beherith <mysterme@gmail.com>, MIT license).
// Replaces the engine BumpWater pipeline when
// `mapinfo.water.damage > 0` -- see bar-game's
// `luarules/gadgets/map_lava.lua` for the in-engine driver and
// `bar-game/modules/lava.lua` for the config defaults this port
// uses verbatim.
//
// Scope (Core port, per session decision):
// - Textured diffuse + emit + normal-height sampling.
// - Heat distortion (simple time-driven; engine tracks camera
//   direction, which the editor doesn't need).
// - Slow global plane rotation.
// - Sun-driven normal lighting.
// - Coast colour ramp at shorelines (samples the rendered
//   heightmap vs `camera.water_y`).
// - Sun specular highlight, emission boost from diffuse alpha.
//
// Skipped:
// - Parallax mapping (config-gated even in BAR).
// - Per-vertex swirl (we render a uniform plane; the global
//   rotation covers the visible motion).
// - LOS / shadow modulation (editor always shows everything).
// - Tide rhythm (gameplay state).
// - The second `lava_fog_light` additive pass (cosmetic only).

@group(2) @binding(12) var lava_diffuse_emit_tex: texture_2d<f32>;
@group(2) @binding(13) var lava_normal_height_tex: texture_2d<f32>;
@group(2) @binding(14) var lava_distortion_tex: texture_2d<f32>;
@group(2) @binding(15) var lava_sampler: sampler;

// Config matching bar-game/modules/lava.lua defaults. Hardcoded
// for now -- expose later via a `LavaConfig` uniform if the
// editor wants per-map overrides.
const LAVA_UV_SCALE: f32 = 2.0;
const LAVA_GLOBAL_ROT_FREQ: f32 = 0.0001;
const LAVA_GLOBAL_ROT_AMP: f32 = 0.05;
const LAVA_COAST_WIDTH_ELMOS: f32 = 25.0;
const LAVA_HEIGHT_OFFSET_ELMOS: f32 = 2.0;
const LAVA_COAST_COLOR: vec3<f32> = vec3<f32>(2.0, 0.5, 0.0);
const LAVA_SPECULAR_EXP: f32 = 64.0;
const LAVA_SPECULAR_STRENGTH: f32 = 1.0;
// Engine widget's `colorCorrection` multiplier (defaults to white,
// per-map configs override with red/orange-shifted vectors -- e.g.
// Forge.lua sets vec3(1.1, 1.0, 0.88)). Kept as a literal until the
// per-map config plumbing lands.
const LAVA_COLOR_CORRECTION: vec3<f32> = vec3<f32>(1.0, 1.0, 1.0);

/// Sample the heightmap at world-XZ (render space). Mirrors the
/// same texel-coord clamp that `water_shallow_scale` in water.wgsl
/// uses, just without the inverse-extent step (we pass world_uv
/// already in [0,1]).
fn lava_sample_ground_y(world_uv: vec2<f32>) -> f32 {
    let dim = vec2<i32>(textureDimensions(heightmap_tex));
    let dim_f = vec2<f32>(dim);
    let tc = clamp(vec2<i32>(world_uv * dim_f), vec2<i32>(0), dim - vec2<i32>(1));
    return textureLoad(heightmap_tex, tc, 0).r * camera.height_scale;
}

/// Main entry. `world_pos` -- render-space fragment position;
/// `eye_dir` -- unit vector from fragment toward camera. Both
/// already computed by `shade_water` so we don't recompute here.
fn shade_map_lava(world_pos: vec3<f32>, eye_dir: vec3<f32>) -> vec4<f32> {
    // Render-space XZ -> [0,1] across the map.
    let world_uv = vec2<f32>(
        (world_pos.x + camera.x_extent) / max(2.0 * camera.x_extent, 1e-4),
        (world_pos.z + camera.z_extent) / max(2.0 * camera.z_extent, 1e-4),
    );

    // Heat distortion. Engine tracks camera direction so the
    // distortion appears to wash *across* the surface as the user
    // pans; the editor's typical orbit-and-hold camera makes a
    // simple time drift functionally equivalent.
    let heat = vec2<f32>(camera.time * 0.0010, camera.time * 0.0007);
    let dist_tex = textureSample(
        lava_distortion_tex,
        lava_sampler,
        (world_uv + heat) * 45.2,
    );
    // Engine: `distortion = distortion.xy * 0.2 * 0.02`, scaled by
    // emit + coast factor. We compute coast factor below, so we
    // hold a copy of the raw scale here and reapply.
    let dist_raw = dist_tex.xy * 0.2 * 0.02;

    // Slow global plane rotation (engine `worldUV.xy +=
    // vec2(sin(t*freq), cos(t*freq)) * amp`).
    let rot_t = camera.time * LAVA_GLOBAL_ROT_FREQ;
    let global_rot = vec2<f32>(sin(rot_t), cos(rot_t)) * LAVA_GLOBAL_ROT_AMP;

    // Coast factor: how close is the underlying terrain to the
    // lava surface? Engine ramps up over the coast width then
    // sharply tapers in the last 10%. Heights are in render
    // space; convert coast width / height offset from elmos.
    let ground_y = lava_sample_ground_y(world_uv);
    let lava_y = camera.water_y;
    let elmo_to_render_y = camera.height_scale / max(camera.height_range_elmos, 1.0);
    let coast_width_r = LAVA_COAST_WIDTH_ELMOS * elmo_to_render_y;
    let height_offset_r = LAVA_HEIGHT_OFFSET_ELMOS * elmo_to_render_y;
    let cf_raw = clamp(
        (ground_y - lava_y + coast_width_r + height_offset_r) / max(coast_width_r, 1e-6),
        0.0,
        1.0,
    );
    var coast_factor: f32;
    if cf_raw > 0.90 {
        // Last 10% taper: ramp DOWN to zero at full coast.
        coast_factor = (9.0 * (1.0 - cf_raw)) / 0.9;
    } else {
        // First 90%: cubic ramp UP from zero to one.
        let t = cf_raw / 0.9;
        coast_factor = t * t * t;
    }

    // Distortion final scale (engine multiplies by an emit term;
    // we approximate using just the coast factor since we don't
    // have the emit value yet -- it's read from the *post-warp* UV
    // and would otherwise need a chicken-and-egg pre-sample).
    let distortion = dist_raw * clamp(0.5 + coast_factor, 0.2, 2.0);

    // Final sample UVs (engine: world_uv * uv_scale + distortion + swirl).
    let sample_uv = world_uv * LAVA_UV_SCALE + distortion + global_rot;

    let de = textureSample(lava_diffuse_emit_tex, lava_sampler, sample_uv);
    let nh = textureSample(lava_normal_height_tex, lava_sampler, sample_uv);

    // Tangent normal decode (engine: `nh.xzy * 2 - 1`, flip z).
    var n = nh.xzy * 2.0 - 1.0;
    n.z = -n.z;
    n = normalize(n);
    let sun_dir = normalize(camera.sun_dir_exp.xyz);
    let light_amount = clamp(dot(sun_dir, n), 0.2, 1.0);
    var col = de.rgb * light_amount;

    // Coast colour boost (engine adds COASTCOLOR * coast_factor).
    col = col + LAVA_COAST_COLOR * coast_factor;

    // Sun specular: reflect sun about the sampled normal, take
    // `pow(max(R.V, 0), exp)` and gate by SPECULAR_STRENGTH.
    let refl = reflect(-sun_dir, n);
    let spec = clamp(
        pow(max(dot(normalize(eye_dir), refl), 0.0), LAVA_SPECULAR_EXP),
        0.0,
        LAVA_SPECULAR_STRENGTH,
    );
    col = col + col * spec;

    // Emission boost where the diffuse-emit alpha (engine's "heat
    // indicator") is high, modulated by the y-component of the
    // distortion sample so it shimmers with the heat haze.
    col = col + col * (de.a * distortion.y * 700.0);

    // Final colour correction (engine's `SWIZZLECOLORS` macro,
    // typically a per-map vec3 multiplier).
    col = col * LAVA_COLOR_CORRECTION;

    // Distance fog -- same gate `shade_water` uses, so lava under
    // a foggy atmosphere fades to the atmospheric fog colour.
    if camera.fog_dists.y > 0.0 {
        let view_dist = length(camera.camera_pos - world_pos);
        let fog_factor = clamp(
            (camera.fog_dists.y - view_dist)
                / max(camera.fog_dists.y - camera.fog_dists.x, 1e-4),
            0.0,
            1.0,
        );
        col = mix(camera.fog_color.rgb, col, fog_factor);
    }

    return vec4<f32>(col, 1.0);
}

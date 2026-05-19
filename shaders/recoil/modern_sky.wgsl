// SPDX-License-Identifier: GPL-3.0-or-later
//
// Ported from Recoil's ModernSkyFS.glsl / ModernSkyVS.glsl.
// Source path inside the upstream repo:
//   cont/base/springcontent/shaders/GLSL/ModernSkyFS.glsl
// Vendored copy: vendor/recoil/shaders/GLSL/ModernSkyFS.glsl
// Pinned commit recorded in vendor/recoil/UPSTREAM.md.
//
// This file is a derivative work of Recoil and inherits its GPL v3 terms.
// See docs/licensing.md for the full license analysis.
//
// What this exposes:
//   modern_sky(dir: vec3<f32>, time: f32) -> vec3<f32>
//     Returns sky colour for a unit-length world-space view direction.
//     Replaces the abandoned procedural sky function in terrain.wgsl.
//
// What's different from upstream:
//   - Uniforms in upstream become module-level `const` here. The values
//     are BAR-typical defaults (sourced from common BAR mapinfo.lua sky
//     blocks). When MapSettings starts feeding sky parameters into the
//     renderer, these become uniforms again.
//   - The upstream shader returns a vec4 with an alpha channel encoding
//     a "below horizon" mask used by the engine's water rendering. We
//     don't need that here, so we return rgb only.
//   - Upstream has a `SIMPLIFIED_RENDERING` preprocessor switch that
//     trades cumulus-cloud quality for shader cost. We pick the higher-
//     quality path; preview render frames are infrequent enough that
//     the cost doesn't matter.

// Cloud frequency parameters lifted from upstream as constants. These
// are compile-time `#define`s in ModernSkyFS.glsl too -- not exposed via
// mapinfo, so they stay constants here.
const CIRRUS1: f32 = 0.9;
const CUMULUS1: f32 = 1.8;

// `planeColor.w == 0` ⇒ plane disabled (we don't render a horizon plane).
const PLANE_COLOR: vec4<f32> = vec4<f32>(0.45, 0.50, 0.55, 0.0);

// Hash-based 3D value noise from Brian Sharpe's Wombat library — the
// same routine upstream uses (see source comment in ModernSkyFS.glsl).
fn value3d(p_in: vec3<f32>) -> f32 {
    var Pi = floor(p_in);
    let Pf = p_in - Pi;
    Pi = Pi - floor(Pi * (1.0 / 69.0)) * 69.0;
    let one = vec3<f32>(1.0);
    let limit = vec3<f32>(69.0 - 1.5);
    let Pi_inc1 = step(Pi, limit) * (Pi + one);

    var Pt = vec4<f32>(Pi.x, Pi.y, Pi_inc1.x, Pi_inc1.y) + vec4<f32>(50.0, 161.0, 50.0, 161.0);
    Pt = Pt * Pt;
    Pt = vec4<f32>(Pt.x, Pt.z, Pt.x, Pt.z) * vec4<f32>(Pt.y, Pt.y, Pt.w, Pt.w);

    let hash_mod = vec2<f32>(
        1.0 / (635.298681 + Pi.z       * 48.500388),
        1.0 / (635.298681 + Pi_inc1.z  * 48.500388),
    );
    let hash_lowz  = fract(Pt * hash_mod.x);
    let hash_highz = fract(Pt * hash_mod.y);

    let blend = Pf * Pf * Pf * (Pf * (Pf * 6.0 - 15.0) + 10.0);
    let res0 = mix(hash_lowz, hash_highz, vec4<f32>(blend.z));
    let blend2 = vec4<f32>(blend.x, blend.y, 1.0 - blend.x, 1.0 - blend.y);
    return dot(
        res0,
        vec4<f32>(blend2.z, blend2.x, blend2.z, blend2.x)
            * vec4<f32>(blend2.w, blend2.w, blend2.y, blend2.y),
    );
}

// Domain-warping rotation matrix (constant in upstream).
fn warp_mat() -> mat3x3<f32> {
    return mat3x3<f32>(
        vec3<f32>( 0.0,   1.60,  1.20),
        vec3<f32>(-1.6,   0.72, -0.96),
        vec3<f32>(-1.2,  -0.96,  1.28),
    );
}

fn fbm(p_in: vec3<f32>) -> f32 {
    let m = warp_mat();
    var p = p_in;
    var f = 0.0;
    f = f + value3d(p) / 2.0;
    p = m * p * 1.1;
    f = f + value3d(p) / 4.0;
    p = m * p * 1.2;
    f = f + value3d(p) / 6.0;
    p = m * p * 1.3;
    f = f + value3d(p) / 12.0;
    p = m * p * 1.4;
    f = f + value3d(p) / 24.0;
    return f;
}

fn modern_sky(dir: vec3<f32>, time: f32) -> vec3<f32> {
    // Engine's `time` uniform is `frameNum * 0.005f` (see
    // `rts/Rendering/Env/ModernSky.cpp` upstream). At BAR's default
    // 30 game-frames-per-second that advances at 0.15 / real-second.
    // Our `camera.time` is plain wall-clock seconds, so we scale here
    // to match upstream rather than retuning the 0.05 / 0.3 cirrus /
    // cumulus drift multipliers below (which match upstream verbatim).
    // Without this the clouds whip by ~6.7x faster than in-game.
    let t = time * 0.15;
    let pos = normalize(dir);

    // Pull per-map sky parameters from the camera uniform (sourced from
    // mapinfo `atmosphere = { ... }`). Mirrors ModernSkyFS.glsl, which
    // takes the same values as uniforms upstream.
    let sky_color = camera.sky_color_density.xyz;
    let cloud_density = camera.sky_color_density.w;
    let cloud_color = camera.cloud_color.xyz;
    // `camera.sun_color.w` holds the mapinfo `light.sunDir.w` intensity,
    // packed host-side. Engine multiplies the sun-corona contribution by
    // it (`ModernSkyFS.glsl:88`: `sunColor.rgb * sunColor.w * 1.3`); we
    // mirror that here so maps that dim the sun via the 4th sunDir
    // component (rare but supported) render with the right corona power.
    let sun_color = camera.sun_color.xyz * camera.sun_color.w;
    let sun_dir_raw = camera.sky_dir.xyz;

    let cirrus = cloud_density * CIRRUS1;
    let cumulus = cloud_density * CUMULUS1;

    let sun_norm = normalize(sun_dir_raw);
    let sun_contrib = pow(max(0.0, dot(pos, sun_norm)), 64.0);

    // Below the horizon, smooth into the optional plane colour.
    let wp_contrib = (1.0 - smoothstep(-0.5, -0.2, pos.y)) * PLANE_COLOR.w;
    var color = mix(sky_color, PLANE_COLOR.rgb, wp_contrib);
    color = mix(color, sun_color * 1.3, sun_contrib);

    let day_extinction = vec3<f32>(1.0);
    let night_extinction = vec3<f32>(1.0 - exp(sun_dir_raw.y)) * 0.2;
    let extinction = mix(day_extinction, night_extinction, -sun_dir_raw.y * 0.2 + 0.5);

    // Cirrus clouds — high, thin, drift slowly.
    let cirrus_density = smoothstep(
        1.0 - cirrus,
        1.0,
        fbm(pos / max(pos.y, 0.001) * 2.0 + vec3<f32>(t * 0.05)),
    ) * 0.3;
    color = mix(
        color,
        cloud_color * extinction * 4.0,
        cirrus_density * max(pos.y, 0.0),
    );

    // Cumulus clouds — fluffier, two passes for depth (high-quality path).
    for (var i: i32 = 0; i < 2; i = i + 1) {
        var cpos = pos;
        cpos.y = smoothstep(-0.5, 1.5, pos.y);
        let denom = max(cpos.y, 0.001);
        cpos = vec3<f32>(pos.x / denom, cpos.y, pos.z / denom);
        let cumulus_density_i = smoothstep(
            1.0 - cumulus,
            1.0,
            fbm((0.7 + f32(i) * 0.01) * cpos + vec3<f32>(t * 0.3)),
        );
        color = mix(
            color,
            cloud_color * extinction * cumulus_density_i * 5.0,
            min(cumulus_density_i, 1.0) * max(pos.y, 0.0),
        );
    }

    return color;
}

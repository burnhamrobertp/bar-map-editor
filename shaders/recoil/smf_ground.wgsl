// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Recoil's `cont/base/springcontent/shaders/GLSL/SMFFragProg.glsl`
// (and the GetShadeInt helper from the same file). Upstream commit
// pinned in `vendor/recoil/UPSTREAM.md`.
//
// Implements the engine's ground lighting math + water-absorption
// path. Other features in the upstream shader (detail textures, splat
// detail, sky-cube reflections, parallax, shadows, info overlay) are
// stubbed — see `docs/recoil-shader-ports.md` for the full gap list.

// Engine-side constant: the engine multiplies its final shaded ground
// colour by 210/255 ≈ 0.823 to match the historic Spring exposure.
// Mirrored verbatim so colours land at the same intensity as in-engine.
const SMF_INTENSITY_MULT: f32 = 210.0 / 255.0;

// Depth (in elmos below sea level) at which `SMF_WATER_ABSORPTION`
// transitions fully into the underwater colour.
const SMF_SHALLOW_WATER_DEPTH: f32 = 10.0;
const SMF_SHALLOW_WATER_DEPTH_INV: f32 = 1.0 / SMF_SHALLOW_WATER_DEPTH;

/// SMF ground lighting — Lambert diffuse + Blinn-Phong specular,
/// modulated by per-fragment shadow occlusion. Matches the engine's
/// `GetShadeInt` for the above-water path (no water absorption here;
/// see `smf_water_shade` for the underwater branch).
///
/// `world_pos` is in render-space (matches `vertexWorldPos.xyz`
/// upstream). `normal` is the shading normal (already normalised).
/// `sun_dir` is the unit vector pointing TOWARDS the sun. `view_dir`
/// is the unit vector from fragment to camera. `shadow_coeff` is in
/// `[0, 1]`: 0 means fully shadowed, 1 means lit.
fn smf_ground_shade(
    world_pos: vec3<f32>,
    normal: vec3<f32>,
    sun_dir: vec3<f32>,
    view_dir: vec3<f32>,
    ambient: vec3<f32>,
    diffuse_color: vec3<f32>,
    specular_color: vec3<f32>,
    specular_exp: f32,
    shadow_coeff: f32,
) -> vec3<f32> {
    let cos_diffuse = clamp(dot(sun_dir, normal), 0.0, 1.0);
    // Halfway vector between sun and view — Blinn-Phong's defining
    // input. Matches upstream's `halfDir` varying.
    let half_dir = normalize(sun_dir + view_dir);
    let cos_specular = clamp(dot(half_dir, normal), 0.001, 1.0);

    var lit = ambient + diffuse_color * (cos_diffuse * shadow_coeff);
    // Specular contribution is gated on the surface actually facing
    // the light — otherwise grazing-angle bouncebacks light up
    // shadowed surfaces. The engine does this via the shadow
    // multiplier; we keep it explicit.
    let spec = pow(cos_specular, max(specular_exp, 1.0));
    lit = lit + specular_color * spec * shadow_coeff;
    return lit * SMF_INTENSITY_MULT;
}

/// SMF underwater shading — applied to ground fragments below sea
/// level (`world_pos.y < 0`, where 0 is the engine's water plane).
/// Reproduces the engine's `SMF_WATER_ABSORPTION` block: blend from
/// the ground shade towards `water_base_color`, darken the result by
/// `water_absorb_color * |y|` (clamped at 1023 elmos to match the
/// upstream `vertexStepHeight` clamp), preserve a minimum colour via
/// `water_min_color`, then re-attenuate by shadow ahead of the
/// shallow-water lerp.
///
/// `ground_shade` is the above-water shading for this fragment (i.e.
/// what `smf_ground_shade` produced). `world_y` is the fragment's
/// elmo-space height relative to the water plane (negative
/// underwater). `cos_diffuse` is `clamp(dot(sun_dir, n), 0, 1)`.
/// `shadow_coeff` matches `smf_ground_shade`'s argument.
fn smf_water_absorb(
    ground_shade: vec3<f32>,
    world_y: f32,
    cos_diffuse: f32,
    shadow_coeff: f32,
    water_base_color: vec3<f32>,
    water_absorb_color: vec3<f32>,
    water_min_color: vec3<f32>,
) -> vec3<f32> {
    // |y| / shallow_depth, capped at 1.0 once the fragment is at
    // least the shallow-depth below the water plane (the engine adds
    // `float(y <= -shallow)` before clamping, equivalent to clamp).
    let depth_alpha = clamp(abs(world_y) * SMF_SHALLOW_WATER_DEPTH_INV, 0.0, 1.0);
    let decay = 0.2 + depth_alpha * 0.1;
    // Engine clamps absorption depth at 1023 elmos so very deep
    // fragments don't go pitch-black.
    let step_height = min(1023.0, -world_y);
    let water_light = min(cos_diffuse * 2.0 + 0.4, 1.0);

    var water_shade = water_base_color - water_absorb_color * step_height;
    water_shade = max(water_min_color, water_shade);
    water_shade = water_shade * (SMF_INTENSITY_MULT * water_light);
    // Shadowed water absorbs more.
    water_shade = water_shade * (vec3<f32>(1.0) - decay * (vec3<f32>(1.0) - vec3<f32>(shadow_coeff)));

    // depth_alpha=0 (just barely underwater) → blend ground_shade and
    // water_shade by the small factor; depth_alpha=1 → fully water.
    return mix(ground_shade, water_shade, depth_alpha);
}

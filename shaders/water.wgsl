// SPDX-License-Identifier: GPL-2.0-or-later
// Original PBR water shader targeting BAR-website aesthetic.
// Not a Recoil port; written from scratch.
//
// Bind group 3 (water normal map) is declared here.
// camera (group 0), reflection_texture (group 2), refraction_texture (group 4)
// and sky_color() are declared in terrain.wgsl and visible throughout the
// concatenated module.

@group(3) @binding(0) var water_normal_tex: texture_2d<f32>;
@group(3) @binding(1) var water_normal_sam: sampler;

fn fresnel_schlick(cos_theta: f32, f0: f32) -> f32 {
    let m = clamp(1.0 - cos_theta, 0.0, 1.0);
    let m2 = m * m;
    return f0 + (1.0 - f0) * m2 * m2 * m;
}

// PBR water surface. Returns RGBA with alpha=1; transparency comes from the
// refraction sample, not blend state.
//
// world_pos  -- fragment world position
// eye_dir    -- unit vector from fragment toward camera
// screen_uv  -- [0,1] screen UV (framebuffer-space, origin top-left)
fn shade_water(world_pos: vec3<f32>, eye_dir: vec3<f32>, screen_uv: vec2<f32>) -> vec4<f32> {
    let t = camera.time;
    // Lake-scale waves. world_pos.xz is in roughly [-x_extent, +x_extent]
    // (x_extent ~ 0.5 for a typical map), so a UV scale of ~8 gives several
    // wave cycles across the visible surface. Two octaves: a long swell
    // and a finer ripple, with crossing scroll directions so the surface
    // doesn't all flow one way.
    let uv = world_pos.xz * 8.0;
    let n0 = textureSample(water_normal_tex, water_normal_sam,
                           uv * 1.0 + vec2<f32>( 0.018,  0.013) * t).xyz * 2.0 - 1.0;
    let n1 = textureSample(water_normal_tex, water_normal_sam,
                           uv * 2.7 + vec2<f32>(-0.027,  0.020) * t).xyz * 2.0 - 1.0;
    let n2 = textureSample(water_normal_tex, water_normal_sam,
                           uv * 6.5 + vec2<f32>( 0.034, -0.021) * t).xyz * 2.0 - 1.0;
    let n_ts_raw = normalize(n0 * 0.45 + n1 * 0.35 + n2 * 0.20);
    // Bias the tangent-space normal toward flat (0,0,1) to soften the
    // wave normals: lake water has gentle slopes, not steep ripples.
    let n_ts = normalize(mix(vec3<f32>(0.0, 0.0, 1.0), n_ts_raw, 0.45));

    // Tangent-to-world for a flat XZ plane: T = X, B = Z, N = Y.
    // n_ts.xyz = (tangent_x, tangent_y, up) -> world = (ts.x, ts.z, ts.y)
    let normal = normalize(vec3<f32>(n_ts.x, n_ts.z, n_ts.y));

    // Distorted screen-space samples for both planes. Both off-screen passes
    // are rendered into a framebuffer with the same Y orientation as the
    // main pass, so sampling at the water fragment's screen UV (no flip)
    // gives the matching pixel. Distortion is kept low: the wave normals
    // produce significant xz components on a per-pixel basis, so even a
    // small UV offset is visible without smearing the reflection.
    let distort = normal.xz * 0.010;
    let refl_uv = clamp(screen_uv + distort,
                        vec2<f32>(0.0), vec2<f32>(1.0));
    let refr_uv = clamp(screen_uv + distort * 0.5,
                        vec2<f32>(0.0), vec2<f32>(1.0));

    let refl = textureSample(reflection_texture, reflection_sampler, refl_uv).rgb;
    let refr = textureSample(refraction_texture, refraction_sampler, refr_uv).rgb;

    let n_dot_v = max(dot(normal, eye_dir), 0.0);
    let fresnel = fresnel_schlick(n_dot_v, 0.02);

    // Transparency floor: how much the "other side" always shows through,
    // even at the maximum-reflectivity end of each branch. Lake water is
    // not a chrome ball -- you can almost always see something through it,
    // just attenuated. Tune both branches to a similar floor so the
    // surface reads consistently from above and below.
    let max_reflectivity = 0.25;

    var surface: vec3<f32>;
    if (camera.camera_pos.y >= camera.water_y) {
        // Above water: fresnel-blended refraction (bottom seen through
        // water) and reflection (sky / above-water terrain). Cap fresnel
        // so even at grazing angles some refraction gets through.
        let f = min(fresnel, max_reflectivity);
        surface = mix(refr, refl, f);
    } else {
        // Below water: Snell's-window-like blend. True Snell's window is a
        // narrow ~49deg cone, but in an editor that hides the above-water
        // scene at any sane orbit angle. Wide cone + reflectivity cap so
        // refraction is always visible; TIR only nudges the colour at
        // grazing angles rather than fully replacing it.
        let up_ray = normalize(-eye_dir);
        let cos_w = clamp(dot(normal, up_ray), 0.0, 1.0);
        let snell_cone = smoothstep(0.15, 0.55, cos_w);
        // (1 - reflectivity_cap) is the minimum refraction blend.
        let cone = mix(1.0 - max_reflectivity, 1.0, snell_cone);
        surface = mix(refl, refr, cone);
    }

    // No sun glint: that was a PBR addition not present in Recoil's pipeline.
    // If we want a specular highlight on water in the future, port Recoil's
    // BumpWaterFS specular formula rather than the GGX one used here.

    return vec4<f32>(surface, 1.0);
}

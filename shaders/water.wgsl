// SPDX-License-Identifier: MIT OR Apache-2.0
// Original PBR water shader targeting BAR-website aesthetic.
// Not a Recoil port; no GPL terms apply.
//
// Bind group 3 (water normal map) is declared here.
// camera (group 0), reflection_texture (group 2), and sky_color() are
// declared in terrain.wgsl and visible throughout the concatenated module.

@group(3) @binding(0) var water_normal_tex: texture_2d<f32>;
@group(3) @binding(1) var water_normal_sam: sampler;

fn fresnel_schlick(cos_theta: f32, f0: f32) -> f32 {
    let m = clamp(1.0 - cos_theta, 0.0, 1.0);
    let m2 = m * m;
    return f0 + (1.0 - f0) * m2 * m2 * m;
}

fn ggx_d(n_dot_h: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let denom = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (3.14159265 * denom * denom);
}

// PBR water surface. Returns RGBA with alpha=1.
// world_pos  -- fragment world position
// eye_dir    -- unit vector from fragment toward camera
// screen_uv  -- [0,1] screen UV, used to sample planar reflection
fn shade_water(world_pos: vec3<f32>, eye_dir: vec3<f32>, screen_uv: vec2<f32>) -> vec4<f32> {
    let t = camera.time;
    let uv = world_pos.xz * 0.05;

    // 4 octaves of scrolling normal samples, weighted toward low frequencies.
    let n0 = textureSample(water_normal_tex, water_normal_sam,
                           uv * 1.0  + vec2<f32>( 0.012,  0.009) * t).xyz * 2.0 - 1.0;
    let n1 = textureSample(water_normal_tex, water_normal_sam,
                           uv * 2.3  + vec2<f32>(-0.020,  0.014) * t).xyz * 2.0 - 1.0;
    let n2 = textureSample(water_normal_tex, water_normal_sam,
                           uv * 5.7  + vec2<f32>( 0.025, -0.011) * t).xyz * 2.0 - 1.0;
    let n3 = textureSample(water_normal_tex, water_normal_sam,
                           uv * 11.0 + vec2<f32>(-0.018, -0.022) * t).xyz * 2.0 - 1.0;
    let n_ts = normalize(n0 * 0.5 + n1 * 0.3 + n2 * 0.15 + n3 * 0.05);

    // Tangent-to-world for a flat XZ plane: T = X, B = Z, N = Y.
    // n_ts.xyz = (tangent_x, tangent_y, up) -> world = (ts.x, ts.z, ts.y)
    let normal = normalize(vec3<f32>(n_ts.x, n_ts.z, n_ts.y));

    let n_dot_v = max(dot(normal, eye_dir), 0.0);
    let fresnel = fresnel_schlick(n_dot_v, 0.02);

    var refl: vec3<f32>;
    if camera.quality > 0.5 {
        // Distort reflection sample by surface normal horizontal components.
        let distort = normal.xz * 0.04;
        let refl_uv = clamp(screen_uv + distort, vec2<f32>(0.0), vec2<f32>(1.0));
        refl = textureSample(reflection_texture, reflection_sampler, refl_uv).rgb;
    } else {
        let refl_dir = reflect(-eye_dir, normal);
        refl = sky_color(refl_dir);
    }

    let flat_sky = sky_color(reflect(-eye_dir, vec3<f32>(0.0, 1.0, 0.0)));
    let base = camera.water_base_color.rgb;
    let surface = mix(base, mix(flat_sky, refl, 0.85), fresnel);

    // GGX sun glint -- tight roughness (0.06) for a hard highlight.
    let sun_dir = normalize(camera.sun_dir_exp.xyz);
    let h = normalize(eye_dir + sun_dir);
    let n_dot_h = max(dot(normal, h), 0.0);
    let n_dot_l = max(dot(normal, sun_dir), 0.0);
    let d = ggx_d(n_dot_h, 0.06);
    let glint = d * n_dot_l * camera.ground_specular.rgb * 6.0;

    return vec4<f32>(surface + glint, 1.0);
}

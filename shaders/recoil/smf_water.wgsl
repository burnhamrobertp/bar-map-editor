// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from BumpWaterFS.glsl / BumpWaterVS.glsl
// Upstream: beyond-all-reason/spring @ 681aea7cb8
// Source: cont/base/springcontent/shaders/GLSL/

@group(3) @binding(0) var water_normal_tex: texture_2d<f32>;
@group(3) @binding(1) var water_normal_sam: sampler;

/// Fresnel-blended planar reflection + normal-mapped surface + Blinn-Phong
/// specular. Ported from BumpWaterFS.glsl `GetNormal` / `GetReflection`.
///
/// `world_pos` — render-space position of the water surface fragment.
/// `eye_dir`   — unit vector from fragment toward the camera.
/// `screen_uv` — [0,1] screen coordinate of this fragment, used to sample
///               the planar-reflection texture with normal distortion.
fn bump_water(world_pos: vec3<f32>, eye_dir: vec3<f32>, screen_uv: vec2<f32>) -> vec4<f32> {
    let t = camera.time;
    let p = world_pos.xz;

    // 4-octave scrolling UVs.  Scale and direction per octave break tiling
    // without domain-warp overhead.  Scales mirror BumpWaterVS texCoords[1–2].
    let uv1 = p * 0.020 + vec2<f32>( 1.0,   0.0  ) * t * 0.010;
    let uv2 = p * 0.035 + vec2<f32>(-0.78,  0.78 ) * t * 0.012;
    let uv3 = p * 0.050 + vec2<f32>( 0.0,   1.0  ) * t * 0.015;
    let uv4 = p * 0.100 + vec2<f32>(-0.71, -0.71 ) * t * 0.020;

    // Sample normal map at each octave; decode from [0,1] → [-1,1] and average.
    let n1 = textureSample(water_normal_tex, water_normal_sam, uv1).rgb * 2.0 - 1.0;
    let n2 = textureSample(water_normal_tex, water_normal_sam, uv2).rgb * 2.0 - 1.0;
    let n3 = textureSample(water_normal_tex, water_normal_sam, uv3).rgb * 2.0 - 1.0;
    let n4 = textureSample(water_normal_tex, water_normal_sam, uv4).rgb * 2.0 - 1.0;
    let normal = normalize(n1 + n2 + n3 + n4);

    // Schlick Fresnel — f0 = 0.02 (water at normal incidence, matching
    // BumpWaterFS FresnelMin/Max defaults).
    let f0 = 0.02;
    let ndotv = max(dot(normal, eye_dir), 0.0);
    let fresnel = f0 + (1.0 - f0) * pow(1.0 - ndotv, 5.0);

    // Planar reflection sampled at normal-distorted screen UV (high-pass only;
    // in low-pass the reflection texture is a 1×1 stub so we skip it).
    var refl = vec3<f32>(0.0);
    if camera.quality > 0.5 {
        let refl_uv = clamp(screen_uv + normal.xz * 0.04, vec2<f32>(0.0), vec2<f32>(1.0));
        refl = textureSample(reflection_texture, reflection_sampler, refl_uv).rgb;
    }

    // Blinn-Phong specular: sun direction + exponent from camera uniform,
    // tinted by groundSpecularColor (matches the ground shading pass).
    let sun_dir   = camera.sun_dir_exp.xyz;
    let shininess = camera.sun_dir_exp.w;
    let half_dir  = normalize(sun_dir + eye_dir);
    let spec      = pow(max(dot(normal, half_dir), 0.0), shininess) * camera.ground_specular.rgb;

    return vec4<f32>(mix(camera.water_base_color.rgb, refl, fresnel) + spec, 1.0);
}

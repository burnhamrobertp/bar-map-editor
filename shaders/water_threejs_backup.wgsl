// SPDX-License-Identifier: GPL-2.0-or-later
// Water shader -- direct port of three.js stock `Water.js` (Slayvin / Jonas
// Wagner ocean shader) used by the BAR community website at
// https://www.beyondallreason.info/map/<slug>. The website achieves its
// recognisably "good-looking water" appearance from this exact algorithm
// plus an HDR sky environment map; we match the algorithm here and feed it
// the same flavour of inputs (planar reflection texture + tiled normal map).
//
// Bind group 3 (water normal map) is declared here.
// camera (group 0), reflection_texture (group 2), refraction_texture (group 4)
// and sky_color() are declared in terrain.wgsl and visible throughout the
// concatenated module.

@group(3) @binding(0) var water_normal_tex: texture_2d<f32>;
@group(3) @binding(1) var water_normal_sam: sampler;

/// World-space scale that maps our render-space coordinates into the same
/// numeric range three.js's Water.js expects -- their shader samples
/// `worldPosition.xz` (BAR engine units, ~hundreds to thousands per map)
/// against octave divisors of 103 / 107 / 8907 / 1091. Our world-space
/// range is roughly [-0.5, 0.5], so we pre-scale by this factor to land the
/// sample UVs in the same neighbourhood and reproduce their wavelength.
const WAVE_WORLD_SCALE: f32 = 2000.0;

/// Multiplier on `camera.time` (seconds) to match the website's animation
/// rate. They tick `time` by `waterSpeed = 0.0042` per frame at ~60 FPS,
/// which is ~0.25/s. Tuning here changes how "fast" the surface flows.
const WAVE_TIME_SCALE: f32 = 0.25;

/// Distortion scale on the reflection UV. Matches CONFIG.waterScale on the
/// website. Scaled down by 0.001 because their distortion formula multiplies
/// by 1/distance in their world units (~hundreds), where ours is ~1.
const DISTORTION_SCALE: f32 = 0.03;

/// Schlick base reflectance. Water.js uses 0.3 (an artistic choice -- water's
/// true Fresnel f0 is ~0.02). The higher value gives the silvery sheen the
/// website water is known for, especially at top-down camera angles where
/// `dot(N, E)` is high and would otherwise barely reflect at all.
const RF0: f32 = 0.3;

/// Output alpha. Matches `CONFIG.waterOpacity = 0.6` from the website.
/// Combined with the pipeline's ALPHA_BLENDING state, this lets the water
/// composite over whatever's already in the framebuffer (planned: underwater
/// features once they're added to the refraction pre-pass -- see
/// `project_water_overhaul.md`).
const WATER_ALPHA: f32 = 0.6;

/// Multi-octave normal-map sampling (Water.js `getNoise`). Sums four lookups
/// of the same texture at four very different scales with time-animated UV
/// offsets, producing a wave field with both fine ripple and long swell from
/// a single tiled normal-map texture.
fn water_noise(uv: vec2<f32>, t: f32) -> vec4<f32> {
    let uv0 = uv / 103.0 + vec2<f32>(t / 17.0, t / 29.0);
    let uv1 = uv / 107.0 - vec2<f32>(t / -19.0, t / 31.0);
    let uv2 = uv / vec2<f32>(8907.0, 9803.0) + vec2<f32>(t / 101.0, t / 97.0);
    let uv3 = uv / vec2<f32>(1091.0, 1027.0) - vec2<f32>(t / 109.0, t / -113.0);
    let n = textureSample(water_normal_tex, water_normal_sam, uv0)
          + textureSample(water_normal_tex, water_normal_sam, uv1)
          + textureSample(water_normal_tex, water_normal_sam, uv2)
          + textureSample(water_normal_tex, water_normal_sam, uv3);
    return n * 0.5 - 1.0;
}

/// Sun specular + diffuse contributions, matching Water.js `sunLight()`.
/// `shiny` = Blinn-Phong exponent (100 in Water.js), `spec_strength` = 2.0,
/// `diff_strength` = 0.5.
struct SunContribution {
    diffuse:  vec3<f32>,
    specular: vec3<f32>,
};

fn water_sun_light(
    surface_normal: vec3<f32>,
    eye_dir:        vec3<f32>,
    sun_dir:        vec3<f32>,
    sun_color:      vec3<f32>,
) -> SunContribution {
    let reflection = normalize(reflect(-sun_dir, surface_normal));
    let direction  = max(0.0, dot(eye_dir, reflection));
    var out: SunContribution;
    out.specular = pow(direction, 100.0) * sun_color * 2.0;
    out.diffuse  = max(dot(sun_dir, surface_normal), 0.0) * sun_color * 0.5;
    return out;
}

/// Shade a water-plane fragment.
///
/// `world_pos`  -- fragment world position (render space).
/// `eye_dir`    -- unit vector from fragment toward camera (eye - world_pos).
/// `screen_uv`  -- [0,1] framebuffer UV; samples the reflection texture
///                 rendered with the mirrored camera in the reflection
///                 pre-pass. UV is not Y-flipped because both passes share
///                 the same framebuffer orientation.
fn shade_water(world_pos: vec3<f32>, eye_dir: vec3<f32>, screen_uv: vec2<f32>) -> vec4<f32> {
    // --- 1. Multi-octave normal -------------------------------------------
    let t     = camera.time * WAVE_TIME_SCALE;
    let noise = water_noise(world_pos.xz * WAVE_WORLD_SCALE, t);
    let n     = normalize(noise.xzy * vec3<f32>(1.5, 1.0, 1.5));

    // --- 2. Sun lighting --------------------------------------------------
    let sun_dir   = normalize(camera.sun_dir_exp.xyz);
    // The Water.js `sunColor` uniform is a dim grey (0x333333 on the
    // website). Our closest existing channel is ground_specular -- the
    // SMF-driven sun specular color -- which is typically in a similar
    // range. Using it ties water sun response to the map's lighting table.
    let sun_color = camera.ground_specular.xyz;
    let sun       = water_sun_light(n, eye_dir, sun_dir, sun_color);

    // --- 3. Reflection sample with distance-attenuated distortion ---------
    let world_to_eye = camera.camera_pos - world_pos;
    let distance_w   = length(world_to_eye);
    let distortion   = n.xz * (0.001 + 1.0 / max(distance_w, 0.001)) * DISTORTION_SCALE;
    let refl_uv      = clamp(screen_uv + distortion, vec2<f32>(0.0), vec2<f32>(1.0));
    let reflection_sample = textureSample(reflection_texture, reflection_sampler, refl_uv).rgb;

    // --- 4. Schlick Fresnel ----------------------------------------------
    let theta       = max(dot(eye_dir, n), 0.0);
    let reflectance = RF0 + (1.0 - RF0) * pow(1.0 - theta, 5.0);

    // --- 5. Two-branch mix -----------------------------------------------
    // `scatter` is the "looking through the water" contribution. Water.js
    // simply uses `(N . E) * waterColor` here -- a flat tint with no actual
    // underwater geometry sample. That's what makes underwater features
    // invisible from above on the website. When the upcoming change adds
    // features to the refraction pre-pass, this is where we'll blend in
    // `refraction_texture` -- the rest of the algorithm doesn't change.
    //
    // Source from `water_base_color` (SMF water.basecolor channel, populated
    // by the SD7 importer from `mapinfo.lua`'s `water.basecolor` table and
    // editable in the recipe). Falls back to the per-frame water_r/g/b
    // channel when the map didn't author a basecolor (length-squared zero),
    // so freshly-created projects without mapinfo still get a sensible blue.
    let mapinfo_color = camera.water_base_color.xyz;
    let fallback      = vec3<f32>(camera.water_r, camera.water_g, camera.water_b);
    let water_color   = select(mapinfo_color, fallback, length(mapinfo_color) < 1e-4);
    let scatter       = max(0.0, dot(n, eye_dir)) * water_color;
    let shadow      = sample_shadow(world_pos);

    let diffuse_branch    = (sun_color * sun.diffuse * 0.3 + scatter) * shadow;
    let reflection_branch = vec3<f32>(0.1)
                          + reflection_sample * 0.9
                          + reflection_sample * sun.specular;
    let outgoing = mix(diffuse_branch, reflection_branch, reflectance);

    return vec4<f32>(outgoing, WATER_ALPHA);
}

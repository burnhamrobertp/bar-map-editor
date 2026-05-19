// SPDX-License-Identifier: GPL-2.0-or-later
// Water shader -- port of Recoil's `cont/base/springcontent/shaders/GLSL/
// BumpWaterFS.glsl` to WGSL. Matches the in-engine BAR water appearance
// (refraction-dominant with a Fresnel-mixed reflection layered on top, plus
// a sun specular highlight). Per-map values come from `mapinfo.lua`'s water
// table -- parsed by `bar-engine/src/importer.rs`, stored in the recipe's
// `WaterSettings`, and uploaded each frame via the `water_params` uniform.
//
// What's ported from BumpWaterFS:
//   - 4-octave normal sampling with the PerlinAmp falloff (the `a/a^2/a^3/a^4`
//     amplitude chain in `GetNormal`).
//   - Refraction sample with normal-driven UV distortion (refractDistortion).
//   - Water surface tint mix: refraction below, surface above, blended by
//     `0.1 + surfaceMix * 0.1` exactly as in `BumpWaterFS.glsl:316`.
//   - Sun specular highlight (anti-Phong: gated by `angle` so it's strongest
//     at glancing angles).
//   - Fresnel-mixed reflection layered last, with `fresnelMin + fresnelMax *
//     pow(angle, fresnelPower)`.
//   - Shadow occlusion on the specular contribution.
//
// What's deferred (port follow-ups; see `project_water_overhaul.md`):
//   - Depth-buffer-driven refraction mixback (BumpWaterFS:304-314). Needs
//     the main depth attachment bound as a sampled depth texture.
//   - Caustics (BumpWaterFS:324-334).
//   - Shore foam + cliff foam (BumpWaterFS:GetShorewaves).
//   - Multi-tap reflection blur (BumpWaterFS:234-244).
//   - infotex / coastmap (in-engine map overlays).
//
// Bind group 3 (water normal map) is declared here. camera (group 0),
// reflection + refraction textures + water_params (group 2), and sky_color()
// are declared in terrain.wgsl and visible throughout the concatenated module.

@group(3) @binding(0) var water_normal_tex: texture_2d<f32>;
@group(3) @binding(1) var water_normal_sam: sampler;

/// Per-map BumpWater inputs. Matches the layout of `WaterParamsUniform` in
/// `renderer.rs`. Bound as `@binding(4)` of group 2 (water_planes).
struct WaterParams {
    /// rgb = surfaceColor, w = surfaceAlpha
    surface_color_alpha: vec4<f32>,
    /// rgb = diffuseColor, w = diffuseFactor
    diffuse_color_factor: vec4<f32>,
    /// rgb = specularColor, w = specularPower
    specular_color_power: vec4<f32>,
    /// x = ambientFactor, y = specularFactor,
    /// z = reflectionDistortion, w = perlinAmplitude
    factors: vec4<f32>,
    /// x = fresnelMin, y = fresnelMax, z = fresnelPower, w = unused
    fresnel: vec4<f32>,
    /// x = blurBase (pixels), y = blurExponent, zw reserved. Drives
    /// the 7-tap `opt_blurreflection` path -- the shader divides x
    /// by `camera.screen_h` to get a UV-space offset, then applies
    /// progressively-larger taps via the `y` exponent.
    blur: vec4<f32>,
}

@group(2) @binding(4) var<uniform> water_params: WaterParams;

/// Refraction-pass depth texture, sampled by `shade_water` to do the
/// engine's depth-aware refraction mixback (BumpWaterFS:304-314). When
/// the distorted refraction UV pulls in a sample that's *closer* than
/// the water plane fragment itself, we know the distortion picked up
/// above-water content (typically near a shoreline) and we replace
/// that sample with the undistorted version. Prevents the visible
/// shore-bleed artifact where distortion smears above-water terrain
/// across the water surface.
@group(2) @binding(5) var refraction_depth_tex: texture_depth_2d;
@group(2) @binding(6) var refraction_depth_sam: sampler;

/// Sample the water normal map at four scales with time-animated UV offsets,
/// then accumulate with the PerlinAmp falloff. Matches `BumpWaterFS.glsl:
/// GetNormal()` -- four octaves with amplitudes a, a^2, a^3, a^4.
///
/// `world_xz` is the fragment's world XZ position (render space). `t` is
/// `camera.time` in seconds; the per-octave time scaling produces drift in
/// crossing directions so the surface doesn't all flow one way.
fn water_octave_normal(world_xz: vec2<f32>, t: f32) -> vec3<f32> {
    let a = water_params.factors.w;
    // World-space wave scales. Render space is roughly [-0.5, 0.5] per axis,
    // so multiplying by `base_scale` gives that many texture cycles across
    // the visible map. 16 cycles -> each cycle covers ~6% of the map width,
    // which is roughly the wavelength you see in BAR in-game water at a
    // gameplay-typical zoom. The four octaves drift in crossed directions
    // so the surface doesn't all flow one way.
    let base_uv = world_xz * 16.0;

    let uv0 = base_uv * 1.0 + vec2<f32>( 0.020,  0.013) * t;
    let uv1 = base_uv * 2.1 - vec2<f32>( 0.027,  0.020) * t;
    let uv2 = base_uv * 4.7 + vec2<f32>(-0.034,  0.021) * t;
    let uv3 = base_uv * 9.3 - vec2<f32>(-0.018, -0.029) * t;

    let n0 = (textureSample(water_normal_tex, water_normal_sam, uv0).rgb * 2.0 - 1.0) * a;
    let n1 = (textureSample(water_normal_tex, water_normal_sam, uv1).rgb * 2.0 - 1.0) * a * a;
    let n2 = (textureSample(water_normal_tex, water_normal_sam, uv2).rgb * 2.0 - 1.0) * a * a * a;
    let n3 = (textureSample(water_normal_tex, water_normal_sam, uv3).rgb * 2.0 - 1.0) * a * a * a * a;

    // `xzy` swizzle matches BumpWaterFS:158 -- the texture stores tangent-
    // space normals with y as up, but our world-space water normal points
    // out of the XZ plane (world Y up), so we swap.
    return normalize((n0 + n1 + n2 + n3).xzy);
}

/// Schlick-like fresnel from BumpWaterFS:246. Not actually Schlick: it's a
/// tunable falloff so map authors can dial how reflective the water gets at
/// grazing angles. `angle` here is `1 - |eye . normal|`: 0 = straight down,
/// 1 = horizon-grazing.
fn water_fresnel(angle: f32) -> f32 {
    let f_min   = water_params.fresnel.x;
    let f_max   = water_params.fresnel.y;
    let f_power = water_params.fresnel.z;
    return f_min + f_max * pow(angle, f_power);
}

/// Water-depth-based attenuation used by BumpWater to make shallow water near
/// shore read as clear (refraction-dominant, no reflection, no specular) and
/// deeper water read as the full BumpWater appearance with Fresnel reflection
/// + sun glint. Without this, our deep- and shallow-water fragments would
/// composite identically, which is a real engine deviation.
///
/// BumpWaterFS samples a precomputed `invwaterdepth` texture (heightmap-alpha
/// channel built by `BumpWater.cpp`); we approximate by sampling our raw
/// heightmap texture for the seabed Y at the fragment's XZ and computing
/// `clamp(depth / threshold, 0, 1)`. The threshold is in render space; the
/// `33.0 / height_scale` factor gives shallow_scale = 1 at roughly 3% of the
/// map's vertical span, which lands at a small number of elmos for typical
/// maps. Exact match against BumpWater's `opt_depth` path (camera-distance-
/// driven) is a deferred follow-up.
fn water_shallow_scale(world_pos: vec3<f32>) -> f32 {
    // Map XZ to heightmap UV: render space spans [-x_extent, +x_extent] on
    // X and [-z_extent, +z_extent] on Z. The terrain mesh uses this same
    // mapping (terrain.wgsl::vs_main).
    let hm_uv = vec2<f32>(
        world_pos.x / (2.0 * camera.x_extent) + 0.5,
        world_pos.z / (2.0 * camera.z_extent) + 0.5,
    );
    // Off-playable water (extended plane covering the map-edge extension):
    // there's no playable heightmap to derive depth from, so let
    // `textureLoad`'s clamp-to-edge sampling produce stale-edge values
    // would make `shallow_scale` collapse toward 0 on mountainous-edge
    // maps (the BumpWater surface vanishes, water becomes pure
    // refraction-clear-colour). Engine reads depth from the actual
    // depth buffer (`opt_depth` path) which DOES register the visible
    // mirrored extension terrain at those screen pixels, producing
    // `shallowScale > 0` and a full BumpWater appearance there. Until
    // we port the depth-buffer path, treat off-playable fragments as
    // deep open sea -- matches the engine result for visible content.
    if any(hm_uv < vec2<f32>(0.0)) || any(hm_uv > vec2<f32>(1.0)) {
        return 1.0;
    }
    let dim = vec2<i32>(textureDimensions(heightmap_tex));
    let dim_f = vec2<f32>(dim);
    let tc = clamp(vec2<i32>(hm_uv * dim_f), vec2<i32>(0), dim - vec2<i32>(1));
    let terrain_y = textureLoad(heightmap_tex, tc, 0).r * camera.height_scale;
    let depth = max(0.0, camera.water_y - terrain_y);
    return clamp(depth * 33.0 / max(camera.height_scale, 1e-4), 0.0, 1.0);
}

/// Shade a water-plane fragment.
///
/// `world_pos` -- fragment world position (render space).
/// `eye_dir`   -- unit vector from fragment toward camera.
/// `screen_uv` -- [0,1] framebuffer UV; samples the reflection / refraction
///                textures, both rendered with the main camera's projection
///                so a no-flip lookup hits the matching pixel.
/// `frag_z`    -- this fragment's NDC depth (0..1). Compared against the
///                refraction-pass depth at the distorted UV for the
///                BumpWater mixback (see `refraction_depth_tex`).
fn shade_water(
    world_pos: vec3<f32>,
    eye_dir: vec3<f32>,
    screen_uv: vec2<f32>,
    frag_z: f32,
) -> vec4<f32> {
    // --- 1. Normal + depth-based attenuation ----------------------------
    let normal        = water_octave_normal(world_pos.xz, camera.time);
    let shallow_scale = water_shallow_scale(world_pos);

    // --- 2. Surface lighting ---------------------------------------------
    // Matches BumpWaterFS:283-289. `SunLow` flattens the sun direction on Y
    // so the diffuse term peaks for nearly-horizontal water normals rather
    // than vertical -- the upstream "water gets brighter when looking
    // toward the sun" feel.
    let sun_dir       = normalize(camera.sun_dir_exp.xyz);
    let sun_low       = sun_dir * vec3<f32>(1.0, 0.1, 1.0);
    let eye_normal_cos = dot(-eye_dir, normal);
    let angle         = 1.0 - abs(eye_normal_cos);

    let diffuse  = pow(max(dot(normal, sun_low), 0.0), 3.0) * water_params.diffuse_color_factor.w;
    let ambient  = smoothstep(-1.3, 0.0, eye_normal_cos) * water_params.factors.x;

    let surface_color = water_params.surface_color_alpha.rgb;
    let surface_alpha = water_params.surface_color_alpha.w;
    let diffuse_color = water_params.diffuse_color_factor.rgb;

    let water_surface = surface_color
                      + diffuse_color * diffuse
                      + vec3<f32>(ambient);
    // BumpWaterFS:290 -- surfaceMix is gated by shallowScale so shoreline
    // shallow water has near-zero surface tint and reads as the refraction
    // sample directly.
    let surface_mix   = (surface_alpha + diffuse) * shallow_scale;

    // --- 3. Refraction ---------------------------------------------------
    // Engine formula (`BumpWaterFS:291`):
    //   distortPx = 60 * (1 - pow(fragZ, 80)) * shallowScale
    //   refrUV    = screencoord + normal.xz * distortPx * ScreenInverse
    // ScreenInverse = (1/screen_w, 1/screen_h). Earlier comments here
    // speculated that engine ran a `BumpWaterCoastBlur` over the
    // refraction texture before sampling -- that turned out to be wrong
    // (`BumpWaterCoastBlurFS` bakes the coastmap for shore foam, not
    // the refraction). So the engine-faithful magnitude IS the pixel
    // formula above; smaller previews get stronger distortion just as
    // BAR running at lower resolution does.
    //
    // The `pow(z, 80)` depth gate near-fully attenuates distortion at
    // the far plane so deep-water refraction doesn't blow out into
    // garbage at the horizon.
    let depth_factor       = 1.0 - pow(frag_z, 80.0);
    let refract_distortion = 60.0 * depth_factor * shallow_scale;
    let screen_inv = vec2<f32>(
        1.0 / max(camera.screen_w, 1.0),
        1.0 / max(camera.screen_h, 1.0),
    );
    let refr_uv            = clamp(
        screen_uv + normal.xz * refract_distortion * screen_inv,
        vec2<f32>(0.0),
        vec2<f32>(1.0),
    );
    let refr_distorted = textureSample(refraction_texture, refraction_sampler, refr_uv).rgb;

    // Engine depth-aware mixback (BumpWaterFS:304-314). The distorted
    // refraction UV can pull in fragments that are CLOSER to the
    // camera than the water plane itself -- typically above-water
    // shoreline terrain bleeding into the water near coastlines.
    // Sample the refraction-pass depth at the distorted UV; if it's
    // closer than our own depth, replace the distorted sample with
    // an undistorted one. Engine does this in eye-space (linear);
    // we do it in NDC depth directly. The comparison is monotonic
    // either way, and we don't need linear precision for the gate.
    let refr_depth = textureSample(refraction_depth_tex, refraction_depth_sam, refr_uv);
    let mixback = clamp(frag_z - refr_depth, 0.0, 1.0);
    let refr_undistorted = textureSample(refraction_texture, refraction_sampler, screen_uv).rgb;
    let refr_color = mix(refr_distorted, refr_undistorted, mixback);

    // Refraction-dominant base mix (BumpWaterFS:316). `0.1 + surfaceMix * 0.1`
    // means even at peak surface contribution the refraction sample is still
    // ~80% of the base color -- that's what makes underwater terrain and
    // features visible from above.
    var col = mix(refr_color, water_surface, 0.1 + surface_mix * 0.1);

    // --- 4. Sun specular -------------------------------------------------
    // BumpWaterFS:284-285,348. `angle` gates spec to be strong only at
    // glancing angles; `shallowScale` cuts it entirely in shallow water.
    let reflect_dir    = reflect(normalize(-sun_dir), normal);
    let spec_intensity = angle
        * pow(max(dot(reflect_dir, eye_dir), 0.0), water_params.specular_color_power.w)
        * water_params.factors.y
        * shallow_scale;
    // Engine `BumpWater.cpp:453` bakes `groundShadowDensity` into a
    // `#define shadowDensity` constant, then the BumpWater fragment
    // shader gates the shadow contribution by it. We mirror by
    // modulating the raw sample against `ground_specular.w` (the
    // density slot from `SmfLighting`).
    let shadow = mix(1.0, sample_shadow(world_pos), camera.ground_specular.w);
    col = col + shadow * spec_intensity * water_params.specular_color_power.rgb;

    // --- 5. Reflection (Fresnel-mixed last) ------------------------------
    // BumpWaterFS:345 -- the `fresnel * shallowScale` factor is what keeps
    // shoreline water clear: even at grazing angles, shallow water doesn't
    // pick up the mirror-like reflection.
    let refl_distort = normal.xz * 0.05 * water_params.factors.z;
    let refl_uv      = clamp(screen_uv + refl_distort, vec2<f32>(0.0), vec2<f32>(1.0));

    // Engine `opt_blurreflection` (BumpWaterFS:234-244): 7 extra
    // reflection samples along a vertical streak with geometric-
    // progression spacing, all averaged together. Softens / dims the
    // peak brightness of bright cubemap reflections at glancing
    // angles. `blurBase` / `blurExponent` now come from mapinfo
    // (`MapInfo.cpp:261-262`, defaults 2.0 / 1.5); per-map overrides
    // matter for shoreline maps that crank the blur to hide hard
    // refraction edges.
    var refl_acc = textureSample(reflection_texture, reflection_sampler, refl_uv).rgb;
    var blur_off = vec2<f32>(0.0, water_params.blur.x / max(camera.screen_h, 1.0));
    let blur_exp = water_params.blur.y;
    refl_acc += textureSample(reflection_texture, reflection_sampler, refl_uv + blur_off).rgb;
    blur_off *= blur_exp;
    refl_acc += textureSample(reflection_texture, reflection_sampler, refl_uv + blur_off).rgb;
    blur_off *= blur_exp;
    refl_acc += textureSample(reflection_texture, reflection_sampler, refl_uv + blur_off).rgb;
    blur_off *= blur_exp;
    refl_acc += textureSample(reflection_texture, reflection_sampler, refl_uv + blur_off).rgb;
    blur_off *= blur_exp;
    refl_acc += textureSample(reflection_texture, reflection_sampler, refl_uv + blur_off).rgb;
    blur_off *= blur_exp;
    refl_acc += textureSample(reflection_texture, reflection_sampler, refl_uv + blur_off).rgb;
    blur_off *= blur_exp;
    refl_acc += textureSample(reflection_texture, reflection_sampler, refl_uv + blur_off).rgb;
    let refl_color = refl_acc * 0.125;
    let fresnel = water_fresnel(angle);
    col = mix(col, refl_color, fresnel * shallow_scale);

    // --- 6. Tonemap ------------------------------------------------------
    // BumpWaterFS produces HDR values: `waterSurface = surfaceColor +
    // diffuseColor*diffuse + ambient` can sum well above 1.0 (e.g. 1.65 for
    // Aurelia's tuning), and the sun-specular peak adds another ~0.3 on top.
    // BAR's engine tonemaps the whole frame at the end of its post-process
    // pipeline; without that step, our 8-bit framebuffer saturates these
    // peaks to pure white -- which is what made Aurelia's water render
    // mercury-silver instead of a muted in-game look.
    //
    // Extended Reinhard with a high white-point compresses the over-bright
    // peaks back into LDR while leaving mid-range mostly intact. White-point
    // 4.0 means values up to 4x exposed get mapped to 1.0; mid-range
    // (around 0.5) drops only ~5%.
    let white = 4.0;
    let white_sq = white * white;
    col = col * (1.0 + col / vec3<f32>(white_sq)) / (1.0 + col);

    // NOTE: `custom.fog` is intentionally NOT applied here even though the
    // water plane sits at `water_y` (below the fog ceiling). The fog already
    // tints the underwater terrain that we sample from the refraction
    // texture, so it composes into the result via the refraction path.
    // Applying it again on the surface composite double-darkens / over-
    // tints the result into milky-cloudy territory.

    // Engine distance fog (`BumpWaterFS.glsl:350-352`): final per-fragment
    // mix toward atmospheric `fogColor`. Fog distances come from mapinfo
    // `atmosphere.fogStart/fogEnd * camera.far` packed into
    // `camera.fog_dists.xy`. For maps with closer fog onsets this darkens
    // distant water; for maps where the fog distance is unreachable in
    // the visible scene (Onyx Cauldron at our render scale), fog_factor
    // saturates to 1.0 and the stage is a no-op.
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

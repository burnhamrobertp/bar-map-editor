// SPDX-License-Identifier: GPL-3.0-or-later
//
// Fragment half of the BAR `map_grass_gl4` widget port. Concatenated
// with `map_grass_vs.wgsl` at pipeline-build time -- bindings and
// the `camera` / `shadow_u` uniforms are shared between the two files.
//
// Faithful 1:1 port of `bar-game/luaui/Shaders/map_grass_gl4
// .frag.glsl` modulo the gameplay-state knobs:
//   - LOS factor is hardcoded to 1.0 (no fog of war in BME).
//   - Night factor is hardcoded to vec3(1.0) (no day/night cycle).
//   - GRASSBRIGHTNESS is hardcoded to 1.0 (engine default).
//   - Distance-fade global multiplier (engine `grassuniforms.w`) is
//     baked into `in.fade` upstream by the VS.
//   - Fog blend is omitted here because BME's `widgets/custom_fog`
//     applies map-authored fog at the surrounding pipeline level.

/// `mapGrassColorModTex` -- BAR's `$grass` token, which resolves to
/// `grassShadingTex` (per-map override) or the engine's minimap
/// texture as a fallback (`SMFReadMap.cpp:313`). Critical detail:
/// this is NOT the high-resolution terrain albedo. It's typically
/// a 1024x1024 down-graded colour map. Sampling the albedo here
/// over-saturates the result by feeding bright per-tile detail into
/// the `* 2.0` blend on line 50 of the upstream frag shader.
@group(1) @binding(4) var grass_color_mod_tex: texture_2d<f32>;

struct FsIn {
    @location(0) uv: vec2<f32>,
    @location(1) world_xz: vec2<f32>,
    @location(2) fade: f32,
    @location(3) shade_factors: vec2<f32>,
    @location(4) base_world: vec3<f32>,
}

// ALPHATHRESHOLD, GRASSBRIGHTNESS, SHADOWFACTOR all live on
// `grass_params` (sourced from mapinfo `grassShaderParams`, with
// engine-stock defaults baked in `MapGrassWidget::default`).

/// Per-fragment pseudo-random number in [0, 1) for hashed alpha
/// testing (see the deviation comment in `fs_grass`). Keyed on a
/// *quantized* UV plus the patch's world XZ so:
///   (a) Wind sway producing sub-grid UV interpolation jitter
///       doesn't flicker the hash threshold frame-to-frame --
///       fragments within the same UV grid cell share a threshold,
///       so small UV motion stays in-cell. (Without quantization
///       the hash crawls visibly during wind animation -> reads
///       as constant TV-static.)
///   (b) Different blade instances at the same UV get decorrelated
///       discard patterns (world_xz term).
/// Grid resolution (64) chosen so the dither block size lands at
/// ~1-2 screen pixels at typical viewing distance -- coarse enough
/// to be stable, fine enough to still read as "per-pixel" noise.
fn grass_hash(uv: vec2<f32>, world_xz: vec2<f32>) -> f32 {
    let quantized_uv = floor(uv * 64.0);
    let quantized_world = floor(world_xz * 0.1);
    let p = quantized_uv + quantized_world;
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453);
}

/// Hardware-PCF shadow tap at the patch's base world position.
/// Comparison samplers can only be used in the fragment stage in
/// WGSL, so this lookup lives here rather than in the VS like in
/// the engine widget. The bind group itself is the same one terrain
/// and features use.
fn grass_shadow_factor(world_pos: vec3<f32>) -> f32 {
    let ls = shadow_u.light_view_proj * vec4<f32>(world_pos, 1.0);
    let ndc = ls.xyz / ls.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, -ndc.y * 0.5 + 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || ndc.z > 1.0) {
        return 1.0;
    }
    let bias = 0.0005;
    return textureSampleCompare(shadow_tex, shadow_samp, uv, ndc.z - bias);
}

@fragment
fn fs_grass(in: FsIn) -> @location(0) vec4<f32> {
    // Diagnostic mode driven at runtime from the viewport gear menu
    // (`bar-gui::ViewportDebug::grass_debug_output`, packed into
    // `grass_params.dbg.x`):
    //   0 = normal grass output
    //   1 = raw `map_color` (grassShadingTex sample)
    //   2 = raw blade-colour sample
    //   3 = post-blend rgb (before modulator)
    let debug_mode = i32(grass_params.dbg.x);
    // Alpha-test technique selector (`grass_params.dbg.y`):
    //   0 = hashed alpha (Wronski 2017 stochastic discard, BME default)
    //   1 = binary discard at ALPHATHRESHOLD only -- isolates whether
    //       the silhouette character comes from the hashed test vs
    //       the colour pipeline. Useful when chasing visual gaps
    //       against the in-engine widget, which uses MSAA + AtoC
    //       (impossible at sample_count=1) but produces a different
    //       look from hashed test under static viewing.
    let alpha_test_mode = i32(grass_params.dbg.y);

    // V-flip the blade UV at sample time. The mesh UVs were copied
    // verbatim from BAR's `grassPatches.lua` and assume OpenGL's
    // `uv.y=0 = bottom of texture` convention. wgpu uses
    // `uv.y=0 = top of texture`, so without this flip the texture
    // renders upside down: the solid colourful blade-base region of
    // the atlas lands at the rendered blade *tip* (visible against
    // the sky) and the alpha-feathered tip region lands at the base
    // (hidden against the ground). Visible side-effect was grass
    // appearing "almost white" -- the opaque base colours rendered
    // in the air. Only the texture sample needs flipping; the mesh
    // UV's role as "base vs tip" for wind shading
    // (`mix(1.0, shade_amount, in.uv.y)` in the VS) is unaffected.
    let blade_uv = vec2<f32>(in.uv.x, 1.0 - in.uv.y);
    var color = textureSample(blade_color_tex, blade_color_sam, blade_uv);

    // Sample `mapGrassColorModTex` at the patch's render-space XZ.
    // Engine convention: this is the per-map `grassShadingTex` (or
    // the engine's downsampled minimap when not specified). UV maps
    // [-x_extent, +x_extent] -> [0, 1] same as terrain. Filtered
    // (`textureSample` + mip chain on the upload side) so that
    // patches at typical viewing distance see averaged colour, the
    // way BAR's `texture(grassShadingTex, ...)` call does -- without
    // this, base-mip nearest sampling shows per-patch terrain
    // variation that the engine's filtered sample averages out.
    let map_uv = vec2<f32>(
        (in.world_xz.x / (2.0 * camera.x_extent)) + 0.5,
        (in.world_xz.y / (2.0 * camera.z_extent)) + 0.5,
    );
    let map_color = textureSample(grass_color_mod_tex, blade_color_sam, map_uv).rgb;

    // Engine-faithful early discard (frag.glsl:48). `ALPHATHRESHOLD`
    // is the user/mapinfo-controlled floor; anything strictly below
    // is fully transparent and shouldn't even pay for the hash
    // below.
    let alpha_threshold = grass_params.fade.w;
    if color.a < alpha_threshold {
        discard;
    }

    // *** Deviation from engine widget (see frag.glsl:50-57): ***
    //
    // BAR renders grass into an MSAA framebuffer with
    // `GL_SAMPLE_ALPHA_TO_COVERAGE` (see Recoil
    // `UnitDrawer.cpp:591` -- AtoC on whenever `msaaLevel >= 4`,
    // which is BAR's default). Under AtoC each sub-pixel sample is
    // independently kept or dropped based on alpha, so continuous
    // alpha in [ALPHATHRESHOLD, 1] produces SUB-PIXEL coverage --
    // each kept sample renders fully opaque and the MSAA resolve
    // anti-aliases blade silhouettes. Critically: a tapered tip with
    // alpha gradually falling from ~0.5 -> 0 keeps proportionally
    // fewer samples, so the blade *narrows* gracefully toward the
    // tip rather than terminating in a hard edge.
    //
    // BME's pipeline is `sample_count = 1`, no MSAA, no AtoC. A
    // faithful continuous-alpha + ALPHA_BLENDING render produces
    // translucent haloes around silhouettes AND dilutes the per-
    // fragment MAPCOLOR{FACTOR,BASE} blends with the terrain
    // colour beneath. A binary discard fixes those two problems
    // but butchers the tapered tip.
    //
    // The standard solution is hashed alpha testing (Wronski 2017):
    // each fragment is kept with probability = its alpha value.
    // Statistically reproduces AtoC's per-sample coverage at
    // sample_count=1.
    //
    // The hash threshold is biased to [0.1, 0.9] (was: full [0, 1]).
    // The narrowing is purely to anchor the solid-interior and
    // fully-transparent extremes so they don't flicker:
    //   alpha > 0.9 -> always kept (solid blade interior is
    //                  deterministic, no terrain dilution)
    //   alpha < 0.1 -> always discarded (no blob halo around
    //                  silhouette)
    //   alpha in [0.1, 0.9] -> stochastic
    // The dither band has to cover the full alpha-gradient region
    // of the blade silhouette + tip taper -- if it's too narrow,
    // pixels just outside the band become a hard cutoff and the
    // blade tip stops tapering (the engine's AtoC gives tapered
    // density across the WHOLE gradient range, not just a thin
    // band).
    //
    // Distance fade is kept on the output alpha channel (drives
    // the standard ALPHA_BLENDING state) so far-blade fade-out
    // remains smooth and not stochastic.
    if alpha_test_mode == 1 {
        // Binary discard: same as the engine widget's `fragColor.a <
        // ALPHATHRESHOLD` check on `frag.glsl:48`, no stochastic
        // smoothing. Useful as a diagnostic to confirm whether
        // stippling artefacts in normal output come from the hashed
        // test or from the colour pipeline. NOT recommended for
        // production -- produces hard-cut blade silhouettes at
        // sample_count=1, where the engine relies on MSAA + AtoC to
        // soften them.
    } else {
        let hash = grass_hash(in.uv, in.world_xz);
        let hash_threshold = 0.1 + hash * 0.8;
        if color.a < hash_threshold {
            discard;
        }
    }

    // Diagnostic short-circuits BEFORE blend math: confirms what the
    // raw samples look like to the FS. If raw `map_color` doesn't
    // read red when `grassShadingTex` is a bright-red file, the
    // problem is upstream of the FS (sampling, binding, upload).
    if debug_mode == 1 {
        return vec4<f32>(map_color, 1.0);
    }
    if debug_mode == 2 {
        return vec4<f32>(color.rgb, 1.0);
    }

    // --- frag.glsl:50-51 ---
    let mc_factor = grass_params.blend.x;
    let mc_base = grass_params.blend.y;
    var rgb = mix(color.rgb, color.rgb * (map_color * 2.0), mc_factor);
    rgb = mix(rgb, map_color, (1.0 - in.uv.y) * mc_base);

    if debug_mode == 3 {
        return vec4<f32>(rgb, 1.0);
    }

    // --- frag.glsl:55 ---
    // RGB *= shadow * LOS * wind_noise * GRASSBRIGHTNESS.
    // shade_factors = (LOS=1.0, wind_shade) from the VS;
    // grass_params.fade.z = SHADOWFACTOR floor.
    // grass_params.blend.w = GRASSBRIGHTNESS.
    let shadow_floor = grass_params.fade.z;
    let shadow = clamp(grass_shadow_factor(in.base_world), shadow_floor, 1.0);
    let brightness = grass_params.blend.w;
    let modulator = shadow
        * in.shade_factors.x
        * in.shade_factors.y
        * brightness;

    // Engine `frag.glsl:57` -- alpha contrast clamp:
    //   `fragColor.a = clamp((a - 0.5) * 1.5 + 0.5, 0, 1)`.
    // Sharpens the distance-fade transition band so blades fading
    // out at the FADESTART->FADEEND boundary don't show a long
    // translucent tail. With our binary hash test handling the
    // silhouette/coverage, this clamp now only affects the
    // distance-fade alpha (since per-pixel coverage has already
    // been resolved via discard above).
    let faded_alpha = clamp((in.fade - 0.5) * 1.5 + 0.5, 0.0, 1.0);

    // Engine-faithful output: blade tip carries its full authored
    // alpha + colour. The earlier `upper_dim` stopgap (50% smoothstep
    // dim above uv.y=0.6) is gone -- the proper engine-equivalent
    // tapering is the per-fragment hashed alpha test above, which
    // statistically reproduces the in-engine AtoC's sub-pixel
    // coverage across the blade's authored alpha gradient.
    return vec4<f32>(rgb * modulator, faded_alpha);
}

// SPDX-License-Identifier: GPL-3.0-or-later
//
// BAR-widget effect: instanced animated grass blades.
// Faithful port of `bar-game/luaui/Shaders/map_grass_gl4.vert.glsl`.
//
// Items kept 1:1 with the engine widget:
//   - Per-instance world position, rotation, size (`instancePosRotSize`).
//   - 6-blade patch mesh from `grassPatches.lua` (geometrydata[4]).
//   - Per-vertex heightmap sampling at the rotated world XZ.
//   - Y placement formula `(vertexPos.y + 0.5) * size + groundHeight`.
//   - Wind-noise position perturbation at the blade tip.
//   - Wind-noise shading factor (darkens the tip more than the base).
//   - Shadow PCF (4-tap), clamped to `[SHADOWFACTOR, 1.0]`.
//   - Distance fade with `distanceMult = 0.45`.
//
// Items deliberately dropped (and why):
//   - LOS texture: gameplay state. Hardcoded to 1.0.
//   - Unit bending: no units in the editor.
//   - Night factor: no time-of-day in the editor.
//   - Fog blend: lives in `widgets/custom_fog.wgsl`, applied outside
//     this widget by the surrounding pipeline.
//
// Wind noise is generated procedurally with a hash function instead
// of sampling `grassWindPerturbTex`. The engine's perlin texture
// would be a single 256x256 RGBA sample; the hash is cheap and
// produces a comparable per-fragment variance pattern without
// shipping another asset.

struct CameraUniform {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    camera_pos: vec3<f32>,
    has_texture: u32,
    height_scale: f32,
    water_r: f32,
    water_g: f32,
    water_b: f32,
    water_y: f32,
    time: f32,
    skip_water: f32,
    height_range_elmos: f32,
    screen_w: f32,
    screen_h: f32,
    x_extent: f32,
    z_extent: f32,
}

@group(0) @binding(0) var<uniform> camera: CameraUniform;
// Group 0 also has the terrain skybox cubemap at bindings 1 and 2;
// we don't sample either but must declare them so the bind group
// layout matches `TerrainRenderer::camera_bind_group_layout` and
// `camera_bind_group` can be bound to the grass pipeline too.
@group(0) @binding(1) var _grass_unused_skybox_tex: texture_cube<f32>;
@group(0) @binding(2) var _grass_unused_skybox_sam: sampler;

@group(1) @binding(0) var blade_color_tex: texture_2d<f32>;
@group(1) @binding(1) var blade_color_sam: sampler;
@group(1) @binding(2) var grass_heightmap_tex: texture_2d<f32>;
/// BAR's `grassWindPerturbTex` (`bitmaps/gpl/perlin_noise.jpg`),
/// bundled into BME at build time. RGBA8 perlin noise with the
/// engine's hand-tuned channel statistics. Sampled with `Repeat`
/// addressing so the tile wraps endlessly across the map.
@group(1) @binding(5) var grass_wind_perturb_tex: texture_2d<f32>;
@group(1) @binding(6) var grass_wind_perturb_sam: sampler;

/// Per-pipeline tuning packed into three vec4s. Mirrors BAR widget's
/// `grassShaderParams` (`map_grass_gl4.lua:93-110`); all defaults
/// match the widget verbatim.
/// blend (slot 0):
///   x = MAPCOLORFACTOR -- terrain-colour multiplicative blend.
///   y = MAPCOLORBASE -- extra blend at the blade base.
///   z = WINDSTRENGTH -- sway magnitude.
///   w = GRASSBRIGHTNESS -- final RGB multiplier (FS).
/// fade (slot 1):
///   x = FADESTART (elmos).
///   y = FADEEND (elmos).
///   z = SHADOWFACTOR -- shadow-multiplier floor.
///   w = ALPHATHRESHOLD -- alpha discard cutoff (FS).
/// wind (slot 2):
///   x = WINDSCALE -- multiplier on the wind-drift offset added
///       to the noise sample position. Higher = the noise pattern
///       drifts faster.
///   y = WINDSAMPLESCALE -- multiplier on the world-XZ sample
///       position. Smaller = broader-grained wind gusts.
///   z = grassWindMult -- speed at which the drift offset
///       advances per second (combines with WINDSCALE).
///   w = unused / padding.
struct GrassParams {
    blend: vec4<f32>,
    fade: vec4<f32>,
    wind: vec4<f32>,
}
@group(1) @binding(3) var<uniform> grass_params: GrassParams;

/// Shadow receiver group -- shared with terrain + features at the
/// same `group(2)` binding slot. WGSL forbids comparison-sampler use
/// in the vertex stage, so the actual `textureSampleCompare` call
/// lives in the fragment shader; the VS only passes the world
/// position through. `shadow_u` is declared here because the WGSL
/// module is shared with the FS via the concat in `map_grass.rs`.
struct ShadowUniform {
    light_view_proj: mat4x4<f32>,
    sun_dir: vec4<f32>,
}
@group(2) @binding(0) var<uniform> shadow_u: ShadowUniform;
@group(2) @binding(1) var shadow_tex: texture_depth_2d;
@group(2) @binding(2) var shadow_samp: sampler_comparison;

/// Wind noise sampled from BAR's bundled perlin texture. Engine
/// widget at `vert.glsl:140-143`:
///   vec4 grassNoise = texture(grassWindPerturbTex, ...);
///   grassNoise = (grassNoise - 0.5).xzyw;
/// We replicate the recenter + swizzle so the returned vec3 matches
/// the engine's `(noise - 0.5).xzy` orientation (perturbation XYZ
/// reads engine.x, engine.z, engine.y respectively).
fn wind_noise(sample_pos: vec2<f32>) -> vec3<f32> {
    // Return RAW perlin samples in [0, 1] (no `-0.5` centering).
    // The engine widget consumes these values uncentered in both
    // sites that use them:
    //   - `shadeamount = (grassNoise.y * 2.0 - 0.66) * 3.0` expects
    //     raw [0,1] mean ~0.5, producing shade_amount mean ~1.02
    //     (mild brightening). Pre-centering biased the mean to ~0
    //     and pushed shade_amount to ~-1.98, multiplying the blade
    //     RGB by a negative number that clamped to black on the GPU
    //     -- a major brightness deficit vs in-engine rendering.
    //   - Position offset (`grassNoise.rgb * vertexPos.y * ...`) is
    //     added directly to world position; the engine's raw +0.5
    //     mean produces consistent drift the rest of the formula
    //     scales down via `vertexPos.y`. Pre-centering removed that
    //     drift and made the per-vertex offset alternate sign.
    //
    // Component swizzle (.x, .z, .y) matches the engine widget's
    // texture component order on read.
    let raw = textureSampleLevel(
        grass_wind_perturb_tex,
        grass_wind_perturb_sam,
        sample_pos,
        0.0,
    );
    return vec3<f32>(raw.x, raw.z, raw.y);
}

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    // Per-instance: (world_x, rotation, world_z, size).
    @location(2) instance: vec4<f32>,
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) world_xz: vec2<f32>,
    @location(2) fade: f32,
    /// Engine `instanceParamsVS.zw` -- LOS (hardcoded 1.0 in BME)
    /// and wind-noise shading (`shadeamount` gated by uv.y). The
    /// FS multiplies these into the RGB modulator. Shadow can't be
    /// computed here -- WGSL forbids comparison samplers in VS --
    /// so we pass the patch-base world position separately for the
    /// FS to do its own PCF tap.
    @location(3) shade_factors: vec2<f32>,
    /// Patch base world position (XZ at instance centre, Y at the
    /// sampled ground). Used by the FS to do the shadow-map
    /// comparison sample. Passing the BASE rather than the per-
    /// fragment world position keeps every fragment of a patch on
    /// the same shadow sample so the patch reads as one consistent
    /// lit/shaded unit -- mirrors how BAR's 4-tap PCF at the
    /// instance centre paints the whole patch uniformly.
    @location(4) base_world: vec3<f32>,
}

@vertex
fn vs_grass(in: VsIn) -> VsOut {
    var out: VsOut;
    let size_elmos = in.instance.w;
    if size_elmos <= 0.0 {
        out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        out.uv = in.uv;
        out.world_xz = vec2<f32>(0.0);
        out.fade = 0.0;
        out.shade_factors = vec2<f32>(0.0);
        out.base_world = vec3<f32>(0.0);
        return out;
    }

    // Clip-space frustum cull at the patch centre, matching
    // `vert.glsl:80-85`. Project the instance origin and reject
    // if its post-projective X falls outside `[-1.1, 1.1]`. The
    // 0.1 slack absorbs patches whose blades extend past the
    // centre and would still be visible after the per-vertex
    // displacement. Reduces fragment cost on maps with grass
    // covering the whole playable area but only a fraction
    // on-screen at any time.
    let instance_world = vec4<f32>(in.instance.x, 0.0, in.instance.z, 1.0);
    let instance_clip = camera.view_proj * instance_world;
    if abs(instance_clip.x / instance_clip.w) > 1.1 {
        out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        out.uv = in.uv;
        out.world_xz = vec2<f32>(0.0);
        out.fade = 0.0;
        out.shade_factors = vec2<f32>(0.0);
        out.base_world = vec3<f32>(0.0);
        return out;
    }

    let elmo_to_render = camera.height_scale / max(camera.height_range_elmos, 1.0);

    // --- Engine recipe (vert.glsl:93-104) ---
    // 1. scale mesh by size
    // 2. rotate around Y (XZ plane only)
    // 3. translate by instance.xz
    // 4. heightmap sample at rotated world XZ
    // 5. world_pos.y = (vertexPos.y + 0.5) * size + groundHeight
    let size_render = size_elmos * elmo_to_render;
    let scaled = in.pos * size_render;
    let cos_r = cos(in.instance.y);
    let sin_r = sin(in.instance.y);
    let rotated_xz = vec2<f32>(
        scaled.x * cos_r - scaled.z * sin_r,
        scaled.x * sin_r + scaled.z * cos_r,
    );
    let world_xz_render = vec2<f32>(in.instance.x, in.instance.z);
    let vertex_xz_render = world_xz_render + rotated_xz;
    // `vert.glsl:102` clamps the heightmap-sample XZ 8 elmos in
    // from each edge so blades whose centres land on the very
    // border don't snap to extrapolated heightmap values. We do
    // the same in render space (8 elmos converted to render units).
    let edge_clamp_render = 8.0 * elmo_to_render;
    let clamped_xz_render = vec2<f32>(
        clamp(
            vertex_xz_render.x,
            -camera.x_extent + edge_clamp_render,
            camera.x_extent - edge_clamp_render,
        ),
        clamp(
            vertex_xz_render.y,
            -camera.z_extent + edge_clamp_render,
            camera.z_extent - edge_clamp_render,
        ),
    );
    let hm_uv = vec2<f32>(
        (clamped_xz_render.x / (2.0 * camera.x_extent)) + 0.5,
        (clamped_xz_render.y / (2.0 * camera.z_extent)) + 0.5,
    );
    let dim = vec2<i32>(textureDimensions(grass_heightmap_tex));
    let dim_f = vec2<f32>(dim);
    let tc = clamp(vec2<i32>(hm_uv * dim_f), vec2<i32>(0), dim - vec2<i32>(1));
    let ground_y = textureLoad(grass_heightmap_tex, tc, 0).r * camera.height_scale;
    let lifted_y_elmos = (in.pos.y + 0.5) * size_elmos;
    var world_pos = vec3<f32>(
        vertex_xz_render.x,
        ground_y + lifted_y_elmos * elmo_to_render,
        vertex_xz_render.y,
    );

    // Wind-noise sampling. Engine widget at `vert.glsl:140`:
    //   texture(grassWindPerturbTex,
    //           (grassVertWorldPos.xz + grassuniforms.xy*WINDSCALE)
    //           * WINDSAMPLESCALE)
    // `grassuniforms.xy = (offsetX, offsetZ)` is the globally-advancing
    // drift that integrates `windDir * grassWindMult * dt` every tick
    // (`map_grass_gl4.lua:1261-1265`). The drift translates the noise
    // pattern across the map, producing the "wind gusts blowing
    // across the map" appearance rather than "blades shaking in
    // place". BME synthesises the drift -- no gameplay wind state
    // available, so a fixed unit direction advances at
    // `grass_wind_mult` per second using `camera.time`. The
    // per-vertex sample position uses world XZ in elmos so the
    // statistics align with the engine.
    let vertex_xz_elmos = vertex_xz_render / elmo_to_render;
    let wind_dir = normalize(vec2<f32>(0.7, 0.7));
    let wind_drift = wind_dir * grass_params.wind.z * camera.time;
    let sample_pos = (vertex_xz_elmos + wind_drift * grass_params.wind.x)
        * grass_params.wind.y;
    let noise = wind_noise(sample_pos);

    // Wind shadeamount -- engine widget `vert.glsl:146-147` verbatim:
    //   shadeamount = (noise.y*2 - 0.66) * 3
    //   wind_shade  = mix(1.0, shadeamount, uv.y)
    // Now that `wind_noise` returns RAW [0, 1] perlin samples (no
    // centering), `noise.y` has mean ~0.5 and shade_amount has
    // mean ~1.02, matching the engine. Range across typical perlin
    // values lands roughly in [-1.5, +4.0], so wind_shade at the
    // tip varies between mild darkening and bright highlights as
    // the noise drifts -- the visual "wind ripple" effect.
    let shade_amount = (noise.y * 2.0 - 0.66) * 3.0;
    let wind_shade = mix(1.0, shade_amount, in.uv.y);

    // Patch-base world position for the FS to do its own shadow
    // PCF lookup. Sampling the BASE (not per-vertex) keeps every
    // fragment of the patch on one shadow value, matching the
    // engine's "PCF at instance centre paints whole patch" effect.
    // Captured BEFORE wind perturbation below so shadows anchor to
    // the ground, not the swaying tip.
    let base_world = vec3<f32>(world_pos.x, ground_y, world_pos.z);

    // Wind position perturbation -- engine widget `vert.glsl:153`:
    //   grassVertWorldPos += grassNoise.rgb * vertexPos.y
    //                        * instancePosRotSize.w * WINDSTRENGTH
    //                        * grassuniforms.z
    // where `grassuniforms.z` is the lua-clamped wind magnitude
    // (`map_grass_gl4.lua:1289` -- `mathMax(4.0, |windDirX| + |windDirZ|)`).
    //
    // BME has no gameplay wind state, so we use the engine's clamp
    // floor (4.0) as a constant for `grassuniforms.z`. The
    // `wind_noise` function is calibrated to land in BAR's +-0.1
    // amplitude range so this constant + the user's `WINDSTRENGTH`
    // produce engine-magnitude sway directly.
    //
    // Engine's `grassNoise.y -= 0.4` is applied to the RAW [0, 1]
    // perlin sample, so after the shift the value ranges roughly
    // [-0.4, 0.6] (mild downward bias = blades droop slightly under
    // wind). `wind_noise` now returns raw values, so we apply the
    // same `-0.4` directly.
    //
    // `in.pos.y` ranges [-0.0012, +9.04] across the patch mesh
    // (base -> tip), so the offset scales by vertex height,
    // anchoring blade bases to the ground while tips sway.
    let wind_speed_floor = 4.0;
    let noise_offset = vec3<f32>(noise.x, noise.y - 0.4, noise.z);
    let wind_xyz_elmos = noise_offset
        * in.pos.y
        * size_elmos
        * grass_params.blend.z
        * wind_speed_floor;
    world_pos = world_pos + wind_xyz_elmos * elmo_to_render;

    // --- Distance fade (vert.glsl:199-201) ---
    // Engine: `clamp((FADEEND*distanceMult - dist) / ((FADEEND -
    // FADESTART)*distanceMult), 0, 1)`. Both endpoints come from
    // `grass_params.fade.xy` in elmos; multiplied by `elmo_to_render`
    // to land in BME render units, and by `distanceMult = 0.45`
    // (`map_grass_gl4.lua:122`) to match the engine's effective fade
    // distance.
    let distance_mult = 0.45;
    let to_cam = camera.camera_pos - world_pos;
    let dist = length(to_cam);
    let fade_end_render = max(grass_params.fade.y * elmo_to_render * distance_mult, 1e-4);
    let fade_start_render = grass_params.fade.x * elmo_to_render * distance_mult;
    let fade = clamp(
        (fade_end_render - dist) / max(fade_end_render - fade_start_render, 1e-4),
        0.0,
        1.0,
    );

    out.clip = camera.view_proj * vec4<f32>(world_pos, 1.0);
    out.uv = in.uv;
    out.world_xz = world_xz_render;
    out.fade = fade;
    // (LOS = 1.0, wind_shade). Shadow lives in the FS.
    out.shade_factors = vec2<f32>(1.0, wind_shade);
    out.base_world = base_world;
    return out;
}

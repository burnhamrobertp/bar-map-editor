// SPDX-License-Identifier: GPL-2.0-or-later
// Feature vertex/fragment shader for placeholder cubes and real S3O models.
//
// Lighting is sourced from the same `CameraUniform` the terrain pipeline uses,
// so features react to the map's `mapinfo.lua` `lighting` table identically
// to the ground. Shadow sampling lives in group 2 (see `shadow.wgsl`).
//
// S3O texture convention (cont/base/springcontent/shaders/GLSL/ModelFragProg.glsl):
//   texture1.rgb = diffuse color
//   texture1.a   = team-color mask (ignored for map-feature gaia team)
//   texture2.rgb = glow / specular / reflectivity
//   texture2.a   = opacity / cutout mask
// Output alpha = texture2.a; sub-threshold discard keeps tree-leaf holes from
// writing depth.

// Layout-matches `CameraUniform` in `crates/bar-render/src/renderer.rs`. We
// only read the fields we need (view_proj, camera_pos, sun_dir_exp,
// ground_*), but the full struct is mirrored so byte offsets line up.
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
    _pad0: f32,
    screen_w: f32,
    screen_h: f32,
    x_extent: f32,
    z_extent: f32,
    sun_dir_exp: vec4<f32>,
    ground_ambient: vec4<f32>,
    ground_diffuse: vec4<f32>,
    ground_specular: vec4<f32>,
    water_absorb: vec4<f32>,
    water_base_color: vec4<f32>,
    water_min_color: vec4<f32>,
    brush_cursor: vec4<f32>,
    clip_plane: vec4<f32>,
}

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(1) @binding(0) var diffuse_tex:  texture_2d<f32>;
@group(1) @binding(1) var shading_tex:  texture_2d<f32>;
@group(1) @binding(2) var tex_samp:     sampler;

// Shadow map sampling -- see crates/bar-render/src/shadow.rs.
struct ShadowUniform {
    light_view_proj: mat4x4<f32>,
    /// World-space sun direction (matches camera.sun_dir_exp.xyz). Currently
    /// unused by feature shader but kept aligned with the terrain side so a
    /// single buffer feeds both.
    sun_dir: vec4<f32>,
}
@group(2) @binding(0) var<uniform> shadow_u: ShadowUniform;
@group(2) @binding(1) var shadow_tex: texture_depth_2d;
@group(2) @binding(2) var shadow_samp: sampler_comparison;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(7) uv:       vec2<f32>,
}

// Per-instance data: 4x4 column-major model transform + RGBA tint (80 bytes).
struct InstanceInput {
    @location(2) col0: vec4<f32>,
    @location(3) col1: vec4<f32>,
    @location(4) col2: vec4<f32>,
    @location(5) col3: vec4<f32>,
    @location(6) tint: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_pos:   vec4<f32>,
    @location(0) world_pos:        vec3<f32>,
    @location(1) world_norm:       vec3<f32>,
    @location(2) uv:               vec2<f32>,
    @location(3) tint:             vec4<f32>,
}

@vertex
fn vs_feature(vert: VertexInput, inst: InstanceInput) -> VertexOutput {
    let model      = mat4x4<f32>(inst.col0, inst.col1, inst.col2, inst.col3);
    let world4     = model * vec4<f32>(vert.position, 1.0);
    let world_pos  = world4.xyz;
    let world_norm = normalize((model * vec4<f32>(vert.normal, 0.0)).xyz);

    var out: VertexOutput;
    out.clip_pos   = camera.view_proj * world4;
    out.world_pos  = world_pos;
    out.world_norm = world_norm;
    out.uv         = vert.uv;
    out.tint       = inst.tint;
    return out;
}

/// 1.0 = fully lit, 0.0 = fully shadowed. Uses hardware 2x2 PCF via the
/// comparison sampler: `textureSampleCompare` does the depth-vs-reference
/// comparison at four bilinearly-weighted texels and returns the weighted
/// average. Sharp at the geometric silhouette, smooth across a single texel
/// edge -- the right shape for small isolated features (no fuzz disconnecting
/// them from their bases).
fn shadow_factor(world_pos: vec3<f32>) -> f32 {
    let ls = shadow_u.light_view_proj * vec4<f32>(world_pos, 1.0);
    let ndc = ls.xyz / ls.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, -ndc.y * 0.5 + 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || ndc.z > 1.0) {
        return 1.0;
    }
    let bias = 0.0003;
    return textureSampleCompare(shadow_tex, shadow_samp, uv, ndc.z - bias);
}

@fragment
fn fs_feature(in: VertexOutput) -> @location(0) vec4<f32> {
    // Honor the camera clip plane: reflection and refraction pre-passes set
    // `clip_plane` to keep one half-space (above-water or below-water); the
    // main pass leaves it at (0, 0, 0, 1) so everything passes.
    if (dot(camera.clip_plane.xyz, in.world_pos) + camera.clip_plane.w < 0.0) {
        discard;
    }

    let diffuse_sample = textureSample(diffuse_tex, tex_samp, in.uv);
    let shading_sample = textureSample(shading_tex, tex_samp, in.uv);

    // Cutout: discard nearly fully transparent texels so leaf-card holes are
    // not written into the depth buffer (would block features behind them).
    if (shading_sample.a < 0.05) {
        discard;
    }

    let sun_dir  = normalize(camera.sun_dir_exp.xyz);
    let normal   = normalize(in.world_norm);
    let view_dir = normalize(camera.camera_pos - in.world_pos);
    let shadow   = shadow_factor(in.world_pos);

    // Identical shading math to terrain (smf_ground.wgsl::smf_ground_shade).
    // Including the SMF intensity multiplier 210/255 so feature brightness
    // matches ground brightness 1:1 -- otherwise features end up ~20% hotter
    // than the terrain they sit on. Specular is added on top of the
    // texture multiply (engine match -- see terrain.wgsl for the rationale).
    let lit = smf_ground_shade(
        in.world_pos,
        normal,
        sun_dir,
        camera.ground_ambient.xyz,
        camera.ground_diffuse.xyz,
        shadow,
    );
    let spec_term = smf_specular(
        normal,
        sun_dir,
        view_dir,
        camera.ground_specular.xyz,
        camera.sun_dir_exp.w,
        shadow,
    );

    // S3O texture2 channels -- per ModelFragProg.glsl:87-109:
    //   .r => self-illumination / emissive, added to the lighting multiplier
    //         so the textured RGB takes the boost (so the glow ends up the
    //         feature's own colour, not a white wash).
    //   .g => specular intensity multiplier. Engine multiplies spec by
    //         `extraColor.g * 4.0` and additionally mixes env reflection by
    //         the same channel; we don't have env reflection so we just
    //         apply the spec multiplier.
    // Without these, the emissive crystal / glow-mushroom features that BAR
    // ships on Azurite Shores etc. render as dead matte geometry instead of
    // the cyan / purple halos seen in-engine.
    //
    // Placeholders and models with no tex2 bind a (0,0,0,1) shading default
    // (see `feature_default_shading_view` in features.rs) so this path is a
    // no-op contribution when shading data is absent.
    let emissive  = vec3<f32>(shading_sample.r);
    let spec_mult = shading_sample.g * 4.0;

    // Tint is (1,1,1,1) for loaded models so the texture passes through
    // untouched; selected features get a yellow tint; placeholders use the
    // catalog-known / unknown colors.
    let rgb = diffuse_sample.rgb * in.tint.rgb * (lit + emissive) + spec_term * spec_mult;
    return vec4<f32>(rgb, shading_sample.a * in.tint.a);
}

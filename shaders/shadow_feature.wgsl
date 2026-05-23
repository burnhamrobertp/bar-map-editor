// SPDX-License-Identifier: GPL-2.0-or-later
// Depth-only feature caster pass. Transforms instanced S3O vertices into
// light space and writes only depth into the shadow map. Mirrors the input
// layout of `features.wgsl::vs_feature` so the same vertex / instance
// buffers feed both pipelines.

struct ShadowUniform {
    light_view_proj: mat4x4<f32>,
    sun_dir: vec4<f32>,
}
@group(0) @binding(0) var<uniform> shadow_u: ShadowUniform;

// Per-mesh diffuse + opacity textures, shared with the main feature pipeline
// so the cutout-discard path is available here too (otherwise a tree's
// transparent leaf-card holes would still write depth and block other features).
@group(1) @binding(0) var diffuse_tex: texture_2d<f32>;
@group(1) @binding(1) var shading_tex: texture_2d<f32>;
@group(1) @binding(2) var tex_samp:    sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(7) uv:       vec2<f32>,
}

struct InstanceInput {
    @location(2) col0: vec4<f32>,
    @location(3) col1: vec4<f32>,
    @location(4) col2: vec4<f32>,
    @location(5) col3: vec4<f32>,
    @location(6) tint: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv:             vec2<f32>,
}

@vertex
fn vs_shadow_feature(vert: VertexInput, inst: InstanceInput) -> VertexOutput {
    let model = mat4x4<f32>(inst.col0, inst.col1, inst.col2, inst.col3);
    let world4 = model * vec4<f32>(vert.position, 1.0);
    var out: VertexOutput;
    out.clip_pos = shadow_u.light_view_proj * world4;
    out.uv = vert.uv;
    return out;
}

@fragment
fn fs_shadow_feature(in: VertexOutput) {
    // Match the main FS's cutout: don't write depth for fully transparent
    // texels, otherwise leaf-card holes would cast (incorrect) hard shadows.
    let shading = textureSample(shading_tex, tex_samp, in.uv);
    if (shading.a < 0.05) {
        discard;
    }
}

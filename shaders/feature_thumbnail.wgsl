// Minimal pipeline for the feature-palette thumbnails. Draws one S3O
// model into a small offscreen texture with a fixed three-quarter-view
// camera and simple Lambert + ambient lighting. Output is registered
// as an egui texture (Rgba8UnormSrgb so egui's sRGB sampling matches).

struct Uniforms {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    sun_dir: vec3<f32>,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var diffuse_tex: texture_2d<f32>;
@group(0) @binding(2) var diffuse_sam: sampler;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(2) uv:       vec2<f32>,
};

struct VsOut {
    @builtin(position) clip:    vec4<f32>,
    @location(0) world_normal:  vec3<f32>,
    @location(1) uv:            vec2<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    let world = u.model * vec4<f32>(in.position, 1.0);
    out.clip = u.view_proj * world;
    out.world_normal = normalize((u.model * vec4<f32>(in.normal, 0.0)).xyz);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let albedo = textureSample(diffuse_tex, diffuse_sam, in.uv);
    let n = normalize(in.world_normal);
    let l = normalize(u.sun_dir);
    let lambert = max(dot(n, l), 0.0);
    // Wrap-around half-Lambert + ambient so the back of the model
    // still reads at thumbnail scale.
    let lit = lambert * 0.7 + 0.4;
    return vec4<f32>(albedo.rgb * lit, 1.0);
}

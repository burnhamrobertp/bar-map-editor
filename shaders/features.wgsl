// SPDX-License-Identifier: GPL-2.0-or-later
// Feature placeholder (unit cube) vertex/fragment shader.
// Shares group 0 / binding 0 camera uniform with the terrain pipeline;
// only view_proj is read (first 64 bytes of the 336-byte CameraUniform).

struct Camera {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> camera: Camera;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
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
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_feature(vert: VertexInput, inst: InstanceInput) -> VertexOutput {
    let model      = mat4x4<f32>(inst.col0, inst.col1, inst.col2, inst.col3);
    let world_pos  = model * vec4<f32>(vert.position, 1.0);
    let world_norm = normalize((model * vec4<f32>(vert.normal, 0.0)).xyz);

    // Simple diffuse + ambient from a fixed sun direction.
    let sun     = normalize(vec3<f32>(0.4, 1.0, 0.6));
    let diffuse = max(dot(world_norm, sun), 0.0) * 0.6 + 0.4;

    var out: VertexOutput;
    out.clip_pos = camera.view_proj * world_pos;
    out.color    = vec4<f32>(inst.tint.rgb * diffuse, inst.tint.a);
    return out;
}

@fragment
fn fs_feature(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}

// SPDX-License-Identifier: GPL-2.0-or-later
// Editor-only placeholder cubes for feature instances that don't have a
// loaded S3O model (catalog-known but no asset, or unknown / invalid type).
//
// Deliberately outside the main feature shading pipeline: no map-driven
// lighting, no fog, no shadow sampling, no env reflection, no team color.
// The whole point of these cubes is to be a *diagnostic marker* that reads
// the same regardless of where the user has aimed the sun or how thick the
// map's fog is. A faint normal-driven face shade is applied so the cube
// still reads as 3D, but it's anchored to the cube's local up axis -- the
// only "lighting input" is the cube's own geometry.

struct CameraUniform {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> camera: CameraUniform;

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
    @location(0) tint:           vec4<f32>,
    @location(1) face_shade:     f32,
}

@vertex
fn vs_placeholder(vert: VertexInput, inst: InstanceInput) -> VertexOutput {
    let model  = mat4x4<f32>(inst.col0, inst.col1, inst.col2, inst.col3);
    let world4 = model * vec4<f32>(vert.position, 1.0);

    // Fixed face shading: bright top, medium sides, dark bottom. Driven by
    // the cube's world-space normal Y component, NOT the map's sun
    // direction -- placeholders must read identically regardless of the
    // map's lighting configuration.
    let world_norm = normalize((model * vec4<f32>(vert.normal, 0.0)).xyz);
    let face_shade = 0.7 + 0.3 * world_norm.y;

    var out: VertexOutput;
    out.clip_pos   = camera.view_proj * world4;
    out.tint       = inst.tint;
    out.face_shade = face_shade;
    return out;
}

@fragment
fn fs_placeholder(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.tint.rgb * in.face_shade, in.tint.a);
}

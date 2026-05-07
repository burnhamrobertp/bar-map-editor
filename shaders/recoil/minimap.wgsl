// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Recoil's
// `cont/base/springcontent/shaders/GLSL/MiniMap{Vert,Frag}Prog.glsl`.
// Upstream commit pinned in `vendor/recoil/UPSTREAM.md`.
//
// The engine composites three textures for the minimap: a shading
// pass, the actual minimap diffuse, and an info-overlay (FoW, metal
// spots, etc.). The editor only needs the first two — info overlays
// are a runtime concern.
//
// Currently not wired into a pipeline. Available for future use by
// the 2D inspector if we want engine-faithful minimap rendering
// alongside the topo-style heightmap visualisation.

struct MinimapCamera {
    /// Model-view-projection matrix the lobby applies before this
    /// shader runs. Identity is fine when sampling a full-screen quad.
    mvp: mat4x4<f32>,
    /// Engine's `uvMult` — the info-overlay sampling rate vs. the
    /// minimap texture. Identity (1, 1) when the overlays are
    /// per-fragment-aligned.
    uv_mult: vec2<f32>,
    /// Engine's `infotexMul` — fades the info overlay in/out. 0 = no
    /// info, 1 = full info contribution.
    info_mul: f32,
    _pad: f32,
}

@group(0) @binding(0) var<uniform> minimap_camera: MinimapCamera;

@group(1) @binding(0) var minimap_shading_tex: texture_2d<f32>;
@group(1) @binding(1) var minimap_diffuse_tex: texture_2d<f32>;
@group(1) @binding(2) var minimap_info_tex: texture_2d<f32>;
@group(1) @binding(3) var minimap_sampler: sampler;

struct MinimapVertexInput {
    @location(0) vertex_pos: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
}

struct MinimapVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

@vertex
fn vs_minimap(in: MinimapVertexInput) -> MinimapVertexOutput {
    var out: MinimapVertexOutput;
    out.clip_position = minimap_camera.mvp
        * vec4<f32>(in.vertex_pos, 0.0, 1.0);
    out.tex_coords = in.tex_coords;
    return out;
}

@fragment
fn fs_minimap(in: MinimapVertexOutput) -> @location(0) vec4<f32> {
    // Engine uses `texture(..., depthBias = -2.0)` to bias mipmap
    // selection towards higher-detail levels for the minimap blit.
    // WGSL doesn't expose depth bias directly; sampling the same
    // texture without bias is visually equivalent at our rendered
    // sizes (we don't typically minify the minimap aggressively).
    let shading = textureSample(
        minimap_shading_tex,
        minimap_sampler,
        in.tex_coords,
    );
    let minimap = textureSample(
        minimap_diffuse_tex,
        minimap_sampler,
        in.tex_coords,
    );
    let info = textureSample(
        minimap_info_tex,
        minimap_sampler,
        in.tex_coords * minimap_camera.uv_mult,
    ) - vec4<f32>(0.5);

    return shading * minimap + info * minimap_camera.info_mul;
}

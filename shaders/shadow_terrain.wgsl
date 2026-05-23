// SPDX-License-Identifier: GPL-2.0-or-later
// Depth-only terrain caster pass. Replicates the displacement done by the
// main terrain VS (terrain.wgsl::vs_main) but renders from the sun's POV into
// the shadow map.
//
// Skirt vertices (uv.y > 1.5) and water plane vertices (uv.x < -0.5) are
// discarded -- they aren't part of the shadow-casting silhouette.

// Layout-matches `CameraUniform` -- we only read x_extent/z_extent/height_scale,
// but the buffer is shared with the main pass.
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

struct ShadowUniform {
    light_view_proj: mat4x4<f32>,
    sun_dir: vec4<f32>,
}

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(1) @binding(0) var<uniform> shadow_u: ShadowUniform;
@group(2) @binding(0) var heightmap_tex: texture_2d<f32>;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(2) uv:       vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) skip:           f32,
}

@vertex
fn vs_shadow_terrain(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    // Skirt / water-plane vertices: marked skip; the FS discards them.
    if (in.uv.y > 1.5 || in.uv.x < -0.5) {
        out.clip_pos = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        out.skip = 1.0;
        return out;
    }
    // Reproduce vs_main's displacement: flat-grid input position is in
    // [-0.5, 0.5]^2 on x/z; heightmap sampled by uv; y = h * height_scale.
    let dim_i = vec2<i32>(textureDimensions(heightmap_tex));
    let dim_f = vec2<f32>(dim_i);
    let tc = clamp(vec2<i32>(in.uv * dim_f), vec2<i32>(0), dim_i - vec2<i32>(1));
    let h = textureLoad(heightmap_tex, tc, 0).r;
    let world_pos = vec3<f32>(
        in.position.x * (2.0 * camera.x_extent),
        h * camera.height_scale,
        in.position.z * (2.0 * camera.z_extent),
    );
    out.clip_pos = shadow_u.light_view_proj * vec4<f32>(world_pos, 1.0);
    out.skip = 0.0;
    return out;
}

@fragment
fn fs_shadow_terrain(in: VertexOutput) {
    if (in.skip > 0.5) {
        discard;
    }
}

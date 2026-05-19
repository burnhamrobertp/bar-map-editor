// SPDX-License-Identifier: GPL-3.0-or-later
//
// BAR-widget effect: instanced animated grass blades.
// Ported (simplified) from BAR's `map_grass_gl4.vert.glsl`.
//
// What we keep from upstream:
//   - Per-instance world position, rotation, size (`instancePosRotSize`).
//   - Heightmap-driven Y placement so blades grow off the terrain
//     surface rather than the flat water plane.
//   - Wind perturbation at the blade tip via a low-frequency sine
//     based on world XZ + time.
//   - Distance fade so far-away blades alpha out cleanly.
//
// What we drop / simplify (rationale per dropped piece):
//   - LOS texture sampling: gameplay state, not in the editor.
//   - Unit-bending: no units in BME's preview.
//   - Shadow PCF: BME already shadows the terrain via a separate
//     pass; grass on shaded ground inherits the colour from the
//     terrain albedo without needing its own shadow lookup.
//   - Map-color blending in the VS: pulled into the FS where it
//     just samples once per fragment.
//   - Night factor: time-of-day system isn't in BME.

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
    _pad_height_range_elmos: f32,
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

/// Per-pipeline tuning packed into a single vec4. Matches
/// `MapGrassUniform` in `widgets/map_grass.rs`.
///   x = WIND_STRENGTH (blade-top XZ sway amplitude in elmos).
///   y = MAP_COLOR_FACTOR -- terrain albedo blend with blade colour.
///   z = MAP_COLOR_BASE -- additional terrain albedo blend at the
///       blade base (fades to terrain colour where the blade
///       meets the ground).
///   w = FADE_END distance (elmos). Distances beyond this saturate
///       to invisible; FADE_START is fixed at 0.65 * FADE_END to
///       give a soft ramp.
@group(1) @binding(3) var<uniform> grass_params: vec4<f32>;

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
}

@vertex
fn vs_grass(in: VsIn) -> VsOut {
    var out: VsOut;
    let size_render = in.instance.w;
    if size_render <= 0.0 {
        // Cull zero-size instances by pushing them outside clip space.
        out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        out.uv = in.uv;
        out.world_xz = vec2<f32>(0.0);
        out.fade = 0.0;
        return out;
    }
    // The static mesh's positions are authored in elmo units
    // (`BLADE_MESH_HEIGHT_ELMOS` etc. in `widgets/map_grass.rs`).
    // `size_render` is the engine `grassMaxSize` multiplier already
    // converted to render units by the CPU instance generator, so
    // multiplying the mesh by it lands in render space.
    let scaled = in.pos * size_render;
    let cos_r = cos(in.instance.y);
    let sin_r = sin(in.instance.y);
    let rotated = vec3<f32>(
        scaled.x * cos_r - scaled.z * sin_r,
        scaled.y,
        scaled.x * sin_r + scaled.z * cos_r,
    );
    // `world_xz` is already in render space ([-x_extent, +x_extent])
    // -- the CPU side generates instance positions there directly.
    let world_xz_render = vec2<f32>(in.instance.x, in.instance.z);
    // Heightmap sample: render space -> normalized UV across the
    // playable area.
    let hm_uv = vec2<f32>(
        (world_xz_render.x / (2.0 * camera.x_extent)) + 0.5,
        (world_xz_render.y / (2.0 * camera.z_extent)) + 0.5,
    );
    let dim = vec2<i32>(textureDimensions(grass_heightmap_tex));
    let dim_f = vec2<f32>(dim);
    let tc = clamp(
        vec2<i32>(hm_uv * dim_f),
        vec2<i32>(0),
        dim - vec2<i32>(1),
    );
    let ground_y = textureLoad(grass_heightmap_tex, tc, 0).r * camera.height_scale;

    // Wind sway: low-frequency sine in world XZ + time. Only the
    // top of the blade moves (`in.pos.y` is the local up-axis;
    // base at 0, tip at BLADE_MESH_HEIGHT_ELMOS). `wind_strength`
    // is in render units so the displacement is proportional to
    // blade height; very tall blades sway more.
    let wind_strength = grass_params.x;
    let wind_phase = camera.time * 0.6
        + world_xz_render.x * 0.05
        + world_xz_render.y * 0.05;
    let sway_amount = in.pos.y * size_render;
    let wind_x = sin(wind_phase) * wind_strength * sway_amount;
    let wind_z = cos(wind_phase * 1.3) * wind_strength * sway_amount;

    let world_pos = vec3<f32>(
        world_xz_render.x + rotated.x + wind_x,
        ground_y + rotated.y,
        world_xz_render.y + rotated.z + wind_z,
    );

    // Distance-fade towards the camera. `fade_end` is in render
    // units (the CPU side packs it that way), matching the
    // render-space coords of `world_pos` and `camera.camera_pos`.
    let to_cam = camera.camera_pos - world_pos;
    let dist = length(to_cam);
    let fade_end = max(grass_params.w, 1e-4);
    let fade_start = fade_end * 0.65;
    let fade = clamp(
        (fade_end - dist) / max(fade_end - fade_start, 1e-4),
        0.0,
        1.0,
    );

    out.clip = camera.view_proj * vec4<f32>(world_pos, 1.0);
    out.uv = in.uv;
    out.world_xz = world_xz_render;
    out.fade = fade;
    return out;
}

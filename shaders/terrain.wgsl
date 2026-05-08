// Terrain Rendering Shader
//
// Material selection (based on UV encoding set by the mesh generator):
//   uv.x < -0.5        -- water / lava plane: call shade_water() from water.wgsl
//   uv.y > 1.5         -- skirt / bottom cap: world-space passthrough (no displacement)
//   has_texture == 1   -- sample albedo from group(1) binding 0
//   otherwise          -- procedural height-based colour

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
    quality: f32,
    /// 1.0 => discard water-plane fragments (reflection pre-pass).
    skip_water: f32,
    screen_w: f32,
    screen_h: f32,
    /// Half-span of the terrain mesh in world units on the X axis.
    x_extent: f32,
    /// Half-span of the terrain mesh in world units on the Z axis.
    z_extent: f32,
    // ── SMF ground shading (sourced from MapSettings) ─────────────
    /// xyz = sun direction (normalised on CPU); w = groundSpecularExponent.
    sun_dir_exp: vec4<f32>,
    ground_ambient: vec4<f32>,
    ground_diffuse: vec4<f32>,
    ground_specular: vec4<f32>,
    /// rgb = waterAbsorbColor.
    water_absorb: vec4<f32>,
    water_base_color: vec4<f32>,
    water_min_color: vec4<f32>,
    /// xy = brush cursor world XZ; z = radius (world units);
    /// w = 1.0 active / 0.0 inactive.
    brush_cursor: vec4<f32>,
}

@group(0) @binding(0) var<uniform> camera: CameraUniform;

// Group 1: albedo + metalmap + typemap.
// has_texture drives the albedo path; metalmap/typemap are sampled unconditionally
// (they default to 1x1 zero textures so absent data evaluates to 0).
@group(1) @binding(0) var albedo_tex: texture_2d<f32>;
@group(1) @binding(1) var albedo_sam: sampler;
@group(1) @binding(2) var metalmap_tex: texture_2d<f32>;
@group(1) @binding(3) var typemap_tex: texture_2d<f32>;
@group(1) @binding(4) var material_sam: sampler;

/// Planar-reflection texture -- rendered in a pre-pass with the camera mirrored
/// through the water plane. Sampled in the water fragment branch.
@group(2) @binding(0) var reflection_texture: texture_2d<f32>;
@group(2) @binding(1) var reflection_sampler: sampler;

/// Heightmap for GPU vertex displacement. Shares group 3 with water_normal (declared in
/// water.wgsl at bindings 0/1). Format: R32Float, non-filterable -- use textureLoad.
@group(3) @binding(2) var heightmap_tex: texture_2d<f32>;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    // Skirt / bottom cap: world-space position, pass through directly.
    if in.uv.y > 1.5 {
        out.clip_position = camera.view_proj * vec4<f32>(in.position, 1.0);
        out.world_position = in.position;
        out.normal = in.normal;
        out.uv = in.uv;
        return out;
    }

    // Water plane: world-space position, pass through directly.
    if in.uv.x < -0.5 {
        out.clip_position = camera.view_proj * vec4<f32>(in.position, 1.0);
        out.world_position = in.position;
        out.normal = in.normal;
        out.uv = in.uv;
        return out;
    }

    // Terrain surface: GPU displacement.
    // Flat-grid positions are in [-0.5, 0.5]^2; scale to world space via extents.
    let uv = in.uv;
    let dim_i = vec2<i32>(textureDimensions(heightmap_tex));
    let dim_f = vec2<f32>(dim_i);
    let tc = clamp(vec2<i32>(uv * dim_f), vec2<i32>(0), dim_i - vec2<i32>(1));
    let h = textureLoad(heightmap_tex, tc, 0).r;
    let world_x = in.position.x * (2.0 * camera.x_extent);
    let world_z = in.position.z * (2.0 * camera.z_extent);
    let world_y = h * camera.height_scale;
    let world_pos = vec3<f32>(world_x, world_y, world_z);

    out.clip_position = camera.view_proj * vec4<f32>(world_pos, 1.0);
    out.world_position = world_pos;
    out.uv = uv;

    // Surface normal via central differences in heightmap texel space.
    let tc_xp = clamp(tc + vec2<i32>(1, 0), vec2<i32>(0), dim_i - vec2<i32>(1));
    let tc_xn = clamp(tc + vec2<i32>(-1, 0), vec2<i32>(0), dim_i - vec2<i32>(1));
    let tc_zp = clamp(tc + vec2<i32>(0, 1), vec2<i32>(0), dim_i - vec2<i32>(1));
    let tc_zn = clamp(tc + vec2<i32>(0, -1), vec2<i32>(0), dim_i - vec2<i32>(1));
    let h_xp = textureLoad(heightmap_tex, tc_xp, 0).r;
    let h_xn = textureLoad(heightmap_tex, tc_xn, 0).r;
    let h_zp = textureLoad(heightmap_tex, tc_zp, 0).r;
    let h_zn = textureLoad(heightmap_tex, tc_zn, 0).r;
    // World-space step between adjacent heightmap texels.
    let world_dx = (2.0 * camera.x_extent) / dim_f.x;
    let world_dz = (2.0 * camera.z_extent) / dim_f.y;
    let dy_dx = (h_xp - h_xn) * camera.height_scale / (2.0 * world_dx);
    let dy_dz = (h_zp - h_zn) * camera.height_scale / (2.0 * world_dz);
    out.normal = normalize(vec3<f32>(-dy_dx, 1.0, -dy_dz));

    return out;
}

/// Sky colour for a unit-length view direction. Wraps the Recoil-ported
/// `modern_sky` for fog and skybox use within this shader.
fn sky_color(dir: vec3<f32>) -> vec3<f32> {
    return modern_sky(dir, camera.time);
}

fn height_color(height: f32) -> vec3<f32> {
    var color: vec3<f32>;
    if (height < 0.05) {
        color = vec3<f32>(0.2, 0.4, 0.5);
    } else if (height < 0.2) {
        let t = (height - 0.05) / 0.15;
        color = mix(vec3<f32>(0.2, 0.5, 0.2), vec3<f32>(0.3, 0.6, 0.2), t);
    } else if (height < 0.5) {
        let t = (height - 0.2) / 0.3;
        color = mix(vec3<f32>(0.3, 0.6, 0.2), vec3<f32>(0.5, 0.4, 0.3), t);
    } else if (height < 0.8) {
        let t = (height - 0.5) / 0.3;
        color = mix(vec3<f32>(0.5, 0.4, 0.3), vec3<f32>(0.6, 0.6, 0.6), t);
    } else {
        let t = (height - 0.8) / 0.2;
        color = mix(vec3<f32>(0.6, 0.6, 0.6), vec3<f32>(0.95, 0.95, 0.95), t);
    }
    return color;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Water plane branch.
    if (in.uv.x < -0.5) {
        if (camera.skip_water > 0.5) {
            discard;
        }
        let eye_dir = normalize(camera.camera_pos - in.world_position);
        let scr_uv = vec2<f32>(
            in.clip_position.x / camera.screen_w,
            in.clip_position.y / camera.screen_h,
        );
        return shade_water(in.world_position, eye_dir, scr_uv);
    }

    let sun_dir = normalize(camera.sun_dir_exp.xyz);
    let normal = normalize(in.normal);
    let view_dir = normalize(camera.camera_pos - in.world_position);
    let shadow_coeff = 1.0;
    let cos_diffuse = clamp(dot(sun_dir, normal), 0.0, 1.0);
    let ground_shade = smf_ground_shade(
        in.world_position,
        normal,
        sun_dir,
        view_dir,
        camera.ground_ambient.xyz,
        camera.ground_diffuse.xyz,
        camera.ground_specular.xyz,
        camera.sun_dir_exp.w,
        shadow_coeff,
    );

    var color: vec3<f32>;
    if (camera.has_texture != 0u && in.uv.y <= 1.5) {
        color = textureSample(albedo_tex, albedo_sam, in.uv).rgb;
    } else {
        let normalized_height = in.world_position.y / max(camera.height_scale, 0.0001);
        color = height_color(normalized_height);
    }

    var lit_color = color * ground_shade;

    // Underwater absorption (SMF_WATER_ABSORPTION path).
    if (camera.water_y >= 0.0 && in.world_position.y < camera.water_y) {
        let elmo_y = (in.world_position.y - camera.water_y)
            / max(camera.height_scale, 1e-4) * 8.0;
        lit_color = smf_water_absorb(
            lit_color,
            elmo_y,
            cos_diffuse,
            shadow_coeff,
            camera.water_base_color.xyz,
            camera.water_absorb.xyz,
            camera.water_min_color.xyz,
        );
    }

    // Atmospheric fog (high-pass only).
    if (camera.quality > 0.5) {
        let view_vec = in.world_position - camera.camera_pos;
        let dist = length(view_vec);
        let fog_factor = 1.0 - exp(-dist * 0.10);
        let fog_color = sky_color(normalize(view_vec));
        lit_color = mix(lit_color, fog_color, fog_factor * 0.18);
    }

    // Brush cursor ring.
    if (camera.brush_cursor.w > 0.5) {
        let cursor_xz = camera.brush_cursor.xy;
        let radius = camera.brush_cursor.z;
        let dx = in.world_position.x - cursor_xz.x;
        let dz = in.world_position.z - cursor_xz.y;
        let d = sqrt(dx * dx + dz * dz);
        let thickness = max(radius * 0.08, 0.002);
        let inner = radius - thickness;
        let outer_t = smoothstep(radius, inner, d);
        let inner_t = smoothstep(inner, inner - thickness, d);
        let ring = clamp(outer_t - inner_t, 0.0, 1.0);
        let disc = smoothstep(radius, inner, d) * 0.18;
        let cursor_color = vec3<f32>(1.0, 0.85, 0.2);
        let cursor_mix = ring * 0.85 + disc;
        lit_color = mix(lit_color, cursor_color, clamp(cursor_mix, 0.0, 1.0));
    }

    return vec4<f32>(lit_color, 1.0);
}

// ── Skybox pass ────────────────────────────────────────────────────────────

struct SkyVOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_sky(@builtin(vertex_index) vid: u32) -> SkyVOut {
    let xy = vec2<f32>(
        f32((vid & 1u) << 1u) * 2.0 - 1.0,
        f32(vid & 2u) * 2.0 - 1.0,
    );
    var out: SkyVOut;
    out.clip_position = vec4<f32>(xy, 1.0, 1.0);
    out.ndc = xy;
    return out;
}

@fragment
fn fs_sky(in: SkyVOut) -> @location(0) vec4<f32> {
    let clip = vec4<f32>(in.ndc, 1.0, 1.0);
    let world_h = camera.inv_view_proj * clip;
    let world_pos = world_h.xyz / world_h.w;
    let view_dir = normalize(world_pos - camera.camera_pos);
    let sky = sky_color(view_dir);
    return vec4<f32>(sky, 1.0);
}

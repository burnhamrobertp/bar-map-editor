// Terrain Rendering Shader
// Renders terrain mesh with albedo texture (or height-based fallback) and lighting.
//
// Material selection (based on UV encoding set by the mesh generator):
//   uv.x < -0.5        → water / lava plane: call bump_water() from smf_water.wgsl
//   uv.y > 1.5         → skirt / bottom cap: force height-based colour (no texture)
//   has_texture == 1   → sample albedo from group(1) texture
//   otherwise          → procedural height-based colour

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
    /// 1.0 ⇒ discard water-plane fragments (used by the planar
    /// reflection pre-pass which renders everything except water).
    skip_water: f32,
    screen_w: f32,
    screen_h: f32,
    _pad: vec2<f32>,
    // ── SMF ground shading (sourced from MapSettings) ─────────────
    /// xyz = sun direction (already normalised on the CPU side);
    /// w   = `groundSpecularExponent` from mapinfo.lua.
    sun_dir_exp: vec4<f32>,
    /// rgb = `groundAmbientColor`.
    ground_ambient: vec4<f32>,
    /// rgb = `groundDiffuseColor`.
    ground_diffuse: vec4<f32>,
    /// rgb = `groundSpecularColor`.
    ground_specular: vec4<f32>,
    /// rgb = `waterAbsorbColor` from mapinfo.lua's water block.
    water_absorb: vec4<f32>,
    /// rgb = `waterBaseColor`.
    water_base_color: vec4<f32>,
    /// rgb = `waterMinColor`.
    water_min_color: vec4<f32>,
    /// xy = brush cursor world XZ; z = brush radius (world units);
    /// w = active flag (1.0 = draw ring, 0.0 = no cursor). Mirrors
    /// `TerrainRenderer::brush_cursor_uniform`.
    brush_cursor: vec4<f32>,
}

@group(0) @binding(0) var<uniform> camera: CameraUniform;

@group(1) @binding(0) var albedo_texture: texture_2d<f32>;
@group(1) @binding(1) var albedo_sampler: sampler;

/// Planar-reflection texture — populated by a pre-pass that rendered the
/// scene with the camera mirrored through the water plane. Sampled in the
/// water branch by screen UV with a small distortion offset for ripples.
@group(2) @binding(0) var reflection_texture: texture_2d<f32>;
@group(2) @binding(1) var reflection_sampler: sampler;

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
    out.clip_position = camera.view_proj * vec4<f32>(in.position, 1.0);
    out.world_position = in.position;
    out.normal = in.normal;
    out.uv = in.uv;
    return out;
}

/// Sky colour for a unit-length view direction. Wraps the Recoil-ported
/// `modern_sky` so the rest of this shader can call it like a local
/// function. Used for (1) the skybox background and (2) atmospheric
/// fog. (Water reflections were previously sourced from this function;
/// since M2.1 they come from the planar reflection texture.)
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
    // SMF ground lighting (Recoil-port). sun_dir + ground colours are
    // sourced from `MapSettings.lighting`; the rest matches the
    // engine's `GetShadeInt` math via `smf_ground_shade` /
    // `smf_water_absorb` from `shaders/recoil/smf_ground.wgsl`.
    let sun_dir = normalize(camera.sun_dir_exp.xyz);
    let normal = normalize(in.normal);
    let view_dir = normalize(camera.camera_pos - in.world_position);
    // We don't render a shadow map in the editor, so every fragment
    // gets full sun. When/if a shadow pass lands, this becomes a
    // sample of the light's depth target.
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
    if (in.uv.x < -0.5) {
        if (camera.skip_water > 0.5) {
            discard;
        }
        let eye_dir = normalize(camera.camera_pos - in.world_position);
        let scr_uv  = vec2<f32>(
            in.clip_position.x / camera.screen_w,
            in.clip_position.y / camera.screen_h,
        );
        return bump_water(in.world_position, eye_dir, scr_uv);
    } else if (camera.has_texture != 0u && in.uv.y <= 1.5) {
        let sampled = textureSample(albedo_texture, albedo_sampler, in.uv);
        color = sampled.rgb;
    } else {
        let normalized_height = in.world_position.y / max(camera.height_scale, 0.0001);
        color = height_color(normalized_height);
    }

    // Apply the SMF ground shading (ambient + diffuse + specular,
    // intensity-multiplied) to the surface colour.
    var lit_color = color * ground_shade;

    // Underwater terrain: SMF_WATER_ABSORPTION path. The engine's
    // formula expects elmo-space height relative to the water plane
    // (negative below water). We translate render-space `world_y`
    // into that frame by `(world_y - water_y) / height_scale * 8`,
    // since the renderer normalises 1 elmo of vertical to
    // `height_scale / 8` of render units (see `bar-app::eval_preview`
    // for the matching forward derivation).
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

    // Atmospheric fog (high-pass only): very subtle exponential haze.
    // Earlier values made the entire map look fogged — distance only
    // adds a slight horizon-coloured wash now, just enough to suggest
    // depth without obscuring detail. `0.10` density / `0.18` intensity
    // produce ~3% mix at distance 1, ~12% at distance 4.
    if (camera.quality > 0.5) {
        let view_vec = in.world_position - camera.camera_pos;
        let dist = length(view_vec);
        let fog_factor = 1.0 - exp(-dist * 0.10);
        let fog_color = sky_color(normalize(view_vec));
        lit_color = mix(lit_color, fog_color, fog_factor * 0.18);
    }

    // Brush cursor ring — drawn as a translucent annulus on the
    // terrain surface so the user can see where the brush will
    // stamp before they click. The ring's thickness is a fixed
    // fraction of the brush radius (8%, clamped to a sane minimum)
    // so it stays visible at all radii. Skipped entirely when
    // brush_cursor.w == 0.
    if (camera.brush_cursor.w > 0.5) {
        let cursor_xz = camera.brush_cursor.xy;
        let radius = camera.brush_cursor.z;
        let dx = in.world_position.x - cursor_xz.x;
        let dz = in.world_position.z - cursor_xz.y;
        let d = sqrt(dx * dx + dz * dz);
        let thickness = max(radius * 0.08, 0.002);
        let inner = radius - thickness;
        // smoothstep on both edges fades the ring antialiased.
        let outer_t = smoothstep(radius, inner, d);
        let inner_t = smoothstep(inner, inner - thickness, d);
        let ring = clamp(outer_t - inner_t, 0.0, 1.0);
        // Filled disc inside the ring at low opacity so the brush
        // footprint reads against busy terrain. Outer edge of the
        // disc fades the same way the ring does.
        let disc = smoothstep(radius, inner, d) * 0.18;
        let cursor_color = vec3<f32>(1.0, 0.85, 0.2);
        let cursor_mix = ring * 0.85 + disc;
        lit_color = mix(lit_color, cursor_color, clamp(cursor_mix, 0.0, 1.0));
    }

    return vec4<f32>(lit_color, 1.0);
}

// Wireframe variant
@fragment
fn fs_wireframe(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(0.8, 0.8, 0.8, 1.0);
}

// ── Skybox pass ────────────────────────────────────────────────────────────
//
// Renders the procedural sky to whatever pixels the terrain pass left
// untouched. Driven by a fullscreen triangle (no vertex buffer) at clip
// depth 1.0; the depth pipeline state uses LessEqual so terrain (z < 1.0)
// occludes us cleanly. The fragment shader reconstructs a world-space
// view direction from clip-space NDC via `inv_view_proj`.

struct SkyVOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_sky(@builtin(vertex_index) vid: u32) -> SkyVOut {
    // Single oversized triangle that covers the [-1, 1] NDC quad and
    // beyond — vertices at (-1, -1), (3, -1), (-1, 3) clip to a fullscreen
    // covering rect with no overdraw on the corners.
    let xy = vec2<f32>(
        f32((vid & 1u) << 1u) * 2.0 - 1.0,
        f32(vid & 2u) * 2.0 - 1.0,
    );
    var out: SkyVOut;
    // Place at far plane (z = 1.0) so the depth test puts terrain in front.
    out.clip_position = vec4<f32>(xy, 1.0, 1.0);
    out.ndc = xy;
    return out;
}

@fragment
fn fs_sky(in: SkyVOut) -> @location(0) vec4<f32> {
    // Reconstruct a world-space point on the far plane, then compute the
    // view direction from the camera to that point. `inv_view_proj` does
    // the heavy lifting; perspective divide recovers the world position.
    let clip = vec4<f32>(in.ndc, 1.0, 1.0);
    let world_h = camera.inv_view_proj * clip;
    let world_pos = world_h.xyz / world_h.w;
    let view_dir = normalize(world_pos - camera.camera_pos);

    let sky = sky_color(view_dir);
    return vec4<f32>(sky, 1.0);
}

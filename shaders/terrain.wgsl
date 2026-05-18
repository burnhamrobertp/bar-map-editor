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
    /// 1.0 => discard water-plane fragments (used by reflection / refraction
    /// pre-passes so the water surface itself isn't captured).
    skip_water: f32,
    /// Heightmap span in Spring elmos (= max_h - min_h). Lets the SMF
    /// water-absorption math convert render-space Y back to absolute
    /// elmos, since the engine's `SMF_SHALLOW_WATER_DEPTH = 10` is in
    /// elmos. Without this, the absorption is calibrated against the
    /// wrong unit and the refraction texture comes out un-tinted.
    height_range_elmos: f32,
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
    /// Signed-distance plane: discard fragments where
    /// dot(clip_plane.xyz, world_pos) + clip_plane.w < 0.
    /// Main pass sets (0, 0, 0, 1) so all fragments pass; reflection and
    /// refraction passes set this to keep only one side of the water plane.
    clip_plane: vec4<f32>,
    /// Height-based custom fog. rgb = colour, w = attenuation per elmo.
    /// Mirrors the in-game `custom.fog` widget that BAR maps use to tint
    /// fragments below a configured altitude (e.g. underwater). Not part
    /// of the engine's core SMF/BumpWater shaders, but applied here as a
    /// final post-pass so previews match in-game appearance.
    custom_fog_color_atten: vec4<f32>,
    /// x = custom-fog enabled (0/1), y = ceiling height in elmos,
    /// z = grass_shading_tex available (extension branch swaps to
    /// `grass_shading_tex` instead of `albedo_tex`), w = unused.
    custom_fog_params: vec4<f32>,
    /// Procedural-sky inputs from mapinfo `atmosphere = { ... }`.
    /// `sun_color.rgb` -> sun disc tint; `sky_color_density.rgb` -> base
    /// sky colour at horizon; `sky_color_density.a` -> cloud density
    /// (0..1, scales the cumulus/cirrus thresholds); `sky_dir.xyz` ->
    /// sun direction in world space (per-map); `cloud_color.rgb` ->
    /// cloud tint.
    sun_color: vec4<f32>,
    sky_color_density: vec4<f32>,
    sky_dir: vec4<f32>,
    cloud_color: vec4<f32>,
    /// x = skybox enabled (0/1). When 1, fs_sky samples the cubemap;
    /// otherwise it falls through to procedural ModernSky.
    /// y = legacy `detailTex` strength (0/1). The engine only applies
    /// `detailTex` to the playable area when the map is in simple
    /// (non-splat) detail mode; we encode that decision CPU-side
    /// from the presence of `splatDistrTex`.
    skybox_params: vec4<f32>,
    /// Per-channel UV scale for the four splat-detail-normal textures
    /// (mapinfo `splats.texScales`). Applied to world XZ in elmos.
    splat_tex_scales: vec4<f32>,
    /// Per-channel mix multiplier for the distribution (mapinfo
    /// `splats.texMults`). Multiplied into the distribution sample
    /// before the weighted sum.
    splat_tex_mults: vec4<f32>,
    /// xy = elmos per render-space unit (host computes from map
    /// dimensions). z = advanced splat detail enabled (0/1). w =
    /// splat-detail diffuse-alpha enabled (0/1).
    splat_params: vec4<f32>,
    /// Distance-fog parameters precomputed host-side from mapinfo
    /// `atmosphere.fogStart` * `camera.far` and `atmosphere.fogEnd` *
    /// `camera.far` -- engine-faithful (`UniformConstants.cpp:231`).
    /// xy = start_dist, end_dist (render-space distances); zw reserved.
    fog_dists: vec4<f32>,
    /// Mapinfo `atmosphere.fogColor`. Distinct from `sky_color_density`:
    /// engine fog-aware shaders mix toward this tint, not the sky
    /// colour. The map-edge extension widget uses it as the haze
    /// target colour.
    fog_color: vec4<f32>,
}

// ── Debug toggles ──────────────────────────────────────────────────────
//
// Bisection knobs for chasing visual divergences from engine. Each
// const wraps one effect in the fragment shader; flipping to `false`
// disables that effect (with the rest still active) so the user can
// pinpoint which path is producing a visible artefact. Order is roughly
// "most likely culprit first" for the current Ascendancy-washout
// investigation. The const-folding optimiser removes the dead branches,
// so leaving them in costs nothing once we land on a setting.
const DBG_SKY_REFLECTION: bool      = true; // skybox cubemap mixed via skyReflectModTex
const DBG_SPECULAR: bool            = true; // sun spec lobe (smf_specular)
const DBG_DETAIL_TEX: bool          = true; // resources.detailTex contribution
const DBG_SPLAT_NORMAL_PERTURB: bool = true; // tangent-space normal perturb from splat-detail-normals
const DBG_SPLAT_DETAIL_COLOR: bool   = true; // alpha-channel detail colour from splat-detail-normals

// VISUALISATION: see the comment block above the `return` in fs_main for
// the channel meanings.
const DBG_VISUALIZE_SPEC: bool      = false;
// VISUALISATION: splat detail-normal contribution. Fires unconditionally
// (NOT gated on the splat path being active), so it also reveals when
// the renderer hasn't enabled splat at all.
//   R = (s1.a + 1) * 0.5         -- alpha of splat-detail-normal #1
//                                   (`splatDetailNormalTex1`), remapped
//                                   to [0,1]. Mid-grey = no/flat content.
//   G = distr_sample.r           -- red channel of the splat distribution
//                                   texture. Should be high at metal-spot
//                                   locations on Ascendency.
//   B = camera.splat_params.z    -- renderer's `advanced_splat_enabled`
//                                   flag (0 or 1). If B is black across
//                                   the whole map, the splat textures
//                                   never got uploaded (sync failed).
// Quick reading:
//   - All black                  -> splat textures not loaded at all.
//   - Green only at metal spots  -> distribution OK; check why renderer
//                                   has B=0 (sync log).
//   - All three channels present -> path is working; investigate inside.
const DBG_VISUALIZE_SPLAT_DETAIL: bool = false;


@group(0) @binding(0) var<uniform> camera: CameraUniform;
/// Skybox cubemap from mapinfo's `atmosphere.skyBox` DDS, sampled by the
/// sky pipeline when `camera.skybox_params.x > 0.5`. Defaults to a 1x1
/// black cubemap until `update_skybox` uploads real face data.
@group(0) @binding(1) var skybox_tex: texture_cube<f32>;
@group(0) @binding(2) var skybox_sam: sampler;

// Group 1: albedo + metalmap + typemap.
// has_texture drives the albedo path; metalmap/typemap are sampled unconditionally
// (they default to 1x1 zero textures so absent data evaluates to 0).
@group(1) @binding(0) var albedo_tex: texture_2d<f32>;
@group(1) @binding(1) var albedo_sam: sampler;
@group(1) @binding(2) var metalmap_tex: texture_2d<f32>;
@group(1) @binding(3) var typemap_tex: texture_2d<f32>;
@group(1) @binding(4) var material_sam: sampler;
/// Detail texture (mapinfo `resources.detailTex`). Sampled at world XZ
/// in repeat mode; subtracted by 0.5 before adding to the diffuse so
/// the texture darkens AND lightens. Defaults to 1x1 (0.5, 0.5, 0.5)
/// when not uploaded, which makes the subtracted value zero -- a
/// no-op contribution.
@group(1) @binding(5) var detail_tex: texture_2d<f32>;
@group(1) @binding(6) var detail_sam: sampler;
/// Splat-detail-normal textures + distribution
/// (`SMF_DETAIL_NORMAL_TEXTURE_SPLATTING` path). Sampled in elmo space
/// at per-texture scales from `splat_tex_scales`, then weighted by the
/// distribution * `splat_tex_mults` and combined into a single signed
/// detail contribution.
@group(1) @binding(7)  var splat_dn_tex_1: texture_2d<f32>;
@group(1) @binding(8)  var splat_dn_tex_2: texture_2d<f32>;
@group(1) @binding(9)  var splat_dn_tex_3: texture_2d<f32>;
@group(1) @binding(10) var splat_dn_tex_4: texture_2d<f32>;
@group(1) @binding(11) var splat_distr_tex: texture_2d<f32>;
/// Per-pixel reflection-strength mask (`skyReflectModTex`). Gates the
/// engine's `SMF_SKY_REFLECTIONS` path -- where it's bright the
/// terrain reflects the skybox cubemap; where it's black no reflection.
@group(1) @binding(12) var sky_reflect_mod_tex: texture_2d<f32>;
/// Per-pixel specular colour + exponent (`specularTex`). Gates the engine's
/// `SMF_SPECULAR_LIGHTING` path -- when `skybox_params.w > 0.5`, the
/// shader samples this texture and uses `.rgb` as the per-pixel specular
/// colour and `.a * 16` as the per-pixel exponent, instead of the global
/// `groundSpecularColor` / `groundSpecularExponent` uniforms. Without
/// this, every lit fragment got the global spec strength (which Ascendancy
/// authors as 0.5) and the entire sun-facing terrain went hot white.
@group(1) @binding(13) var specular_tex: texture_2d<f32>;

/// Map-edge extension texture (mapinfo `grassShadingTex`). Sampled by
/// the extension shader branch when `custom_fog_params.z > 0.5`; falls
/// back to the playable albedo otherwise. Defaults to a 1x1 grey;
/// real content lands via `update_grass_shading_tex`.
@group(1) @binding(14) var grass_shading_tex: texture_2d<f32>;

/// Planar-reflection texture -- rendered in a pre-pass with the camera mirrored
/// through the water plane. Sampled in the water fragment branch.
@group(2) @binding(0) var reflection_texture: texture_2d<f32>;
@group(2) @binding(1) var reflection_sampler: sampler;

/// Planar-refraction texture -- rendered with the original camera and a clip
/// plane that keeps only the side of the water plane opposite to the camera.
/// Sampled in the water fragment branch for transparency / Snell's window.
@group(2) @binding(2) var refraction_texture: texture_2d<f32>;
@group(2) @binding(3) var refraction_sampler: sampler;

/// Heightmap for GPU vertex displacement. Shares group 3 with water_normal (declared in
/// water.wgsl at bindings 0/1). Format: R32Float, non-filterable -- use textureLoad.
@group(3) @binding(2) var heightmap_tex: texture_2d<f32>;

/// Pre-baked per-fragment surface normal map. Engine parity with
/// `SMFFragProg.glsl::GetFragmentNormal`: stores world-space (X, Z) of
/// the unit normal in an Rg8Snorm texture and lets the fragment shader
/// reconstruct Y = sqrt(1 - X*X - Z*Z). Sampling with the (filtering)
/// `water_normal_sam` at group 3 binding 1 gives smooth lighting on
/// slopes -- noticeably crisper than the per-vertex normal that was
/// interpolated across triangles. Generated CPU-side in
/// `renderer.rs::build_normal_map_bytes` whenever the heightmap or
/// height_scale changes.
@group(3) @binding(3) var normal_map_tex: texture_2d<f32>;

/// Shadow map -- see `crates/bar-render/src/shadow.rs`. Group 4 is unused by
/// the reflection/refraction pre-passes (they bind a dummy receiver group so
/// the pipeline layout matches).
struct ShadowUniform {
    light_view_proj: mat4x4<f32>,
    sun_dir: vec4<f32>,
}
@group(4) @binding(0) var<uniform> shadow_u: ShadowUniform;
@group(4) @binding(1) var shadow_tex: texture_depth_2d;
@group(4) @binding(2) var shadow_samp: sampler_comparison;

/// Sample the shadow map at `world_pos` and return 1.0 = lit, 0.0 = shadowed.
/// Hardware 2x2 PCF via comparison sampler (see `features.wgsl::shadow_factor`
/// for the same approach). Sharp at silhouettes, soft only across a single
/// texel. Fragments outside the frustum default to lit.
fn sample_shadow(world_pos: vec3<f32>) -> f32 {
    let ls = shadow_u.light_view_proj * vec4<f32>(world_pos, 1.0);
    let ndc = ls.xyz / ls.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, -ndc.y * 0.5 + 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || ndc.z > 1.0) {
        return 1.0;
    }
    let bias = 0.0005;
    return textureSampleCompare(shadow_tex, shadow_samp, uv, ndc.z - bias);
}

/// Returns true if the fragment is on the kept side of camera.clip_plane.
fn pass_clip_plane(world_pos: vec3<f32>) -> bool {
    return dot(camera.clip_plane.xyz, world_pos) + camera.clip_plane.w >= 0.0;
}

/// Apply the mapinfo `custom.fog` height-based tint to a fragment colour.
/// Returns `color` unchanged when the fog is disabled or the fragment is
/// above the ceiling. Inside the fog region the colour is *multiplicatively
/// tinted* toward `fog_color`: at the ceiling `tint = vec3(1)` (no change),
/// at full attenuation `tint = fog_color` (dims and colour-shifts). This
/// matches the in-game behaviour where the fog absorbs light selectively
/// per channel rather than blending the fragment toward a bright fog
/// colour (which is what a plain `mix(...)` would do and what made the
/// previous version of this pass look milky/cloudy at depth).
fn apply_custom_fog(color: vec3<f32>, world_pos: vec3<f32>) -> vec3<f32> {
    if (camera.custom_fog_params.x < 0.5) {
        return color;
    }
    let elmo_y = world_pos.y
        / max(camera.height_scale, 1e-4)
        * camera.height_range_elmos;
    let below = camera.custom_fog_params.y - elmo_y;
    if (below <= 0.0) {
        return color;
    }
    let f = clamp(below * camera.custom_fog_color_atten.w, 0.0, 1.0);
    let tint = mix(vec3<f32>(1.0), camera.custom_fog_color_atten.xyz, f);
    return color * tint;
}

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

    // Mirrored map-edge extension: vertex `position.xz` is in world
    // space in the mirror quadrant, but `position.y == 0` -- the VS
    // samples the heightmap at the encoded playable UV to give the
    // mirrored terrain the same shape as the playable area. UV is
    // packed as (playable_u, 4.0 + playable_v).
    //
    // After sampling Y, the engine widget bends the mirror downward
    // with the square of distance (in elmos) from the playable
    // corner/edge-midpoint nearest the quadrant -- the "earth
    // curvature" effect. We replicate that here in render space by
    // converting `curvatureBend = 150 elmos` and the squared distance
    // into our coordinates via `splat_params.xy` (= elmos/render-XZ)
    // and `height_range_elmos`. Reference point lives in render
    // space on the corner of the playable boundary -- inferred from
    // the vertex's world XZ position.
    if in.uv.y > 3.5 {
        let playable_uv = vec2<f32>(in.uv.x, in.uv.y - 4.0);
        let dim_f = vec2<f32>(textureDimensions(heightmap_tex));
        let tc = clamp(
            vec2<i32>(playable_uv * dim_f),
            vec2<i32>(0),
            vec2<i32>(dim_f - vec2<f32>(1.0)),
        );
        let h = textureLoad(heightmap_tex, tc, 0).r;
        var world_y = h * camera.height_scale;

        // Curvature: detect mirror axes from the vertex's world XZ
        // relative to the playable bounds, then bend Y by the engine
        // formula in elmo space.
        let west  = in.position.x < -camera.x_extent;
        let east  = in.position.x >  camera.x_extent;
        let north = in.position.z < -camera.z_extent;
        let south = in.position.z >  camera.z_extent;
        let apply_x = west || east;
        let apply_z = north || south;
        let ref_x = select(0.0, select(camera.x_extent, -camera.x_extent, west), apply_x);
        let ref_z = select(0.0, select(camera.z_extent, -camera.z_extent, north), apply_z);
        let elmo_per_render = camera.splat_params.xy;
        let curvature_bend_elmos = 150.0;
        var bend_elmos = 0.0;
        if (apply_x) {
            let dx_elmos = (in.position.x - ref_x) * elmo_per_render.x;
            let q = dx_elmos / curvature_bend_elmos;
            bend_elmos = bend_elmos + q * q;
        }
        if (apply_z) {
            let dz_elmos = (in.position.z - ref_z) * elmo_per_render.y;
            let q = dz_elmos / curvature_bend_elmos;
            bend_elmos = bend_elmos + q * q;
        }
        let bend_render = bend_elmos * camera.height_scale
            / max(camera.height_range_elmos, 1.0);
        world_y = world_y - bend_render;

        let world_pos = vec3<f32>(in.position.x, world_y, in.position.z);
        out.clip_position = camera.view_proj * vec4<f32>(world_pos, 1.0);
        out.world_position = world_pos;
        out.normal = vec3<f32>(0.0, 1.0, 0.0);
        out.uv = in.uv;
        return out;
    }

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
    if (!pass_clip_plane(in.world_position)) {
        discard;
    }

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
        // `clip_position.z` here is the fragment's NDC depth (0..1).
        // We pass it through so `shade_water` can compare against the
        // refraction-pass depth texture for the engine's depth-aware
        // mixback.
        return shade_water(in.world_position, eye_dir, scr_uv, in.clip_position.z);
    }

    // Mirrored map-edge extension branch -- port of BAR's
    // `luaui/Widgets/map_edge_extension2.lua`. Geometry sits in 8
    // quadrants surrounding the playable area; each vertex carries a
    // playable-area UV in `[0, 1]` even though its world XZ is in the
    // mirror quadrant. Sampling the albedo at that playable UV gives
    // a mirrored copy of the map's surface. The widget further dims
    // the luminance (preserving chroma via a YCbCr round-trip) and
    // optionally fades with linear fog at the outer edge.
    //
    // Sentinel: uv.y in [4.0, 5.0] encodes the playable UV.x in
    // `uv.x - 4.0` when in this range -- avoids collision with the
    // existing skirt (uv.y = 2.0) and water (uv.x = -1.0) ranges.
    // (Vertex playable UV is packed into uv.x = playable_u, uv.y =
    // 4.0 + playable_v.)
    if (in.uv.y > 3.5) {
        let playable_uv = vec2<f32>(in.uv.x, in.uv.y - 4.0);
        // Engine `MAP_BASE_GRASS_TEX` semantics: sample `grass_shading_tex`
        // when the map sets one, otherwise fall back to the playable
        // albedo (engine's minimap-fallback approximation). `custom_fog_params.z`
        // is repurposed as the gate -- belongs in a dedicated extension
        // uniform eventually.
        var albedo: vec3<f32>;
        if (camera.custom_fog_params.z > 0.5) {
            albedo = textureSample(grass_shading_tex, albedo_sam, playable_uv).rgb;
        } else if (camera.has_texture != 0u) {
            albedo = textureSample(albedo_tex, albedo_sam, playable_uv).rgb;
        } else {
            albedo = vec3<f32>(0.35, 0.32, 0.28);
        }

        // Mirror axes: which side of the playable area this fragment is
        // in. Drives both the normal-component flips below and the
        // curvature falloff further down.
        let west  = in.world_position.x < -camera.x_extent;
        let east  = in.world_position.x >  camera.x_extent;
        let north = in.world_position.z < -camera.z_extent;
        let south = in.world_position.z >  camera.z_extent;
        let apply_x = west || east;
        let apply_z = north || south;

        // Sample the same pre-baked playable-area normal map at the
        // mirrored UV (engine widget samples `$ssmf_normals`). Reconstruct
        // Y from the unit-length constraint, then flip X / Z components
        // for axes the geometry is mirrored across so lighting stays
        // consistent with the playable surface across the seam.
        var ext_normal: vec3<f32>;
        {
            let nxz = textureSample(normal_map_tex, water_normal_sam, playable_uv).rg;
            let xz_len_sq = dot(nxz, nxz);
            if (xz_len_sq < 0.999) {
                let ny = sqrt(1.0 - xz_len_sq);
                ext_normal = vec3<f32>(nxz.x, ny, nxz.y);
            } else {
                ext_normal = vec3<f32>(0.0, 1.0, 0.0);
            }
            if (apply_x) { ext_normal.x = -ext_normal.x; }
            if (apply_z) { ext_normal.z = -ext_normal.z; }
            ext_normal = normalize(ext_normal);
        }

        // Engine order (`map_edge_extension2.lua:366-388`): darken the
        // raw albedo via YCbCr first (brightness = 0.3), then apply
        // hemispheric lighting `albedo * (dot(N, sun) * 0.5 + 1.0)`.
        // The hemispheric form ranges [0.5, 1.5] so back-facing slopes
        // still get 50% brightness and front-facing up to 150% -- no
        // pure black, no shadow term. Replaces my earlier port to
        // `smf_ground_shade` which used the engine's full SMF
        // `ambient + diffuse * clamp(N.L, 0, 1)` formula -- that's the
        // playable-surface lighting, not the extension widget's.
        let RGB2YCBCR = mat3x3<f32>(
            0.2126, -0.114572, 0.5,
            0.7152, -0.385428, -0.454153,
            0.0722,  0.5,      -0.0458471,
        );
        let YCBCR2RGB = mat3x3<f32>(
            1.0,  1.0,        1.0,
            0.0, -0.187324,   1.8556,
            1.5748, -0.468124, -5.55112e-17,
        );
        var ycbcr = RGB2YCBCR * albedo;
        ycbcr.x = clamp(ycbcr.x * 0.3, 0.0, 1.0);
        let darkened_albedo = YCBCR2RGB * ycbcr;
        let ext_sun_dir = normalize(camera.sun_dir_exp.xyz);
        let hemi = dot(ext_normal, ext_sun_dir) * 0.5 + 1.0;
        let darkened = darkened_albedo * hemi;

        // Curvature alpha falloff. Engine formula:
        //   alpha = 1 + 6*(sum_axes(-((world - ref) / mapSize)^2) + 0.18)
        // Distances are normalised by playable map size (= 2 * extent
        // in render space). Each active axis contributes a quadratic
        // attenuation -- so corner quadrants drop off fastest, edge
        // quadrants more gradually.
        let ref_x = select(0.0, select(camera.x_extent, -camera.x_extent, west), apply_x);
        let ref_z = select(0.0, select(camera.z_extent, -camera.z_extent, north), apply_z);
        var curv_acc = 0.0;
        if (apply_x) {
            let dn = (in.world_position.x - ref_x) / (2.0 * camera.x_extent);
            curv_acc = curv_acc - dn * dn;
        }
        if (apply_z) {
            let dn = (in.world_position.z - ref_z) / (2.0 * camera.z_extent);
            curv_acc = curv_acc - dn * dn;
        }
        let curv_alpha = clamp(1.0 + 6.0 * (curv_acc + 0.18), 0.0, 1.0);

        // Edge fog -- linear falloff in camera-to-fragment world
        // distance, blended toward the atmosphere's sky colour.
        // Replaces the engine widget's "mix toward fogColor by
        // fogFactor" line; fragments far from the camera approach
        // the sky colour, hiding the curvature bend's seam at the
        // horizon. Start/end are scaled by `x_extent` so the fog
        // kicks in just past the playable edge regardless of map
        // size.
        let view_dist = length(camera.camera_pos - in.world_position);
        // Engine path (`UniformConstants.cpp:231`): mapinfo `atmosphere.fogStart`
        // / `atmosphere.fogEnd` scaled by camera far-plane host-side. Falls
        // back to `extent_radius`-scaled defaults if the uniform is unset.
        let extent_radius = max(camera.x_extent, camera.z_extent);
        let fog_start = select(3.0 * extent_radius, camera.fog_dists.x, camera.fog_dists.y > 0.0);
        let fog_end = select(7.0 * extent_radius, camera.fog_dists.y, camera.fog_dists.y > 0.0);
        let fog_factor = clamp(
            (fog_end - view_dist) / max(fog_end - fog_start, 1e-4),
            0.0,
            1.0,
        );
        // Engine widget (`map_edge_extension2.lua:390`) mixes toward
        // mapinfo `atmosphere.fogColor` for the fog step. This is the
        // engine-wide `fogColor` uniform (`UniformConstants.cpp:227`),
        // distinct from the sky tint. Using sky_color here as I did
        // previously produced the pale-blue cast that didn't match
        // in-game appearance -- engine fog and sky are independently
        // authored colours and most maps make them different.
        let fog_target = camera.fog_color.rgb;
        let after_fog = mix(fog_target, darkened, fog_factor);

        // Curvature alpha would normally drive output alpha for the
        // engine's transparent-blend pipeline (where the sky shows
        // through behind the extension). In our opaque pipeline the
        // closest approximation is to mix toward the sky colour at
        // the curvature falloff -- procedural-sky maps get the
        // authored sky tint; cubemap-skybox maps get an approximation
        // that's not pixel-perfect but reads as "dissolves into sky".
        let sky_tint = camera.sky_color_density.rgb;
        let composited = mix(sky_tint, after_fog, curv_alpha);
        return vec4<f32>(apply_custom_fog(composited, in.world_position), 1.0);
    }

    let sun_dir = normalize(camera.sun_dir_exp.xyz);
    // `normal` is a `var` rather than `let` so the splat-detail-normal
    // block below can perturb it before we compute lighting. Engine
    // order: normal perturbation -> lighting (`SMFFragProg.glsl::main`).
    //
    // Surface normal is sampled from the pre-baked normal map keyed off
    // the heightmap (engine path: `SMFFragProg.glsl::GetFragmentNormal`).
    // Stored as Rg8Snorm holding world-space (X, Z); Y reconstructs
    // from the unit-length constraint. Falls back to the interpolated
    // per-vertex normal when XZ has length >= 1 (only happens near
    // wrap-around values produced by snorm rounding at the extremes).
    var normal: vec3<f32>;
    {
        let nxz = textureSample(normal_map_tex, water_normal_sam, in.uv).rg;
        let xz_len_sq = dot(nxz, nxz);
        if (xz_len_sq < 0.999) {
            let ny = sqrt(1.0 - xz_len_sq);
            normal = normalize(vec3<f32>(nxz.x, ny, nxz.y));
        } else {
            normal = normalize(in.normal);
        }
    }
    let view_dir = normalize(camera.camera_pos - in.world_position);
    let shadow_coeff = sample_shadow(in.world_position);

    var color: vec3<f32>;
    if (camera.has_texture != 0u && in.uv.y <= 1.5) {
        color = textureSample(albedo_tex, albedo_sam, in.uv).rgb;
    } else if (camera.has_texture != 0u && in.uv.y > 1.5 && in.uv.y < 3.5) {
        // Skirt / cap face (uv.y == 2.0). Engine path:
        // `SMFBorderFragProg.glsl:17-18` samples the playable diffuse at
        // the world-XZ UV clamped to [0, 1] with a small `UV_BORDER_LEEWAY`
        // (1e-2), then darkens by `diffuseMult = 110/255` (~0.43). That
        // keeps the visible cliff face between the playable mesh and
        // the extension mesh visually consistent with the surrounding
        // terrain -- without this branch the fragment falls to the
        // procedural `height_color` ramp and renders bright tan
        // regardless of what the actual playable albedo looks like.
        let edge_uv = vec2<f32>(
            in.world_position.x / (2.0 * camera.x_extent) + 0.5,
            in.world_position.z / (2.0 * camera.z_extent) + 0.5,
        );
        let clamped = clamp(edge_uv, vec2<f32>(1e-2), vec2<f32>(1.0 - 1e-2));
        color = textureSample(albedo_tex, albedo_sam, clamped).rgb * (110.0 / 255.0);
    } else {
        let normalized_height = in.world_position.y / max(camera.height_scale, 0.0001);
        color = height_color(normalized_height);
    }

    // DIAGNOSTIC: fires unconditionally (NOT gated on splat_params.z).
    // Samples the splat textures directly so we see content even when
    // the renderer thinks splat is disabled. See the comment above
    // `DBG_VISUALIZE_SPLAT_DETAIL` for the channel meanings.
    if (DBG_VISUALIZE_SPLAT_DETAIL && in.uv.y <= 1.5) {
        let dbg_world_xz_elmos = in.world_position.xz * camera.splat_params.xy;
        let dbg_s1 = textureSample(
            splat_dn_tex_1, detail_sam,
            dbg_world_xz_elmos * camera.splat_tex_scales.x,
        );
        let dbg_distr_uv = vec2<f32>(
            in.world_position.x / (2.0 * camera.x_extent) + 0.5,
            1.0 - (in.world_position.z / (2.0 * camera.z_extent) + 0.5),
        );
        let dbg_distr = textureSample(splat_distr_tex, detail_sam, dbg_distr_uv);
        return vec4<f32>(
            dbg_s1.a,
            dbg_distr.r,
            camera.splat_params.z,
            1.0,
        );
    }

    // Detail texture contribution. Matches `SMFFragProg::GetDetailTextureColor`
    // -- sample at world.xz with `specularTexGen` (= 1/mapSize in
    // elmos), which tiles the texture once across the playable area.
    // Subtracting 0.5 centres it so the texture both lightens AND
    // darkens the base diffuse rather than only brightening.
    //
    // Strength gate from `skybox_params.y`: 0 when the map uses splat
    // detail (Aurelia, most modern BAR maps -- the engine routes
    // detailTex to its border shader instead, which we don't render),
    // 1 otherwise. This is what made the playable area go all-red on
    // Aurelia previously -- we were stamping the detail texture over
    // a surface the engine never applies it to.
    //
    // Skirts / cap (uv.y > 1.5) get no detail -- they're not surface
    // terrain.
    var detail_contrib = vec3<f32>(0.0);
    if (DBG_DETAIL_TEX && in.uv.y <= 1.5 && camera.skybox_params.y > 0.5) {
        // world.xz is in render space ([-x_extent, x_extent]). Map
        // that to [0, 1] so the texture tiles once across, matching
        // the engine's `vertexWorldPos.xz / mapSize` behaviour.
        let detail_uv = in.world_position.xz / (2.0 * camera.x_extent) + vec2<f32>(0.5);
        let detail_sample = textureSample(detail_tex, detail_sam, detail_uv).rgb;
        detail_contrib = detail_sample - vec3<f32>(0.5);
    }

    // Advanced splat-detail-normal path (engine
    // `SMF_DETAIL_NORMAL_TEXTURE_SPLATTING`). Active for Aurelia and
    // most modern BAR maps. Four detail-normal textures get sampled at
    // their own per-channel scales (in elmo space), weighted by the
    // distribution texture * `splat_tex_mults`, summed. The alpha of
    // the weighted sum provides the detail-colour contribution
    // (`splatDetailStrength.y` upstream). The RGB provides a
    // tangent-space normal perturbation that gets rotated into world
    // space and mixed with the surface normal by `splatDetailStrength.x`
    // (sum of distribution cofacs, clamped to 1).
    if (in.uv.y <= 1.5 && camera.splat_params.z > 0.5) {
        // Convert render XZ -> world elmo XZ. Engine samples in elmo
        // units, multiplied by the per-channel scales.
        let world_xz_elmos = in.world_position.xz * camera.splat_params.xy;
        let s1 = textureSample(splat_dn_tex_1, detail_sam, world_xz_elmos * camera.splat_tex_scales.x)
            * 2.0 - 1.0;
        let s2 = textureSample(splat_dn_tex_2, detail_sam, world_xz_elmos * camera.splat_tex_scales.y)
            * 2.0 - 1.0;
        let s3 = textureSample(splat_dn_tex_3, detail_sam, world_xz_elmos * camera.splat_tex_scales.z)
            * 2.0 - 1.0;
        let s4 = textureSample(splat_dn_tex_4, detail_sam, world_xz_elmos * camera.splat_tex_scales.w)
            * 2.0 - 1.0;

        // Distribution: tiles once across the playable area
        // (`specTexCoords = worldXZ / mapSize` upstream).
        //
        // V-flip experiment: BAR's engine renders in OpenGL where V=0
        // is the bottom of the texture; we render in wgpu where V=0 is
        // the top. If the splat distribution DDS was authored against
        // the engine convention, sampling it with our convention
        // produces an N/S-mirrored distribution -- cliff channels land
        // on the flat ground, ground channels land on the cliffs --
        // which is what ref3 vs ref4 showed in the original screenshot
        // comparison.
        // V-flip on the splat distribution UV. Engine OpenGL stores tex
        // V=0 at the bottom of the texture; wgpu V=0 is at the top.
        // Confirmed correct for Azurite Shores; confirmed for at least one
        // other map that's been spot-checked. If a future map shows
        // distribution channels swapped N/S after fresh import, the
        // suspect is map-specific DDS row ordering (some authoring tools
        // write bottom-up rows) and the right fix is to detect orientation
        // at DDS load time, not to revisit this flip.
        let distr_uv = vec2<f32>(
            in.world_position.x / (2.0 * camera.x_extent) + 0.5,
            1.0 - (in.world_position.z / (2.0 * camera.z_extent) + 0.5),
        );
        let splat_cofac = textureSample(splat_distr_tex, detail_sam, distr_uv)
            * camera.splat_tex_mults;

        // Weighted sum -- one vec4 (RGB + alpha) accumulated across
        // the 4 textures by their respective distribution channels.
        var splat_normal = vec4<f32>(0.0);
        splat_normal = splat_normal + s1 * splat_cofac.r;
        splat_normal = splat_normal + s2 * splat_cofac.g;
        splat_normal = splat_normal + s3 * splat_cofac.b;
        splat_normal = splat_normal + s4 * splat_cofac.a;

        // Alpha-channel detail colour, gated by the diffuse-alpha
        // flag (mapinfo `splatDetailNormalDiffuseAlpha`).
        if (DBG_SPLAT_DETAIL_COLOR && camera.splat_params.w > 0.5) {
            let detail_y = clamp(splat_normal.a, -1.0, 1.0);
            detail_contrib = vec3<f32>(detail_y);
        }

        if (DBG_SPLAT_NORMAL_PERTURB) {
            // Normal perturbation. Engine builds a tangent basis from the
            // surface normal (`SMFFragProg.glsl::main` for SMF_BLEND_NORMALS):
            //   tTangent = normalize(cross(normal, vec3(-1, 0, 0)))
            //   sTangent = cross(normal, tTangent)
            //   stnMatrix = mat3(sTangent, tTangent, normal)
            // The tangent-space splat normal is rotated into world space
            // via `stnMatrix`, then mixed with the surface normal by
            // `splatDetailStrength.x = clamp(dot(splatCofac, vec4(1)), 0, 1)`.
            // y = 0.01 floor prevents the perturbed normal from pointing
            // sideways when all cofacs happen to be zero.
            splat_normal.y = max(splat_normal.y, 0.01);
            let s_strength_x = clamp(splat_cofac.r + splat_cofac.g + splat_cofac.b + splat_cofac.a, 0.0, 1.0);
            let t_tangent = normalize(cross(normal, vec3<f32>(-1.0, 0.0, 0.0)));
            let s_tangent = cross(normal, t_tangent);
            let stn = mat3x3<f32>(s_tangent, t_tangent, normal);
            let world_perturbed = normalize(stn * splat_normal.xyz);
            normal = normalize(mix(normal, world_perturbed, s_strength_x));
        }
    }

    // Sky cube reflection (engine `SMF_SKY_REFLECTIONS` path). Order
    // matters: applied AFTER normal perturbation so the reflect
    // direction uses the perturbed surface (engine does
    // `perturb -> reflect -> shade`). Mixed into `color` BEFORE the
    // shade multiply so reflection respects ambient + diffuse the way
    // `SMFFragProg.glsl::main` does:
    //   diffuseCol = mix(diffuseCol, reflectCol, reflectMod)
    //   fragColor  = (diffuseCol + detailCol) * shadeInt
    // Gated on both a real skybox cubemap AND a real reflection-mask
    // texture -- without the mask we'd reflect uniformly across the
    // whole terrain, which is wrong.
    if (DBG_SKY_REFLECTION && in.uv.y <= 1.5 && camera.skybox_params.x > 0.5 && camera.skybox_params.z > 0.5) {
        let cam_to_frag = in.world_position - camera.camera_pos;
        let reflect_dir = reflect(cam_to_frag, normal);
        let reflect_col = textureSample(skybox_tex, skybox_sam, reflect_dir).rgb;
        // V-flip for the same reason as the splat distribution: whole-map
        // mask DDS authored against engine OpenGL row-ordering.
        let mod_uv = vec2<f32>(
            in.world_position.x / (2.0 * camera.x_extent) + 0.5,
            1.0 - (in.world_position.z / (2.0 * camera.z_extent) + 0.5),
        );
        let reflect_mod = textureSample(sky_reflect_mod_tex, detail_sam, mod_uv).rgb;
        color = mix(color, reflect_col, reflect_mod);
    }

    // Lighting is computed AFTER any normal perturbation (splat-detail-normal,
    // future SMF_BLEND_NORMALS, etc.) so the diffuse / specular terms
    // pick up the perturbed surface. This is what `SMFFragProg.glsl::main`
    // does: perturb -> shade.
    let cos_diffuse = clamp(dot(sun_dir, normal), 0.0, 1.0);
    let ground_shade = smf_ground_shade(
        in.world_position,
        normal,
        sun_dir,
        camera.ground_ambient.xyz,
        camera.ground_diffuse.xyz,
        shadow_coeff,
    );
    // Sun specular computed separately so it can be added on top of
    // the `texture × shade` term -- engine adds `specularInt` after
    // the texture multiply in `SMFFragProg.glsl::main`. Folding spec
    // into `shade_int` (which is then multiplied by the terrain
    // texture) would dim every glint by the local texture brightness.
    var spec_term = vec3<f32>(0.0);
    if (DBG_SPECULAR) {
        // SMF_SPECULAR_LIGHTING path (`SMFFragProg.glsl:300-310`): when
        // the map ships a `specularTex`, sample it per-fragment for the
        // local specular colour and exponent. Otherwise fall back to the
        // global ground_specular / sun_dir_exp.w uniforms.
        //
        // Engine encoding:
        //   specCol.rgb  -> per-pixel specular colour (mostly near-zero on
        //                   natural terrain; only metal / wet patches
        //                   are visibly reflective).
        //   specCol.a*16 -> per-pixel specular exponent (low alpha = broad
        //                   matte spec, high alpha = tight glint).
        // Without this path, maps that author non-zero `groundSpecularColor`
        // (Ascendancy: 0.5) but rely on the texture to gate where spec
        // actually appears show whole-surface spec blowout.
        var spec_color: vec3<f32> = camera.ground_specular.xyz;
        var spec_exp: f32 = camera.sun_dir_exp.w;
        if (camera.skybox_params.w > 0.5) {
            // V-flip matches the splat distribution: whole-map DDS authored
            // against engine OpenGL row-ordering. Without this, the metal-
            // spot spec content lands at N/S-mirrored XZ and produces
            // bright glints on the snow opposite the actual metal pads.
            let spec_uv = vec2<f32>(
                in.world_position.x / (2.0 * camera.x_extent) + 0.5,
                1.0 - (in.world_position.z / (2.0 * camera.z_extent) + 0.5),
            );
            let spec_sample = textureSample(specular_tex, detail_sam, spec_uv);
            spec_color = spec_sample.rgb;
            spec_exp = spec_sample.a * 16.0;
        }
        spec_term = smf_specular(
            normal,
            sun_dir,
            view_dir,
            spec_color,
            spec_exp,
            shadow_coeff,
        );
    }

    // Engine match (`rts/.../SMFFragProg.glsl:381`):
    //   fragColor.rgb = (diffuseCol.rgb + detailCol.rgb) * shadeInt.rgb
    // So we compute the shade (ground or water-absorbed) FIRST, then
    // multiply by the terrain colour at the end. Previously we did the
    // colour multiply before the water-absorb branch, which made
    // `smf_water_absorb` return water_shade alone for deep water --
    // bypassing the texture multiply entirely. That's what produced the
    // bright-blue "pool of mercury" deep water: the diffuse texture
    // (which is normally a dark seabed tint) wasn't darkening the result.
    var shade_int = ground_shade;

    // Underwater absorption (SMF_WATER_ABSORPTION path).
    if (camera.water_y >= 0.0 && in.world_position.y < camera.water_y) {
        // Render-space Y -> elmo Y. `height_scale` is render-y per unit of
        // [0,1] normalised heightmap, and `height_range_elmos` is the elmo
        // span of the same [0,1] range, so the ratio converts between them.
        let elmo_y = (in.world_position.y - camera.water_y)
            / max(camera.height_scale, 1e-4)
            * camera.height_range_elmos;
        shade_int = smf_water_absorb(
            ground_shade,
            elmo_y,
            cos_diffuse,
            shadow_coeff,
            camera.water_base_color.xyz,
            camera.water_absorb.xyz,
            camera.water_min_color.xyz,
        );
    }

    // Engine order: `fragColor = (diffuse + detail) * shadeInt; fragColor += specularInt;`.
    // Adding spec AFTER the texture multiply is what keeps glints bright.
    // `detail_contrib` is zero when no detail texture is loaded (default
    // 1x1 grey - 0.5 = 0), so this stays a no-op for maps without one.
    var lit_color = (color + detail_contrib) * shade_int + spec_term;

    // Debug viz: short-circuit the shader and output diagnostic channels.
    //   R = cos_specular  (red: where half_dir aligns with normal)
    //   G = spec_exp/100  (green: constant tint reveals actual exponent)
    //   B = spec_term.r*5 (blue: where spec is bright after pow + color)
    // If G channel is uniformly green = exp is ~100. If G is dim = exp
    // is much lower than expected. If R is bright everywhere = half_dir
    // is aligning with normal far too often (input vectors wrong).
    if (DBG_VISUALIZE_SPEC) {
        // Re-sample the specularTex directly so we see what the per-fragment
        // path is actually reading at this pixel. Same UV as the spec path.
        let spec_uv_dbg = vec2<f32>(
            in.world_position.x / (2.0 * camera.x_extent) + 0.5,
            1.0 - (in.world_position.z / (2.0 * camera.z_extent) + 0.5),
        );
        let spec_sample_dbg = textureSample(specular_tex, detail_sam, spec_uv_dbg);
        // R = spec_color.r       (raw texture R channel; how reflective the
        //                         surface is in red. Bright = strong glints.)
        // G = spec_sample.a      (alpha channel directly. Multiplied by 16
        //                         to get the actual spec exponent. Max
        //                         possible value = 1.0 here, which would
        //                         give spec_exp = 16 - a broad lobe.)
        // B = cos_specular       (per-fragment half-vector alignment.
        //                         Bright = many fragments aligned with
        //                         half_dir at this angle.)
        let half_dbg = normalize(sun_dir + view_dir);
        let cos_spec_dbg = clamp(dot(half_dbg, normal), 0.001, 1.0);
        return vec4<f32>(spec_sample_dbg.r, spec_sample_dbg.a, cos_spec_dbg, 1.0);
    }
    // Height-based `custom.fog` post-pass (matches the in-game widget that
    // ships with BAR). For underwater fragments this is what gives the
    // seabed its strong cool/blue tint; SMF water-absorption alone leaves
    // it too warm because the rust-coloured texture dominates.
    lit_color = apply_custom_fog(lit_color, in.world_position);

    // Engine distance fog (`SMFFragProg.glsl:437`): final per-fragment mix
    // toward atmospheric `fogColor`. Same fog_dists / fog_color uniforms
    // the water shader uses. For maps where the fog distance is
    // unreachable in the visible scene (Onyx at our render scale) this
    // saturates to 1.0 and the stage is a no-op.
    if camera.fog_dists.y > 0.0 {
        let view_dist = length(camera.camera_pos - in.world_position);
        let fog_factor = clamp(
            (camera.fog_dists.y - view_dist)
                / max(camera.fog_dists.y - camera.fog_dists.x, 1e-4),
            0.0,
            1.0,
        );
        lit_color = mix(camera.fog_color.rgb, lit_color, fog_factor);
    }

    // Brush cursor ring: 1-px AA outline, no disc fill.
    if (camera.brush_cursor.w > 0.5) {
        let cursor_xz = camera.brush_cursor.xy;
        let radius = camera.brush_cursor.z;
        let dx = in.world_position.x - cursor_xz.x;
        let dz = in.world_position.z - cursor_xz.y;
        let d = sqrt(dx * dx + dz * dz);
        // fwidth gives the world-space change in d per screen pixel, so
        // (1.0 - abs(d - radius) / aa) is a tent that spans ~2 px and
        // stays exactly at the brush boundary regardless of brush size.
        let aa = max(fwidth(d), 0.0001);
        let ring = clamp(1.0 - abs(d - radius) / aa, 0.0, 1.0);
        let cursor_color = vec3<f32>(1.0, 0.9, 0.2);
        lit_color = mix(lit_color, cursor_color, ring * 0.92);
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
    // The sky lives above the water plane by definition. The reflection
    // pre-pass keeps the above-water half-space (clip_plane.y > 0) and we
    // want sky in it. The refraction pre-pass keeps the below-water half
    // (clip_plane.y < 0) and we DO NOT want sky there -- if we did, the
    // water shader's refraction sample would be sky-tinted everywhere and
    // the water would render as a near-mirror.
    if (camera.clip_plane.y < -0.5) {
        discard;
    }
    let clip = vec4<f32>(in.ndc, 1.0, 1.0);
    let world_h = camera.inv_view_proj * clip;
    let world_pos = world_h.xyz / world_h.w;
    let view_dir = normalize(world_pos - camera.camera_pos);
    // When the map ships a `skyBox` cubemap (and we've uploaded it),
    // sample that directly -- it's authored content and overrides the
    // procedural sky. Fall back to procedural when there's no cubemap.
    var sky: vec3<f32>;
    if (camera.skybox_params.x > 0.5) {
        sky = textureSample(skybox_tex, skybox_sam, view_dir).rgb;
    } else {
        sky = sky_color(view_dir);
    }
    return vec4<f32>(sky, 1.0);
}

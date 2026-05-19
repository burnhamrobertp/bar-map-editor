use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use wgpu::util::DeviceExt;

use crate::camera::Camera;
use crate::terrain::{
    generate_flat_grid, generate_map_edge_extension, generate_terrain_skirts_and_cap,
    generate_water_plane, TerrainVertex,
};
use bar_data::{ColorBuffer, Heightmap};

// ── Texture-format conventions ────────────────────────────────────────────
//
// Maps each map-authored texture role to the wgpu format it must use.
//
// Engine reference: BAR runs with `GL_FRAMEBUFFER_SRGB` disabled
// (`bar-recoil/.../GL/State.h:185`) and uploads textures without the
// sRGB flag, so its samplers return raw `byte/255` to the shader for
// every texture -- colour or data. BAR's shaders do all their math in
// this sRGB-perceptual space and write to a non-sRGB framebuffer; the
// display device gamma-decodes the bytes on output. The pipeline is
// gamma-incorrect by modern standards, but consistent end-to-end.
//
// BME mirrors BAR's pipeline so map authors see what they'll see
// in-engine: every colour texture uses a non-sRGB format (no GPU
// decode on sample), every data texture too, every render target /
// framebuffer uses non-sRGB (no GPU encode on write). The named
// constants below pin this convention per texture role so an
// accidental sRGB format choice surfaces immediately in the
// format-convention tests at the bottom of this file.

/// Colour textures (mapinfo-authored perceptual sRGB values, treated
/// as linear by every shader that multiplies them with lighting
/// uniforms). Stored without the sRGB flag so the GPU returns raw
/// byte/255 -- matches BAR's `glTexImage2D` upload that omits the
/// sRGB internal-format variant. Marker constant; the inline
/// `create_texture` call sites spell `Rgba8Unorm` directly. Pinned by
/// the format-convention test below.
#[allow(dead_code)]
const COLOUR_TEX_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// BC1-compressed colour textures (the SMT terrain atlas). Same
/// reasoning as `COLOUR_TEX_FORMAT`: BAR uploads BC1 without the
/// `_SRGB` variant, samples return raw byte/255 to the shader.
#[allow(dead_code)]
const COLOUR_TEX_FORMAT_BC1: wgpu::TextureFormat = wgpu::TextureFormat::Bc1RgbaUnorm;

/// Splat detail-normal textures: RGB carries tangent-space normal
/// coordinates decoded via `(sample * 2 - 1)` in `SMFFragProg.glsl:183`;
/// A carries detail strength. Pure data.
const SPLAT_DETAIL_NORMAL_TEX_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Splat distribution texture: per-channel material weights, sampled
/// and multiplied directly with `splatTexMults` in
/// `SMFFragProg.glsl:168`. Pure data. (Same wgpu format as
/// detail-normal -- the closure that builds both uses the
/// detail-normal constant since they coincide.)
#[allow(dead_code)]
const SPLAT_DISTR_TEX_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Sky-reflection mod texture: per-channel mix factor for
/// `mix(diffuse, reflect, reflectMod)` in `SMFFragProg.glsl:348`.
/// Pure data (interpolation weight).
const SKY_REFLECT_MOD_TEX_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Per-pixel specular texture: RGB used as `specCol.rgb * specularPow`,
/// A used as `exp = A * 16` (`SMFFragProg.glsl:413,419`). Engine treats
/// both as direct face-value data; we follow suit.
const SPECULAR_TEX_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Parameters for a full heightmap replacement. Passed to
/// [`TerrainRenderer::update_heightmap`] to avoid a clippy `too_many_arguments`
/// violation.
pub struct TerrainUpdateParams {
    pub height_scale: f32,
    pub x_extent: f32,
    pub z_extent: f32,
    pub water_y: f32,
    pub water_color: [f32; 3],
    pub grid_n: u32,
    /// Vertical span of the heightmap in Spring elmos (`max_h - min_h`).
    /// Lets the shader convert render-space Y back to absolute elmos so the
    /// SMF water-absorption depth math (which is calibrated against the
    /// engine's `SMF_SHALLOW_WATER_DEPTH = 10` elmos) actually matches the
    /// engine. Previously the shader assumed `1 height_scale unit == 8
    /// elmos`, which under-counted the depth by ~75x on typical BAR maps
    /// and left the refraction texture nearly un-tinted.
    pub height_range_elmos: f32,
    /// Elmos per unit of render-space XZ. Computed by the host from
    /// map dimensions (`world_size_elmos / (2 * extent)`). Needed by
    /// the splat-detail shader path to apply per-channel UV scales
    /// in elmo units, matching `vertexWorldPos.xzxz * splatTexScales`
    /// upstream.
    pub elmo_per_render_xz: [f32; 2],
    /// Append the mirrored map-edge extension mesh (port of BAR's
    /// `map_edge_extension2.lua` widget). Preview layout sets `true`
    /// so the playable area appears surrounded by a darkened mirror
    /// of itself reaching toward the horizon; Sculpt3D sets `false`
    /// to keep the edit view focused on the playable area.
    pub include_edge_extension: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 3],
    has_texture: u32,
    height_scale: f32,
    water_r: f32,
    water_g: f32,
    water_b: f32,
    water_y: f32,
    time: f32,
    skip_water: f32,
    height_range_elmos: f32,
    screen_w: f32,
    screen_h: f32,
    x_extent: f32,
    z_extent: f32,
    sun_dir_exp: [f32; 4],
    ground_ambient: [f32; 4],
    ground_diffuse: [f32; 4],
    ground_specular: [f32; 4],
    water_absorb: [f32; 4],
    water_base_color: [f32; 4],
    water_min_color: [f32; 4],
    brush_cursor: [f32; 4],
    // Signed-distance plane: keep fragments where dot(plane.xyz, world_pos)
    // + plane.w >= 0. Used by reflection and refraction passes; main pass
    // sets it to (0, 0, 0, 1) so all fragments pass.
    clip_plane: [f32; 4],
    // Height-based custom fog. rgb = colour, a = attenuation rate per elmo.
    custom_fog_color_atten: [f32; 4],
    // x = enabled (0/1), y = height (elmos), zw = unused.
    custom_fog_params: [f32; 4],
    // Atmosphere / procedural sky inputs (mapinfo `atmosphere = { ... }`).
    sun_color: [f32; 4],
    sky_color_density: [f32; 4], // rgb = skyColor, a = cloudDensity
    sky_dir: [f32; 4],
    cloud_color: [f32; 4],
    // x = skybox enabled (0/1) -- when 1 the sky shader samples
    // `skybox_tex`, otherwise falls back to procedural ModernSky.
    skybox_params: [f32; 4],
    // Per-channel UV scale for splat-detail-normal sampling (mapinfo
    // `splats.texScales`). The engine does `worldXZ * scales.{r,g,b,a}`
    // per texture and `worldXZ` is in elmos, so the shader needs to
    // convert render-space -> elmos first using `elmo_per_render` below.
    splat_tex_scales: [f32; 4],
    splat_tex_mults: [f32; 4],
    // xy = elmos per render-space unit (world_size_elmos / 2*extent).
    // z = advanced splat detail enabled (0/1).
    // w = splat detail diffuse-alpha enabled (0/1).
    splat_params: [f32; 4],
    // Distance fog parameters sourced from mapinfo `atmosphere.fogStart`
    // / `atmosphere.fogEnd`. Engine precomputes `fraction * far_plane`
    // host-side (see `rts/Rendering/UniformConstants.cpp:231`) and
    // hands the shaders absolute distances; we do the same so the
    // map-edge-extension shader can mix toward `sky_color` over the
    // map-authored range without needing to know the camera state.
    // x = fog_start_dist, y = fog_end_dist, zw = reserved.
    fog_dists: [f32; 4],
    // Mapinfo `atmosphere.fogColor` -- engine-equivalent of the
    // `fogColor` uniform every fog-aware shader (sky / projectiles /
    // map_edge_extension2 / etc.) reads. rgb = colour, w reserved.
    fog_color: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<CameraUniform>() == 528);

/// Clip-plane value that passes every fragment. Used by the main pass.
const NO_CLIP: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// Per-frame parameters passed to [`TerrainRenderer::render`].
///
/// Heightmap and texture data are pushed via dedicated `update_*` methods on
/// `TerrainRenderer`; this struct carries only the per-frame uniform inputs
/// that can change without a geometry or texture re-upload.
pub struct PreviewFrame {
    pub height_scale: f32,
    pub x_extent: f32,
    pub z_extent: f32,
    /// Render-space Y of the water plane. Negative => no water.
    pub water_y: f32,
    pub water_color: [f32; 3],
    /// True for the high-pass render (sky reflections + fog).
    pub quality_high: bool,
    pub time: f32,
    pub smf_lighting: SmfLighting,
    /// See [`TerrainUpdateParams::height_range_elmos`].
    pub height_range_elmos: f32,
    /// See [`TerrainUpdateParams::elmo_per_render_xz`].
    pub elmo_per_render_xz: [f32; 2],
}

/// Engine-faithful SMF shading inputs. Ground / sun fields come from the
/// `lighting` table in `mapinfo.lua`; water fields come from the `water`
/// table, with defaults matching Recoil's `rts/Map/MapInfo.cpp`.
#[derive(Clone, Copy, Debug)]
pub struct SmfLighting {
    // Ground lighting (SMF ground shader inputs)
    pub sun_dir: [f32; 3],
    pub ground_ambient: [f32; 3],
    pub ground_diffuse: [f32; 3],
    pub ground_specular: [f32; 3],
    /// Per-map shadow strength. Engine modulates the shadow sample as
    /// `shadow_coeff = mix(1.0, raw_shadow, density)` -- see
    /// `bar-recoil/rts/Map/SMF/SMFFragProg.glsl:371` -- so at density=0
    /// shadows disappear entirely, at density=1 the raw sample passes
    /// through. Default 0.8 (`MapInfo.cpp::ReadLight`).
    pub ground_shadow_density: f32,
    pub specular_exponent: f32,
    // Water absorption colors (used by `smf_water_absorb` for underwater
    // ground shading -- not the water surface itself)
    pub water_absorb: [f32; 3],
    pub water_base: [f32; 3],
    pub water_min: [f32; 3],
    // Water surface (BumpWater inputs) -- see `WaterParamsUniform`.
    pub water_surface_color: [f32; 3],
    pub water_surface_alpha: f32,
    pub water_diffuse_color: [f32; 3],
    pub water_specular_color: [f32; 3],
    pub water_ambient_factor: f32,
    pub water_diffuse_factor: f32,
    pub water_specular_factor: f32,
    pub water_specular_power: f32,
    pub water_fresnel_min: f32,
    pub water_fresnel_max: f32,
    pub water_fresnel_power: f32,
    pub water_reflection_distortion: f32,
    pub water_perlin_amplitude: f32,
    // Height-based "custom" fog (mapinfo's `custom.fog` block). Applied as
    // a final post-pass in the terrain and water shaders -- not part of
    // the engine SMF/BumpWater pipeline but matches in-game appearance
    // because BAR ships a widget that renders it.
    pub custom_fog_enabled: bool,
    pub custom_fog_color: [f32; 3],
    pub custom_fog_height_elmos: f32,
    pub custom_fog_atten: f32,
    // Atmosphere / sky parameters (`atmosphere = { ... }` in mapinfo).
    // Drive the procedural sky shader so each map has its authored sky
    // rather than a hardcoded one.
    pub sun_color: [f32; 3],
    /// Sun intensity from mapinfo `light.sunDir.w`. Packed into the
    /// uniform's `sun_color.w` so the sky shader can multiply its sun-
    /// corona term by it (matches `ModernSky.cpp:82` ->
    /// `ModernSkyFS.glsl:88` upstream). Default 1.0.
    pub sun_intensity: f32,
    pub sky_color: [f32; 3],
    pub sky_dir: [f32; 3],
    pub cloud_density: f32,
    pub cloud_color: [f32; 3],
    /// True when an actual cubemap has been uploaded via
    /// `TerrainRenderer::update_skybox`. The renderer also tracks this
    /// state internally; carrying it on `SmfLighting` lets the shader
    /// branch on a uniform read instead of needing two pipeline
    /// variants.
    pub skybox_enabled: bool,
    /// True when a `skyReflectModTex` has been uploaded. Gates the
    /// engine's `SMF_SKY_REFLECTIONS` path so the shader only mixes
    /// the cubemap into the terrain diffuse when a real mask is
    /// available -- otherwise we'd reflect into everything uniformly.
    pub sky_reflect_mod_enabled: bool,
    /// True when a `specularTex` has been uploaded. Gates the engine's
    /// `SMF_SPECULAR_LIGHTING` path: when set, the terrain shader samples
    /// the per-pixel specular colour + exponent from this texture instead
    /// of using the global `groundSpecularColor` / `groundSpecularExponent`
    /// uniforms. The texture's RGB is the per-pixel specular colour;
    /// alpha * 16 is the per-pixel exponent. Most natural terrain texels
    /// have near-zero values here, which is why maps that ship
    /// `specularTex` look right in-engine but show whole-surface spec
    /// blowout in our editor before this path is wired.
    pub specular_tex_enabled: bool,
    /// True when a `grassShadingTex` has been uploaded. Gates the
    /// map-edge extension shader between sampling the dedicated
    /// border texture vs falling back to the playable albedo.
    pub grass_shading_tex_enabled: bool,
    /// True when a `lightEmissionTex` has been uploaded. Gates the
    /// engine's `SMF_LIGHT_EMISSION` apply-emission stage in the
    /// terrain shader (`SMFFragProg.glsl:392-401`). When false, the
    /// emission blend is skipped entirely; the bound texture is still
    /// the inert 1x1 `(0,0,0,0)` default so it costs nothing to
    /// sample, but the explicit gate keeps the data-flow obvious.
    pub light_emission_tex_enabled: bool,
    /// 1.0 when the legacy `detailTex` should apply to the playable
    /// area, 0.0 when it shouldn't. Engine-side, `detailTex` is only
    /// applied to the playable area by `SMFFragProg` when the map is
    /// in the simple (non-splat) detail mode; maps using splat detail
    /// (i.e. `splatDistrTex` set) route it to the border shader
    /// instead, which we don't render. Carrying this here lets the
    /// shader gate detail without re-encoding the heuristic.
    pub detail_strength: f32,
    /// Per-channel UV scale for splat-detail sampling (from mapinfo
    /// `splats.texScales`). Engine multiplies world XZ in elmos by
    /// these; we convert render-space XZ -> elmos in the shader using
    /// `elmo_per_render_xz`.
    pub splat_tex_scales: [f32; 4],
    pub splat_tex_mults: [f32; 4],
    /// Set by `update_splat_textures` once all four splat-detail-normal
    /// textures + the distribution texture are uploaded. When false
    /// the shader skips the splat sampling entirely and the playable
    /// area renders without splat detail.
    pub advanced_splat_enabled: bool,
    /// Mirrors mapinfo `splatDetailNormalDiffuseAlpha`. When true the
    /// alpha of the weighted detail-normal sum contributes to the
    /// per-pixel detail colour; when false detail colour is 0.
    pub splat_detail_diffuse_alpha: bool,
    /// Map size in elmos per unit of render-space (= world_size_elmos /
    /// (2 * extent_render)). Set by the host so the splat shader can
    /// convert render-XZ -> elmo-XZ before applying per-channel scales.
    pub elmo_per_render_xz: [f32; 2],
    /// Distance-fog start/end as fractions of the camera far-plane,
    /// sourced from mapinfo `atmosphere.fogStart` / `atmosphere.fogEnd`.
    /// Engine path multiplies these by the camera far-plane host-side
    /// (`rts/Rendering/UniformConstants.cpp:231`) to produce absolute
    /// distances every fog-aware shader reads. Defaults match engine
    /// (`MapInfo.cpp`: 0.1 / 1.0).
    pub atmosphere_fog_start: f32,
    pub atmosphere_fog_end: f32,
    /// Mapinfo `atmosphere.fogColor`. Engine sources this from the
    /// `ISky::fogColor` uniform (`UniformConstants.cpp:227`) and every
    /// fog-aware shader mixes toward it -- including the map-edge
    /// extension widget which uses it as the haze tint distinct from
    /// the sky tint. Default `(0.7, 0.7, 0.8)` matches engine
    /// `MapInfo.cpp`.
    pub atmosphere_fog_color: [f32; 3],
}

/// Helper: clone `f.smf_lighting` with renderer-runtime flags overridden.
/// MapSettings doesn't know which assets the renderer has actually
/// uploaded -- so the per-frame uniform reads upload state from the
/// renderer instead. Same pattern as the skybox flag.
#[allow(clippy::too_many_arguments)]
fn bar_render_smf_with_runtime_overrides(
    mut smf: SmfLighting,
    skybox_enabled: bool,
    advanced_splat_enabled: bool,
    sky_reflect_mod_enabled: bool,
    specular_tex_enabled: bool,
    grass_shading_tex_enabled: bool,
    light_emission_tex_enabled: bool,
    elmo_per_render_xz: [f32; 2],
) -> SmfLighting {
    smf.skybox_enabled = skybox_enabled;
    smf.advanced_splat_enabled = advanced_splat_enabled;
    smf.sky_reflect_mod_enabled = sky_reflect_mod_enabled;
    smf.specular_tex_enabled = specular_tex_enabled;
    smf.grass_shading_tex_enabled = grass_shading_tex_enabled;
    smf.light_emission_tex_enabled = light_emission_tex_enabled;
    smf.elmo_per_render_xz = elmo_per_render_xz;
    smf
}

impl From<&bar_project::MapSettings> for SmfLighting {
    /// Build the renderer-side lighting + water inputs directly from a
    /// recipe's `MapSettings`. Both the GUI (`live_smf_lighting` in
    /// `bar-app::viewport`) and the CLI (`bar-cli::cmd_preview`) call
    /// through this so there is one canonical mapping; previously each
    /// site copied the 20+ fields by hand, which made it easy for the
    /// CLI to drift (it shipped default zero-water for a while, which
    /// in turn made headless debugging meaningless).
    fn from(ms: &bar_project::MapSettings) -> Self {
        let l = &ms.lighting;
        let w = &ms.water;
        Self {
            sun_dir: l.sun_dir,
            // Engine-faithful colour pipeline: mapinfo colour triples
            // pass through to the shader as raw sRGB-perceptual values.
            // BAR runs with `GL_FRAMEBUFFER_SRGB` disabled
            // (`bar-recoil/.../GL/State.h:185`) and samples colour
            // textures as raw byte / 255 -- so every multiplication in
            // BAR's terrain / water shaders happens in sRGB-perceptual
            // space. To match BAR's visible output, BME does the same:
            // no sRGB decode at this boundary, color textures are
            // uploaded as linear formats (no GPU decode on sample),
            // framebuffer is non-sRGB (no encode on write). The
            // pipeline is gamma-incorrect by modern graphics
            // standards, but the editor's purpose is engine-fidelity
            // for map authors who tune their maps to look right
            // in-game.
            ground_ambient: l.ground_ambient,
            ground_diffuse: l.ground_diffuse,
            ground_specular: l.ground_specular,
            ground_shadow_density: l.ground_shadow_density.clamp(0.0, 1.0),
            specular_exponent: l.spec_exponent,
            water_absorb: w.absorb,
            water_base: w.base_color,
            water_min: w.min_color,
            water_surface_color: w.surface_color,
            water_surface_alpha: w.surface_alpha,
            water_diffuse_color: w.diffuse_color,
            water_specular_color: w.specular_color,
            water_ambient_factor: w.ambient_factor,
            water_diffuse_factor: w.diffuse_factor,
            water_specular_factor: w.specular_factor,
            water_specular_power: w.specular_power,
            water_fresnel_min: w.fresnel_min,
            water_fresnel_max: w.fresnel_max,
            water_fresnel_power: w.fresnel_power,
            water_reflection_distortion: w.reflection_distortion,
            water_perlin_amplitude: w.perlin_amplitude,
            custom_fog_enabled: ms.custom_fog.enabled,
            custom_fog_color: ms.custom_fog.color,
            custom_fog_height_elmos: ms.custom_fog.height_elmos,
            custom_fog_atten: ms.custom_fog.atten,
            sun_color: ms.atmosphere.sun_color,
            sun_intensity: ms.lighting.sun_intensity,
            sky_color: ms.atmosphere.sky_color,
            sky_dir: ms.atmosphere.sky_dir,
            cloud_density: ms.atmosphere.cloud_density,
            cloud_color: ms.atmosphere.cloud_color,
            // MapSettings doesn't carry runtime upload state; the
            // renderer overrides this flag in `sync_to_frame` based
            // on whether a real cubemap is uploaded.
            skybox_enabled: false,
            sky_reflect_mod_enabled: false,
            specular_tex_enabled: false,
            grass_shading_tex_enabled: false,
            light_emission_tex_enabled: false,
            // Apply legacy detailTex only when the map has no splat
            // distribution texture -- matches engine routing of
            // detailTex to the playable area only in that case.
            detail_strength: if ms.resources.splat_distr_tex.is_empty() {
                1.0
            } else {
                0.0
            },
            splat_tex_scales: ms.resources.splat_tex_scales,
            splat_tex_mults: ms.resources.splat_tex_mults,
            // `advanced_splat_enabled` is a renderer-runtime flag,
            // overridden in `sync_to_frame` based on which textures
            // were actually uploaded. Map-settings can't know that.
            advanced_splat_enabled: false,
            splat_detail_diffuse_alpha: ms.resources.splat_detail_normal_diffuse_alpha,
            // Same: host computes this from map dimensions and sets
            // it via update_heightmap / sync_to_frame.
            elmo_per_render_xz: [1.0, 1.0],
            atmosphere_fog_start: ms.atmosphere.fog_start,
            atmosphere_fog_end: ms.atmosphere.fog_end,
            atmosphere_fog_color: ms.atmosphere.fog_color,
        }
    }
}

impl Default for SmfLighting {
    fn default() -> Self {
        Self {
            sun_dir: [0.0, 1.0, 2.0],
            ground_ambient: [0.5, 0.5, 0.5],
            ground_diffuse: [0.5, 0.5, 0.5],
            ground_specular: [0.1, 0.1, 0.1],
            ground_shadow_density: 0.8,
            // Engine default is 100.0 (MapInfo.cpp::ReadLight). Our 10.0
            // produced a much broader, dimmer spec lobe than engine on any
            // map that didn't override `specularExponent` in mapinfo.lua.
            specular_exponent: 100.0,
            water_absorb: [0.0, 0.0, 0.0],
            water_base: [0.0, 0.0, 0.0],
            water_min: [0.0, 0.0, 0.0],
            water_surface_color: [0.75, 0.8, 0.85],
            water_surface_alpha: 0.55,
            water_diffuse_color: [1.0, 1.0, 1.0],
            water_specular_color: [1.0, 1.0, 1.0],
            water_ambient_factor: 1.0,
            water_diffuse_factor: 1.0,
            water_specular_factor: 1.0,
            water_specular_power: 20.0,
            water_fresnel_min: 0.2,
            water_fresnel_max: 0.8,
            water_fresnel_power: 4.0,
            water_reflection_distortion: 1.0,
            water_perlin_amplitude: 0.9,
            custom_fog_enabled: false,
            custom_fog_color: [0.0, 0.0, 0.0],
            custom_fog_height_elmos: 0.0,
            custom_fog_atten: 0.0,
            sun_color: [1.0, 1.0, 1.0],
            sun_intensity: 1.0,
            sky_color: [0.1, 0.15, 0.7],
            sky_dir: [0.0, 0.0, -1.0],
            cloud_density: 0.5,
            cloud_color: [1.0, 1.0, 1.0],
            skybox_enabled: false,
            sky_reflect_mod_enabled: false,
            specular_tex_enabled: false,
            grass_shading_tex_enabled: false,
            light_emission_tex_enabled: false,
            detail_strength: 1.0,
            splat_tex_scales: [1.0, 1.0, 1.0, 1.0],
            splat_tex_mults: [1.0, 1.0, 1.0, 1.0],
            advanced_splat_enabled: false,
            splat_detail_diffuse_alpha: false,
            elmo_per_render_xz: [1.0, 1.0],
            atmosphere_fog_start: 0.1,
            atmosphere_fog_end: 1.0,
            atmosphere_fog_color: [0.7, 0.7, 0.8],
        }
    }
}

impl SmfLighting {
    fn to_uniform_slots(self) -> SmfUniformSlots {
        let s = self.sun_dir;
        let len = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt().max(1e-4);
        let s = [s[0] / len, s[1] / len, s[2] / len];
        SmfUniformSlots {
            sun_dir_exp: [s[0], s[1], s[2], self.specular_exponent],
            ground_ambient: [
                self.ground_ambient[0],
                self.ground_ambient[1],
                self.ground_ambient[2],
                0.0,
            ],
            ground_diffuse: [
                self.ground_diffuse[0],
                self.ground_diffuse[1],
                self.ground_diffuse[2],
                0.0,
            ],
            ground_specular: [
                self.ground_specular[0],
                self.ground_specular[1],
                self.ground_specular[2],
                // `.w` carries mapinfo `lighting.groundShadowDensity`.
                // Shader reads it at the shadow-coeff modulation site to
                // mirror `SMFFragProg.glsl:371`: `shadow_coeff = mix(1,
                // raw_shadow, density)`.
                self.ground_shadow_density,
            ],
            water_absorb: [
                self.water_absorb[0],
                self.water_absorb[1],
                self.water_absorb[2],
                0.0,
            ],
            water_base_color: [
                self.water_base[0],
                self.water_base[1],
                self.water_base[2],
                0.0,
            ],
            water_min_color: [self.water_min[0], self.water_min[1], self.water_min[2], 0.0],
            custom_fog_color_atten: [
                self.custom_fog_color[0],
                self.custom_fog_color[1],
                self.custom_fog_color[2],
                self.custom_fog_atten,
            ],
            custom_fog_params: [
                if self.custom_fog_enabled { 1.0 } else { 0.0 },
                self.custom_fog_height_elmos,
                // Repurposed: gates the map-edge extension between
                // sampling `grassShadingTex` (when set) and falling
                // back to the playable albedo (when unset). Belongs
                // in a future dedicated `extension_params` uniform;
                // packed here for now to avoid touching the layout.
                if self.grass_shading_tex_enabled {
                    1.0
                } else {
                    0.0
                },
                // Gates the engine's `SMF_LIGHT_EMISSION` apply-emission
                // stage in `terrain.wgsl` -- when 0 the shader skips
                // the blend; when 1 it applies `fragColor = fragColor *
                // (1 - emit.a) + emit.rgb` (`SMFFragProg.glsl:392-401`).
                if self.light_emission_tex_enabled {
                    1.0
                } else {
                    0.0
                },
            ],
            sun_color: [
                self.sun_color[0],
                self.sun_color[1],
                self.sun_color[2],
                self.sun_intensity,
            ],
            sky_color_density: [
                self.sky_color[0],
                self.sky_color[1],
                self.sky_color[2],
                self.cloud_density,
            ],
            sky_dir: [self.sky_dir[0], self.sky_dir[1], self.sky_dir[2], 0.0],
            cloud_color: [
                self.cloud_color[0],
                self.cloud_color[1],
                self.cloud_color[2],
                0.0,
            ],
            // x = skybox enabled, y = legacy detailTex strength,
            // z = sky_reflect_mod enabled (gates `SMF_SKY_REFLECTIONS`
            // path in terrain.wgsl).
            // w = specular_tex enabled (gates `SMF_SPECULAR_LIGHTING`
            // path -- when on, terrain samples per-pixel spec colour +
            // exponent from `specular_tex` instead of using the global
            // groundSpecularColor / groundSpecularExponent uniforms).
            skybox_params: [
                if self.skybox_enabled { 1.0 } else { 0.0 },
                self.detail_strength,
                if self.sky_reflect_mod_enabled {
                    1.0
                } else {
                    0.0
                },
                if self.specular_tex_enabled { 1.0 } else { 0.0 },
            ],
            splat_tex_scales: self.splat_tex_scales,
            splat_tex_mults: self.splat_tex_mults,
            splat_params: [
                self.elmo_per_render_xz[0],
                self.elmo_per_render_xz[1],
                if self.advanced_splat_enabled {
                    1.0
                } else {
                    0.0
                },
                if self.splat_detail_diffuse_alpha {
                    1.0
                } else {
                    0.0
                },
            ],
            atmosphere_fog: [self.atmosphere_fog_start, self.atmosphere_fog_end],
            atmosphere_fog_color: [
                self.atmosphere_fog_color[0],
                self.atmosphere_fog_color[1],
                self.atmosphere_fog_color[2],
                1.0,
            ],
        }
    }
}

struct SmfUniformSlots {
    sun_dir_exp: [f32; 4],
    ground_ambient: [f32; 4],
    ground_diffuse: [f32; 4],
    ground_specular: [f32; 4],
    water_absorb: [f32; 4],
    water_base_color: [f32; 4],
    water_min_color: [f32; 4],
    custom_fog_color_atten: [f32; 4],
    custom_fog_params: [f32; 4],
    sun_color: [f32; 4],
    sky_color_density: [f32; 4],
    sky_dir: [f32; 4],
    cloud_color: [f32; 4],
    skybox_params: [f32; 4],
    splat_tex_scales: [f32; 4],
    splat_tex_mults: [f32; 4],
    /// Atmosphere distance-fog fractions packed for shader consumption.
    /// xy hold the start/end fractions; the host multiplies by camera
    /// far-plane to fill `CameraUniform::fog_dists`.
    atmosphere_fog: [f32; 2],
    /// Mapinfo `atmosphere.fogColor` packed for `CameraUniform::fog_color`.
    atmosphere_fog_color: [f32; 4],
    splat_params: [f32; 4],
}

/// Per-map water surface parameters consumed by the BumpWater port in
/// `shaders/water.wgsl`. Packed into 4 vec4s for std140 alignment; field
/// names mirror Recoil's `WaterRendering` struct.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, PartialEq)]
struct WaterParamsUniform {
    /// rgb = surfaceColor, w = surfaceAlpha
    surface_color_alpha: [f32; 4],
    /// rgb = diffuseColor, w = diffuseFactor
    diffuse_color_factor: [f32; 4],
    /// rgb = specularColor, w = specularPower
    specular_color_power: [f32; 4],
    /// x = ambientFactor, y = specularFactor,
    /// z = reflectionDistortion, w = perlinAmplitude
    factors: [f32; 4],
    /// x = fresnelMin, y = fresnelMax, z = fresnelPower, w = unused
    fresnel: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<WaterParamsUniform>() == 80);

impl From<&SmfLighting> for WaterParamsUniform {
    fn from(l: &SmfLighting) -> Self {
        // Engine-side prescale (`rts/Rendering/Env/BumpWater.cpp:429,436`).
        // BumpWater bakes these as `#define` constants when it compiles the
        // shader, so the WGSL port has to apply the same scale before the
        // value reaches the uniform. Without them the surface tint runs to
        // double the engine's, which is what produced the white-wash look.
        const SURFACE_COLOR_SCALE: f32 = 0.4;
        const DIFFUSE_FACTOR_SCALE: f32 = 15.0;
        Self {
            surface_color_alpha: [
                l.water_surface_color[0] * SURFACE_COLOR_SCALE,
                l.water_surface_color[1] * SURFACE_COLOR_SCALE,
                l.water_surface_color[2] * SURFACE_COLOR_SCALE,
                l.water_surface_alpha,
            ],
            diffuse_color_factor: [
                l.water_diffuse_color[0],
                l.water_diffuse_color[1],
                l.water_diffuse_color[2],
                l.water_diffuse_factor * DIFFUSE_FACTOR_SCALE,
            ],
            specular_color_power: [
                l.water_specular_color[0],
                l.water_specular_color[1],
                l.water_specular_color[2],
                l.water_specular_power.max(1.0),
            ],
            factors: [
                l.water_ambient_factor,
                l.water_specular_factor,
                l.water_reflection_distortion,
                l.water_perlin_amplitude,
            ],
            fresnel: [
                l.water_fresnel_min,
                l.water_fresnel_max,
                l.water_fresnel_power.max(0.01),
                0.0,
            ],
        }
    }
}

/// The terrain rendering pipeline.
pub struct TerrainRenderer {
    render_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    /// Depth-only terrain pipeline used for casting into the shadow map.
    shadow_terrain_pipeline: wgpu::RenderPipeline,
    /// Shadow map (depth texture + light VP uniform + caster/receiver bind groups).
    shadow: crate::shadow::ShadowMap,
    /// Group-2 bind group for shadow_terrain caster (heightmap).
    shadow_caster_heightmap_bg: wgpu::BindGroup,
    shadow_caster_heightmap_bgl: wgpu::BindGroupLayout,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    /// Cached for `update_skybox` -- it rebuilds the camera bind group
    /// against a freshly-uploaded cubemap view.
    camera_bind_group_layout: wgpu::BindGroupLayout,
    skybox_sampler: wgpu::Sampler,
    /// Currently-bound skybox view. Starts as a 1x1 black cubemap and is
    /// replaced when `update_skybox` is called with a real cubemap.
    skybox_view: wgpu::TextureView,
    /// Optional: holds the cubemap texture so it isn't dropped while
    /// the view above is in use.
    #[allow(dead_code)]
    skybox_texture: Option<wgpu::Texture>,
    /// Whether the renderer has a non-default cubemap loaded. Sourced
    /// into `SmfLighting.skybox_enabled` and ultimately into the
    /// shader's `skybox_params.x` flag.
    skybox_enabled: bool,
    feature_renderer: Option<crate::features::FeatureRenderer>,
    // ── Group 1: albedo + metalmap + typemap ────────────────────────────────
    texture_bind_group_layout: wgpu::BindGroupLayout,
    texture_bind_group: wgpu::BindGroup,
    albedo_texture: wgpu::Texture,
    albedo_sampler: wgpu::Sampler,
    metalmap_texture: wgpu::Texture,
    typemap_texture: wgpu::Texture,
    /// Detail texture (mapinfo `resources.detailTex`). Starts as a 1x1
    /// mid-grey so the shader's `(detail - 0.5)` term contributes
    /// nothing until a real texture is uploaded.
    detail_texture: wgpu::Texture,
    detail_sampler: wgpu::Sampler,
    /// Four splat-detail-normal textures + a single distribution map.
    /// Defaults are 1x1 grey + zero distribution. `update_splat_textures`
    /// replaces all five and flips `advanced_splat_enabled` in the
    /// SmfLighting we sync at the next render.
    splat_detail_normal_1: wgpu::Texture,
    splat_detail_normal_2: wgpu::Texture,
    splat_detail_normal_3: wgpu::Texture,
    splat_detail_normal_4: wgpu::Texture,
    splat_distr_texture: wgpu::Texture,
    advanced_splat_enabled: bool,
    /// `skyReflectModTex` upload state. The 1x1 black default is
    /// **never sampled** by the shader -- the cubemap-mix branch in
    /// `terrain.wgsl` is gated behind `skybox_params.z > 0.5`
    /// (the `sky_reflect_mod_enabled` flag), so maps without this
    /// texture fall through to no env-reflection at all, matching the
    /// engine's `#ifdef SMF_SKY_REFLECTIONS` compile-out
    /// (`bar-recoil/rts/Map/SMF/SMFRenderState.cpp:117`). The 1x1
    /// placeholder exists only so the bind group always has a valid
    /// texture view; it's inert under the gate.
    sky_reflect_mod_texture: wgpu::Texture,
    sky_reflect_mod_enabled: bool,
    /// `specularTex` upload state. The 1x1 black default is **never
    /// sampled** by the shader: the per-pixel branch in `terrain.wgsl`
    /// is gated behind `skybox_params.w > 0.5` (the
    /// `specular_tex_enabled` flag), and the `#else` path uses the
    /// global `ground_specular.xyz` colour and `sun_dir_exp.w`
    /// exponent uniforms. That mirrors the engine's
    /// `#ifdef SMF_SPECULAR_LIGHTING ... #else ...` split in
    /// `SMFFragProg.glsl:403-416`. The placeholder exists only to
    /// keep the bind group valid; it's inert under the gate.
    specular_tex_texture: wgpu::Texture,
    specular_tex_enabled: bool,
    /// `grassShadingTex` upload state. 1x1 grey default; replaced by
    /// `update_grass_shading_tex` when the map specifies one. Gates
    /// the extension shader's texture choice via
    /// `grass_shading_tex_enabled` -- when off, the extension samples
    /// the playable albedo instead.
    grass_shading_tex_texture: wgpu::Texture,
    grass_shading_tex_enabled: bool,
    /// `lightEmissionTex` upload state. 1x1 `(0,0,0,0)` default so the
    /// emission blend collapses to identity (no glow) until a real
    /// texture is uploaded. Engine path `SMF_LIGHT_EMISSION`
    /// (`SMFFragProg.glsl:392-401`). `light_emission_enabled` gates
    /// the shader's apply-emission stage so even a stale texture
    /// from a prior map doesn't keep glowing after a switch.
    light_emission_tex_texture: wgpu::Texture,
    light_emission_tex_enabled: bool,
    has_albedo: bool,
    // ── Group 2: planar reflection (b0/b1) + planar refraction (b2/b3) + water params (b4) ──────
    water_planes_bind_group_layout: wgpu::BindGroupLayout,
    reflection_sampler: wgpu::Sampler,
    refraction_sampler: wgpu::Sampler,
    /// Non-filtering sampler for the refraction-pass depth texture
    /// (used by the water shader's depth-aware refraction mixback).
    refraction_depth_sampler: wgpu::Sampler,
    water_planes_bind_group: wgpu::BindGroup,
    water_planes_bind_group_dummy: wgpu::BindGroup,
    /// Per-map water surface uniform (BumpWater inputs). Re-uploaded each
    /// frame in `render_internal` from the current `SmfLighting`.
    water_params_buffer: wgpu::Buffer,
    /// Last `WaterParamsUniform` we uploaded -- used so the per-frame log
    /// in `render_internal` only fires on change instead of spamming every
    /// frame. Diagnostic only; not load-bearing.
    last_water_params: WaterParamsUniform,
    // ── Group 3: water_normal (bindings 0,1) + heightmap (binding 2) ─────────
    #[allow(dead_code)]
    water_normal_texture: wgpu::Texture,
    water_normal_sampler: wgpu::Sampler,
    water_normal_view: wgpu::TextureView,
    heightmap_bind_group_layout: wgpu::BindGroupLayout,
    heightmap_bind_group: wgpu::BindGroup,
    heightmap_texture: wgpu::Texture,
    /// Per-fragment surface normal map keyed off `heightmap_texture`.
    /// Stores world-space (X, Z) of the unit normal; Y reconstructed
    /// in the shader. Re-baked whenever `update_heightmap` runs.
    normal_map_texture: wgpu::Texture,
    // ── Geometry ────────────────────────────────────────────────────────────
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    num_indices: u32,
    /// Index where the water-plane sub-range starts in `index_buffer`. The
    /// main pass draws `[0, water_index_offset)` for terrain ground, then the
    /// feature pass runs, then `[water_index_offset, num_indices)` for water.
    /// This ordering lets underwater features write into the depth buffer
    /// before the water surface, so the water shader's alpha-blend correctly
    /// composites the features instead of depth-culling them.
    water_index_offset: u32,
    /// Grid resolution used when building the flat terrain mesh.
    grid_n: u32,
    // ── Gamma-encode post-process pass ──────────────────────────────────────
    /// Fullscreen pipeline that samples `output_texture` (BAR's perceptual
    /// pixels) and writes `pow(c, 2.2)` into `display_texture`. egui samples
    /// the display texture, the sRGB swapchain re-encodes back to the raw
    /// perceptual byte, and the display gamma decodes to V^2.2 -- the
    /// in-engine appearance. Without this pass the sRGB swapchain leaves
    /// the displayed intensity at V (too bright, washed-out highlights).
    gamma_pipeline: wgpu::RenderPipeline,
    gamma_bgl: wgpu::BindGroupLayout,
    gamma_sampler: wgpu::Sampler,
    gamma_bind_group: Option<wgpu::BindGroup>,
    /// Single-float uniform driving `gamma_encode.wgsl`'s pow exponent.
    /// Live-tunable via the viewport debug overlay so the right value
    /// can be dialled in against an in-engine reference. Padded to 16
    /// bytes for std140.
    gamma_uniform_buffer: wgpu::Buffer,
    display_texture: Option<wgpu::Texture>,
    display_view: Option<wgpu::TextureView>,
    output_format: wgpu::TextureFormat,
    // ── Output targets ──────────────────────────────────────────────────────
    depth_texture: Option<wgpu::TextureView>,
    depth_format: wgpu::TextureFormat,
    output_texture: Option<wgpu::Texture>,
    output_view: Option<wgpu::TextureView>,
    reflection_texture: Option<wgpu::Texture>,
    reflection_view: Option<wgpu::TextureView>,
    reflection_depth_view: Option<wgpu::TextureView>,
    refraction_texture: Option<wgpu::Texture>,
    refraction_view: Option<wgpu::TextureView>,
    refraction_depth_view: Option<wgpu::TextureView>,
    pub width: u32,
    pub height: u32,
    // ── Cached per-frame state ──────────────────────────────────────────────
    height_scale: f32,
    /// Elmo span of the heightmap; needed by the shader's underwater
    /// absorption to convert render-space Y back into engine elmos.
    height_range_elmos: f32,
    /// Elmos per unit of render-space XZ -- mirrors the XZ axes of
    /// `height_range_elmos`, needed by the splat-detail shader path.
    elmo_per_render_xz: [f32; 2],
    water_y: f32,
    water_color: [f32; 3],
    smf_lighting: SmfLighting,
    brush_cursor: Option<(f32, f32, f32)>,
    x_extent: f32,
    z_extent: f32,
    time: f32,
    quality_high: bool,
}

/// Generate a tileable noise-based water normal map.
///
/// Earlier this was 3 low-frequency sine octaves, but the result is
/// visibly periodic when the water shader's 4-octave sampler tiles it at
/// large scales (the diagonal stripes you can see in `water-cp1.png`). A
/// hash-based value-noise field at multiple octaves doesn't repeat in a
/// recognisable way at any sample scale, so the tiled appearance breaks up.
fn make_water_normal_map(size: u32) -> Vec<u8> {
    // 2D hash → [0, 1] from integer cell coords. Cheap, no self-correlation.
    fn hash(x: i32, y: i32) -> f32 {
        let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263);
        h ^= h >> 13;
        h = h.wrapping_mul(1274126177);
        h ^= h >> 16;
        (h & 0x00FF_FFFF) as f32 / 0x00FF_FFFF as f32
    }
    // Smoothed value noise (tileable across `period` cells), bilinear with
    // a fade curve so derivatives are continuous across cell boundaries.
    fn value_noise(u: f32, v: f32, period: i32) -> f32 {
        let scaled_u = u * period as f32;
        let scaled_v = v * period as f32;
        let xi = scaled_u.floor() as i32;
        let yi = scaled_v.floor() as i32;
        let xf = scaled_u - xi as f32;
        let yf = scaled_v - yi as f32;
        let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
        let fx = fade(xf);
        let fy = fade(yf);
        // Wrap cell coords through `period` so the noise tiles cleanly.
        let wrap = |c: i32| c.rem_euclid(period);
        let a = hash(wrap(xi), wrap(yi));
        let b = hash(wrap(xi + 1), wrap(yi));
        let c = hash(wrap(xi), wrap(yi + 1));
        let d = hash(wrap(xi + 1), wrap(yi + 1));
        let ab = a * (1.0 - fx) + b * fx;
        let cd = c * (1.0 - fx) + d * fx;
        ab * (1.0 - fy) + cd * fy
    }

    let n = size as usize;
    let mut data = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let u = x as f32 / n as f32;
            let v = y as f32 / n as f32;
            // Three octaves of value noise; periods chosen so the texture
            // tiles seamlessly at every octave (each period divides `size`
            // for a 128-texel input -> 8 / 16 / 32 cells).
            let h = 0.50 * value_noise(u, v, 8)
                + 0.30 * value_noise(u, v, 16)
                + 0.20 * value_noise(u, v, 32);
            // Central differences on the noise field give a continuous
            // gradient suitable for use as a tangent-space normal.
            let eps = 1.0 / n as f32;
            let hx = 0.50 * value_noise(u + eps, v, 8)
                + 0.30 * value_noise(u + eps, v, 16)
                + 0.20 * value_noise(u + eps, v, 32);
            let hy = 0.50 * value_noise(u, v + eps, 8)
                + 0.30 * value_noise(u, v + eps, 16)
                + 0.20 * value_noise(u, v + eps, 32);
            // Slope of the height field; scaled so the normal lies firmly
            // away from straight-up but not so steep that pow(N.V, ~100)
            // for the sun specular dies completely.
            let nx_raw = (h - hx) * 6.0;
            let ny_raw = (h - hy) * 6.0;
            let nz_raw = (1.0 - nx_raw * nx_raw - ny_raw * ny_raw)
                .max(0.04_f32)
                .sqrt();
            let to_u8 = |f: f32| ((f * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0) as u8;
            data.push(to_u8(nx_raw));
            data.push(to_u8(ny_raw));
            data.push(to_u8(nz_raw));
            data.push(255u8);
        }
    }
    data
}

/// Create a 1x1 R32Float texture with value `v`. Used for the default heightmap.
fn make_default_r32float(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    v: f32,
) -> wgpu::Texture {
    let data = v.to_ne_bytes();
    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    )
}

/// Default 1x1 normal map encoding a flat upward-pointing surface
/// normal (0, 1, 0). Stored as (X, Z) = (0, 0) in Rg8Snorm; the
/// shader reconstructs Y = sqrt(1 - 0 - 0) = 1.
fn make_default_normal_map(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("normal_map_default"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg8Snorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &[0u8, 0u8],
    )
}

/// CPU-generate a per-texel surface normal map from a heightmap. Output
/// is two-channel signed bytes packing world-space (X, Z) of the unit
/// normal; the fragment shader reconstructs Y. Uses the same central-
/// difference math the vertex shader previously did, baked once per
/// heightmap upload so fragment shading reads off a texture lookup
/// instead of an interpolated per-vertex normal.
fn build_normal_map_bytes(
    hm: &Heightmap,
    height_scale: f32,
    x_extent: f32,
    z_extent: f32,
) -> Vec<i8> {
    let w = hm.width().max(1);
    let h = hm.height().max(1);
    let world_dx = (2.0 * x_extent) / w as f32;
    let world_dz = (2.0 * z_extent) / h as f32;
    let mut out = Vec::with_capacity((w * h * 2) as usize);
    for z in 0..h {
        for x in 0..w {
            let xp = (x + 1).min(w - 1);
            let xn = x.saturating_sub(1);
            let zp = (z + 1).min(h - 1);
            let zn = z.saturating_sub(1);
            let h_xp = hm.get(xp, z).unwrap_or(0.0);
            let h_xn = hm.get(xn, z).unwrap_or(0.0);
            let h_zp = hm.get(x, zp).unwrap_or(0.0);
            let h_zn = hm.get(x, zn).unwrap_or(0.0);
            let dy_dx = (h_xp - h_xn) * height_scale / (2.0 * world_dx);
            let dy_dz = (h_zp - h_zn) * height_scale / (2.0 * world_dz);
            let nx = -dy_dx;
            let ny = 1.0;
            let nz = -dy_dz;
            let inv_len = 1.0 / (nx * nx + ny * ny + nz * nz).sqrt();
            let nx_u = nx * inv_len;
            let nz_u = nz * inv_len;
            out.push((nx_u * 127.0).clamp(-127.0, 127.0) as i8);
            out.push((nz_u * 127.0).clamp(-127.0, 127.0) as i8);
        }
    }
    out
}

/// Ensure a mip chain runs all the way down to 1x1, synthesising any
/// missing levels via 2x2 box filtering on the RGBA8 data.
///
/// Input invariant: `mips[0]` is the base level. Subsequent entries are
/// `(w >> level)`-sized half-decay if present. If the caller passed only
/// the base level (e.g. PNG / TGA / single-mip DDS) we generate the rest
/// down to 1x1 so the GPU sampler has every level to bilerp between.
///
/// Box-filter mip generation is a close-enough approximation of what the
/// engine gets from `glGenerateMipmap` on the OpenGL side; both are
/// simple 2x2 averages with no normal-renormalisation. For splat
/// detail-normals the loss in normal preservation is well within the
/// tolerance for "no longer aliases into per-fragment grain".
fn ensure_full_mip_chain(mut chain: Vec<(Vec<u8>, u32, u32)>) -> Vec<(Vec<u8>, u32, u32)> {
    if chain.is_empty() {
        return chain;
    }
    let (_, base_w, base_h) = chain[0];
    let max_dim = base_w.max(base_h).max(1);
    let target_count = (32 - max_dim.leading_zeros()) as usize; // log2 + 1
    while chain.len() < target_count {
        let (prev_rgba, pw, ph) = chain.last().unwrap();
        let pw = *pw;
        let ph = *ph;
        let nw = (pw / 2).max(1);
        let nh = (ph / 2).max(1);
        if nw == pw && nh == ph {
            break;
        }
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                // 2x2 box filter. Edge pixels get clamped sampling so a
                // non-power-of-2 base doesn't read out of bounds.
                let sx0 = (x * 2).min(pw - 1);
                let sx1 = (x * 2 + 1).min(pw - 1);
                let sy0 = (y * 2).min(ph - 1);
                let sy1 = (y * 2 + 1).min(ph - 1);
                let i00 = ((sy0 * pw + sx0) * 4) as usize;
                let i10 = ((sy0 * pw + sx1) * 4) as usize;
                let i01 = ((sy1 * pw + sx0) * 4) as usize;
                let i11 = ((sy1 * pw + sx1) * 4) as usize;
                for c in 0..4 {
                    let sum = prev_rgba[i00 + c] as u32
                        + prev_rgba[i10 + c] as u32
                        + prev_rgba[i01 + c] as u32
                        + prev_rgba[i11 + c] as u32;
                    let out_i = ((y * nw + x) * 4) as usize;
                    next[out_i + c] = (sum / 4) as u8;
                }
            }
        }
        chain.push((next, nw, nh));
    }
    chain
}

/// Create a 1x1 R8Unorm texture with value `v` (0.0..1.0). Used for default
/// metalmap / typemap textures. R8Unorm is universally filterable.
fn make_default_r8unorm(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    v: f32,
) -> wgpu::Texture {
    let data = [(v.clamp(0.0, 1.0) * 255.0).round() as u8];
    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    )
}

impl TerrainRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        output_format: wgpu::TextureFormat,
    ) -> Self {
        // Assemble WGSL from Recoil ports + original shaders. Concatenation
        // gives the same effect as #include; WGSL has no preprocessor.
        let modern_sky_source = include_str!("../../../shaders/recoil/modern_sky.wgsl");
        let smf_ground_source = include_str!("../../../shaders/recoil/smf_ground.wgsl");
        let water_source = include_str!("../../../shaders/water.wgsl");
        let terrain_source = include_str!("../../../shaders/terrain.wgsl");
        let shader_source =
            format!("{modern_sky_source}\n{smf_ground_source}\n{water_source}\n{terrain_source}");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain_shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        // Group 0: camera uniform
        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("camera_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // Skybox cubemap (sourced from mapinfo `atmosphere.skyBox`).
                    // When the map doesn't specify a skybox -- or until the
                    // user re-imports a map that does -- this is a 1x1 black
                    // cubemap and `camera.skybox_params.x` is 0, telling the
                    // sky shader to fall back to the procedural ModernSky
                    // path. Lives on the camera bind group so every pipeline
                    // that binds group 0 sees the same skybox without
                    // additional plumbing.
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::Cube,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        // Group 1: albedo (tex + sampler) + metalmap (tex) + typemap (tex) + shared material sampler
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("texture_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    // Detail texture (mapinfo `resources.detailTex`).
                    // Sampled at world.xz * detail_tex_params.x, then
                    // centred via `-0.5` and added to the diffuse colour
                    // BEFORE the shade multiply -- matches engine's
                    // `(diffuseCol + detailCol) * shadeInt`. Defaults
                    // to a 1x1 grey (0.5, 0.5, 0.5) so the contribution
                    // is zero when no detail texture is loaded.
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 6,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    // Splat detail-normal textures + distribution
                    // (mapinfo `splatDetailNormalTex{1..4}` and
                    // `splatDistrTex`). When all five are uploaded
                    // and `splat_params.z >= 0.5`, the terrain shader
                    // computes the engine's `splatDetailStrength.y`
                    // contribution and adds it to the diffuse before
                    // the shade multiply. Defaults are 1x1 (127, 127,
                    // 127, 127), i.e. a centered zero contribution so
                    // a missing splat-detail texture yields no change.
                    wgpu::BindGroupLayoutEntry {
                        binding: 7,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 8,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 9,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 10,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 11,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    // Sky-reflection mod texture (mapinfo `skyReflectModTex`).
                    // Engine `SMF_SKY_REFLECTIONS` path: per-pixel mask
                    // for where the skybox cubemap reflects on terrain.
                    // Defaults to 1x1 black so a missing texture yields
                    // zero reflection mix.
                    wgpu::BindGroupLayoutEntry {
                        binding: 12,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    // Per-pixel specular texture (mapinfo `specularTex`).
                    // Engine `SMF_SPECULAR_LIGHTING` path: `.rgb` is the
                    // per-pixel specular colour, `.a * 16` the per-pixel
                    // exponent. Defaults to 1x1 black so a missing texture
                    // yields zero spec contribution everywhere (rather
                    // than the global `groundSpecularColor`, which would
                    // produce hot whole-surface spec on maps that author
                    // a non-zero global colour).
                    wgpu::BindGroupLayoutEntry {
                        binding: 13,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    // Map-edge extension texture (mapinfo
                    // `grassShadingTex`). Sampled by the extension
                    // branch of the terrain shader to texture the area
                    // outside the playable map -- maps that set this
                    // (e.g. Onyx Cauldron) get custom rocks/etc. for
                    // the off-map region; maps that don't fall back to
                    // the playable albedo. Defaults to 1x1 grey so the
                    // shader's `grass_shading_enabled` flag controls
                    // path selection rather than texture lookup.
                    wgpu::BindGroupLayoutEntry {
                        binding: 14,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    // Self-illumination texture (mapinfo `lightEmissionTex`,
                    // engine path `SMF_LIGHT_EMISSION` -- see
                    // `bar-recoil/rts/Map/SMF/SMFFragProg.glsl:392-401`).
                    // Sampled at `specTexCoords`; alpha gates the blend
                    // (`fragColor = fragColor * (1 - emit.a) + emit.rgb`)
                    // so the glow is unshadowed and overrides whatever's
                    // underneath. Defaults to 1x1 (0,0,0,0) so the blend
                    // is a no-op for maps that don't ship an emission
                    // texture.
                    wgpu::BindGroupLayoutEntry {
                        binding: 15,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });

        // Group 2: planar reflection (b0/b1) + planar refraction (b2/b3).
        let water_planes_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("water_planes_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    // BumpWater surface params (per-map values from mapinfo.lua).
                    // 80-byte uniform; matches `WaterParamsUniform` and the
                    // `water_params` declaration in shaders/water.wgsl.
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // Refraction-pass depth, sampled by the water shader
                    // for the engine's depth-aware refraction mixback
                    // (BumpWaterFS:304-314). NonFiltering sampler --
                    // wgpu only allows depth textures with non-filtering
                    // sampling on a regular sampler binding (or
                    // sampler_comparison on a `texture_depth_2d`); we
                    // pick the former since we only need raw depth
                    // reads, not shadow-style compares.
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 6,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                        count: None,
                    },
                ],
            });

        // Group 3: water_normal map (bindings 0-1, filterable) + heightmap (binding 2, non-filterable).
        let heightmap_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("group3_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    // Pre-baked surface normal map (Rg8Snorm). Sampled by
                    // fs_main for engine-parity per-fragment shading;
                    // reuses the group-1 filtering sampler.
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });

        // Shadow map (depth texture + light_view_proj uniform + bind groups).
        // Layouts are constructed here so the main terrain pipeline can include
        // the receiver BGL as group 4, matching `shaders/terrain.wgsl`.
        let shadow = crate::shadow::ShadowMap::new(device);

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain_pipeline_layout"),
            bind_group_layouts: &[
                &camera_bind_group_layout,       // group 0: camera uniform
                &texture_bind_group_layout,      // group 1: albedo/metalmap/typemap
                &water_planes_bind_group_layout, // group 2: reflection + refraction
                &heightmap_bind_group_layout,    // group 3: water normal + heightmap
                &shadow.receiver_bgl,            // group 4: shadow tex + sampler + light_vp
            ],
            push_constant_ranges: &[],
        });

        // Caster pipeline layout: group 0 camera (for x_extent/z_extent/
        // height_scale), group 1 shadow uniform (light_view_proj), group 2
        // heightmap.
        let shadow_caster_heightmap_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("shadow_caster_heightmap_bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }],
            });

        let depth_format = wgpu::TextureFormat::Depth32Float;

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[TerrainVertex::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sky_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky_pipeline_layout"),
            bind_group_layouts: &[&camera_bind_group_layout],
            push_constant_ranges: &[],
        });
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky_pipeline"),
            layout: Some(&sky_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_sky"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_sky"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let smf = SmfLighting::default().to_uniform_slots();
        let camera_uniform = CameraUniform {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            inv_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            camera_pos: [0.0, 0.0, 0.0],
            has_texture: 0,
            height_scale: 0.3,
            water_r: 0.2,
            water_g: 0.4,
            water_b: 0.7,
            water_y: -1.0,
            time: 0.0,
            skip_water: 0.0,
            height_range_elmos: 1.0,
            screen_w: 512.0,
            screen_h: 512.0,
            x_extent: 0.5,
            z_extent: 0.5,
            sun_dir_exp: smf.sun_dir_exp,
            ground_ambient: smf.ground_ambient,
            ground_diffuse: smf.ground_diffuse,
            ground_specular: smf.ground_specular,
            water_absorb: smf.water_absorb,
            water_base_color: smf.water_base_color,
            water_min_color: smf.water_min_color,
            brush_cursor: [0.0, 0.0, 0.0, 0.0],
            clip_plane: NO_CLIP,
            custom_fog_color_atten: smf.custom_fog_color_atten,
            custom_fog_params: smf.custom_fog_params,
            sun_color: smf.sun_color,
            sky_color_density: smf.sky_color_density,
            sky_dir: smf.sky_dir,
            cloud_color: smf.cloud_color,
            skybox_params: smf.skybox_params,
            splat_tex_scales: smf.splat_tex_scales,
            splat_tex_mults: smf.splat_tex_mults,
            splat_params: smf.splat_params,
            // Placeholder camera far-plane scaling for the initial empty
            // uniform; `render_internal` recomputes per-frame from the
            // live camera.
            fog_dists: [smf.atmosphere_fog[0], smf.atmosphere_fog[1], 0.0, 0.0],
            fog_color: smf.atmosphere_fog_color,
        };

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera_buffer"),
            contents: bytemuck::bytes_of(&camera_uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Default skybox cubemap: 1x1 black, used until a map's actual
        // skybox DDS is loaded via `update_skybox`. wgpu requires the
        // bind group to have a real cubemap view regardless of whether
        // the shader uses it, so we create this once at construction.
        let skybox_default_data: [u8; 4 * 6] = [0; 4 * 6];
        let skybox_default_tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("skybox_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 6,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &skybox_default_data,
        );
        let skybox_default_view = skybox_default_tex.create_view(&wgpu::TextureViewDescriptor {
            label: Some("skybox_default_view"),
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });
        let skybox_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("skybox_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera_bind_group"),
            layout: &camera_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&skybox_default_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&skybox_sampler),
                },
            ],
        });

        let albedo_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("albedo_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            // Linear mip interpolation so the map-edge extension's
            // mirrored sampling reads cleanly at oblique angles without
            // sparkle. Other consumers (playable albedo) already had
            // chain-aware mip selection in their textures; this sampler
            // is shared, so flipping to Linear lifts quality everywhere
            // without a behavioural change for the playable area.
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let white: [u8; 4] = [255, 255, 255, 255];
        let albedo_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("albedo_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &white,
        );
        let metalmap_texture = make_default_r8unorm(device, queue, "metalmap_default", 0.0);
        let typemap_texture = make_default_r8unorm(device, queue, "typemap_default", 0.0);

        // Default detail texture: 1x1 mid-grey. The shader subtracts
        // 0.5 from the sample (mirroring engine's `texture - 0.5`),
        // so a 0.5 sample yields no contribution -- safe no-op when
        // no map-authored detail texture has been uploaded.
        let mid_grey: [u8; 4] = [128, 128, 128, 255];
        let detail_default = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("detail_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &mid_grey,
        );
        // Shared sampler for the detail / splat-detail-normal / splat
        // distribution / sky-reflection-mask textures. Trilinear with 16x
        // anisotropy. Matches engine behaviour:
        //   - `GL_LINEAR_MIPMAP_LINEAR` -> wgpu `mipmap_filter: Linear` +
        //     min/mag/`Linear` for the in-mip and inter-mip filtering steps.
        //   - `GL_TEXTURE_MAX_ANISOTROPY_EXT` defaults to 16 in BAR
        //     -> wgpu `anisotropy_clamp: 16`. wgpu silently caps to the
        //     adapter's max anisotropy support (most desktop drivers do 16).
        // Without anisotropy + mips, the splat-detail-normal textures
        // sampled at world-space elmo scales alias into per-fragment normal
        // noise at oblique angles, which lit every shadowed surface as
        // grain.
        let detail_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("detail_sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });

        // Default splat textures: 1x1 (127, 127, 127, 127). The shader
        // does `sample * 2 - 1` so 127/255 ≈ 0.498 → centered ≈ -0.004,
        // i.e. effectively zero contribution. Distribution gets the
        // same default; with no real distribution loaded the shader's
        // `splat_params.z` flag (advanced enabled) is false so the
        // path isn't even taken.
        let splat_default_data: [u8; 4] = [127, 127, 127, 127];
        let make_splat_default = |label: &str| {
            device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    // Distribution texture also goes through this closure;
                    // both formats happen to be the same (linear data).
                    format: SPLAT_DETAIL_NORMAL_TEX_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &splat_default_data,
            )
        };
        let splat_dn_default_1 = make_splat_default("splat_dn_default_1");
        let splat_dn_default_2 = make_splat_default("splat_dn_default_2");
        let splat_dn_default_3 = make_splat_default("splat_dn_default_3");
        let splat_dn_default_4 = make_splat_default("splat_dn_default_4");
        let splat_distr_default = make_splat_default("splat_distr_default");

        // Sky-reflection mod default: 1x1 black -> mix factor 0
        // everywhere -> zero reflection contribution until a real
        // map-authored texture is uploaded.
        let sky_reflect_mod_default_data: [u8; 4] = [0, 0, 0, 255];
        let sky_reflect_mod_default = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("sky_reflect_mod_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SKY_REFLECT_MOD_TEX_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &sky_reflect_mod_default_data,
        );

        // Specular texture default: 1x1 black -> per-pixel spec colour = 0
        // and alpha-driven exponent = 0 -> no spec contribution everywhere
        // until a real map-authored texture is uploaded. The `specular_tex_enabled`
        // flag also has to be set on for the shader to take this path at
        // all, so the default texture is mostly belt-and-braces.
        let specular_tex_default_data: [u8; 4] = [0, 0, 0, 255];
        let specular_tex_default = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("specular_tex_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SPECULAR_TEX_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &specular_tex_default_data,
        );

        // 1x1 mid-grey default for grassShadingTex -- only sampled
        // when `grass_shading_tex_enabled` is true, so the contents
        // are inert in the default case.
        let grass_shading_tex_default_data: [u8; 4] = [127, 127, 127, 255];
        let grass_shading_tex_default = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("grass_shading_tex_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &grass_shading_tex_default_data,
        );

        // 1x1 (0, 0, 0, 0) default for lightEmissionTex. Engine blend is
        // `fragColor = fragColor * (1 - emit.a) + emit.rgb`; with alpha
        // zero the blend collapses to identity (no glow). Maps that
        // ship a real emission texture get it via
        // `update_light_emission_tex`.
        let light_emission_tex_default_data: [u8; 4] = [0, 0, 0, 0];
        let light_emission_tex_default = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("light_emission_tex_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &light_emission_tex_default_data,
        );

        let texture_bind_group = {
            let av = albedo_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mv = metalmap_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let tv = typemap_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let dv = detail_default.create_view(&wgpu::TextureViewDescriptor::default());
            let sd1 = splat_dn_default_1.create_view(&wgpu::TextureViewDescriptor::default());
            let sd2 = splat_dn_default_2.create_view(&wgpu::TextureViewDescriptor::default());
            let sd3 = splat_dn_default_3.create_view(&wgpu::TextureViewDescriptor::default());
            let sd4 = splat_dn_default_4.create_view(&wgpu::TextureViewDescriptor::default());
            let sdv = splat_distr_default.create_view(&wgpu::TextureViewDescriptor::default());
            let srmv = sky_reflect_mod_default.create_view(&wgpu::TextureViewDescriptor::default());
            let specv = specular_tex_default.create_view(&wgpu::TextureViewDescriptor::default());
            let gstv =
                grass_shading_tex_default.create_view(&wgpu::TextureViewDescriptor::default());
            let letv =
                light_emission_tex_default.create_view(&wgpu::TextureViewDescriptor::default());
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("texture_bind_group"),
                layout: &texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&av),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&albedo_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&mv),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&tv),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&albedo_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(&dv),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::Sampler(&detail_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: wgpu::BindingResource::TextureView(&sd1),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: wgpu::BindingResource::TextureView(&sd2),
                    },
                    wgpu::BindGroupEntry {
                        binding: 9,
                        resource: wgpu::BindingResource::TextureView(&sd3),
                    },
                    wgpu::BindGroupEntry {
                        binding: 10,
                        resource: wgpu::BindingResource::TextureView(&sd4),
                    },
                    wgpu::BindGroupEntry {
                        binding: 11,
                        resource: wgpu::BindingResource::TextureView(&sdv),
                    },
                    wgpu::BindGroupEntry {
                        binding: 12,
                        resource: wgpu::BindingResource::TextureView(&srmv),
                    },
                    wgpu::BindGroupEntry {
                        binding: 13,
                        resource: wgpu::BindingResource::TextureView(&specv),
                    },
                    wgpu::BindGroupEntry {
                        binding: 14,
                        resource: wgpu::BindingResource::TextureView(&gstv),
                    },
                    wgpu::BindGroupEntry {
                        binding: 15,
                        resource: wgpu::BindingResource::TextureView(&letv),
                    },
                ],
            })
        };

        let reflection_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("reflection_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let sky_default: [u8; 4] = [180, 200, 230, 255];
        let reflection_default_tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("reflection_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &sky_default,
        );
        let reflection_default_view =
            reflection_default_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let refraction_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("refraction_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let refraction_default_data: [u8; 4] = [80, 110, 140, 255];
        let refraction_default_tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("refraction_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &refraction_default_data,
        );
        let refraction_default_view =
            refraction_default_tex.create_view(&wgpu::TextureViewDescriptor::default());

        // Water surface params uniform (BumpWater inputs). Initial contents
        // are SmfLighting defaults; `render_internal` re-uploads each frame
        // from the current map's settings.
        let water_params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water_params_uniform"),
            size: std::mem::size_of::<WaterParamsUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &water_params_buffer,
            0,
            bytemuck::bytes_of(&WaterParamsUniform::from(&SmfLighting::default())),
        );

        // Default refraction-depth texture: 1x1 depth = 1.0 (far). Used
        // before resize() creates the real one. With depth = 1.0 the
        // mixback computation yields 0 everywhere (nothing's closer
        // than the water plane), which is the right no-op.
        let refraction_depth_default = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("refraction_depth_default"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: depth_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let refraction_depth_default_view =
            refraction_depth_default.create_view(&wgpu::TextureViewDescriptor::default());
        let refraction_depth_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("refraction_depth_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let make_water_planes_bg =
            |refl_view: &wgpu::TextureView,
             refr_view: &wgpu::TextureView,
             refr_depth_view: &wgpu::TextureView| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("water_planes_bind_group"),
                    layout: &water_planes_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(refl_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&reflection_sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(refr_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::Sampler(&refraction_sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: water_params_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::TextureView(refr_depth_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: wgpu::BindingResource::Sampler(&refraction_depth_sampler),
                        },
                    ],
                })
            };
        let water_planes_bind_group = make_water_planes_bg(
            &reflection_default_view,
            &refraction_default_view,
            &refraction_depth_default_view,
        );
        let water_planes_bind_group_dummy = make_water_planes_bg(
            &reflection_default_view,
            &refraction_default_view,
            &refraction_depth_default_view,
        );

        let water_normal_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("water_normal_sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let water_normal_data = make_water_normal_map(128);
        let water_normal_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("water_normal"),
                size: wgpu::Extent3d {
                    width: 128,
                    height: 128,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &water_normal_data,
        );
        let water_normal_view =
            water_normal_texture.create_view(&wgpu::TextureViewDescriptor::default());
        // Default 1x1 heightmap (zero height) + default flat normal map
        // until update_heightmap is called. Group 3 bind group combines
        // water_normal (0,1), heightmap (2), and normal_map (3).
        let heightmap_texture = make_default_r32float(device, queue, "heightmap_default", 0.0);
        let heightmap_view = heightmap_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let normal_map_texture = make_default_normal_map(device, queue);
        let normal_map_view =
            normal_map_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let heightmap_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("group3_bind_group"),
            layout: &heightmap_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&water_normal_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&water_normal_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&heightmap_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&normal_map_view),
                },
            ],
        });

        // Shadow caster heightmap bind group: same texture, single-binding
        // layout for the depth-only terrain pipeline.
        let shadow_caster_heightmap_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow_caster_heightmap_bg"),
            layout: &shadow_caster_heightmap_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&heightmap_view),
            }],
        });

        // Shadow caster pipeline for terrain. Writes only depth into the shadow
        // map; the heightmap displacement matches `vs_main` so the shadow
        // silhouette matches the rendered terrain.
        let shadow_terrain_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadow_terrain_shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../shaders/shadow_terrain.wgsl").into(),
            ),
        });
        let shadow_caster_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow_caster_terrain_layout"),
            bind_group_layouts: &[
                &camera_bind_group_layout,    // group 0: camera (extents + height_scale)
                &shadow.caster_bgl,           // group 1: light view-proj
                &shadow_caster_heightmap_bgl, // group 2: heightmap
            ],
            push_constant_ranges: &[],
        });
        let shadow_terrain_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("shadow_terrain_pipeline"),
                layout: Some(&shadow_caster_layout),
                vertex: wgpu::VertexState {
                    module: &shadow_terrain_shader,
                    entry_point: Some("vs_shadow_terrain"),
                    buffers: &[TerrainVertex::desc()],
                    compilation_options: Default::default(),
                },
                // Depth-only: no fragment color output. The FS just runs the
                // `discard` for skirt/water vertices.
                fragment: Some(wgpu::FragmentState {
                    module: &shadow_terrain_shader,
                    entry_point: Some("fs_shadow_terrain"),
                    targets: &[],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: crate::shadow::ShadowMap::FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    // Small slope-scale bias attenuates depth acne on the
                    // displaced terrain mesh without pushing feature contact
                    // shadows off the ground.
                    bias: wgpu::DepthBiasState {
                        constant: 1,
                        slope_scale: 1.0,
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        let feature_renderer = crate::features::FeatureRenderer::new(
            device,
            queue,
            output_format,
            depth_format,
            &camera_bind_group_layout,
            &shadow.caster_bgl,
            &shadow.receiver_bgl,
        );

        // ── Gamma-encode post-process pipeline ──────────────────────────
        // Samples `output_texture` (perceptual) and writes pow(c, 2.2) to
        // `display_texture`. See shaders/gamma_encode.wgsl for the full
        // chain rationale.
        let gamma_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gamma_encode_shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../shaders/gamma_encode.wgsl").into(),
            ),
        });
        let gamma_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gamma_encode_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        // Seed the uniform with a sensible starting exponent so the very
        // first frame after resize -- before any UI tick has called
        // `set_gamma_exponent` -- already runs with a non-zero pow().
        // 1.5 is the current empirical sweet-spot pick; the slider in
        // the viewport debug overlay can dial it from there.
        let gamma_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gamma_encode_uniform"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &gamma_uniform_buffer,
            0,
            bytemuck::bytes_of(&[1.5_f32, 0.0, 0.0, 0.0]),
        );
        let gamma_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gamma_encode_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let gamma_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("gamma_encode_layout"),
                bind_group_layouts: &[&gamma_bgl],
                push_constant_ranges: &[],
            });
        let gamma_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gamma_encode_pipeline"),
            layout: Some(&gamma_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &gamma_shader,
                entry_point: Some("vs_gamma"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &gamma_shader,
                entry_point: Some("fs_gamma"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            render_pipeline,
            sky_pipeline,
            shadow_terrain_pipeline,
            shadow,
            shadow_caster_heightmap_bg,
            shadow_caster_heightmap_bgl,
            camera_buffer,
            camera_bind_group,
            camera_bind_group_layout,
            skybox_sampler,
            skybox_view: skybox_default_view,
            skybox_texture: None,
            skybox_enabled: false,
            feature_renderer: Some(feature_renderer),
            texture_bind_group_layout,
            texture_bind_group,
            albedo_texture,
            albedo_sampler,
            metalmap_texture,
            typemap_texture,
            detail_texture: detail_default,
            detail_sampler,
            splat_detail_normal_1: splat_dn_default_1,
            splat_detail_normal_2: splat_dn_default_2,
            splat_detail_normal_3: splat_dn_default_3,
            splat_detail_normal_4: splat_dn_default_4,
            splat_distr_texture: splat_distr_default,
            advanced_splat_enabled: false,
            sky_reflect_mod_texture: sky_reflect_mod_default,
            sky_reflect_mod_enabled: false,
            specular_tex_texture: specular_tex_default,
            specular_tex_enabled: false,
            grass_shading_tex_texture: grass_shading_tex_default,
            light_emission_tex_texture: light_emission_tex_default,
            light_emission_tex_enabled: false,
            grass_shading_tex_enabled: false,
            has_albedo: false,
            water_planes_bind_group_layout,
            reflection_sampler,
            refraction_sampler,
            refraction_depth_sampler,
            water_planes_bind_group,
            water_planes_bind_group_dummy,
            water_params_buffer,
            last_water_params: WaterParamsUniform::default(),
            water_normal_texture,
            water_normal_sampler,
            water_normal_view,
            heightmap_bind_group_layout,
            heightmap_bind_group,
            heightmap_texture,
            normal_map_texture,
            vertex_buffer: None,
            index_buffer: None,
            num_indices: 0,
            water_index_offset: 0,
            grid_n: 512,
            gamma_pipeline,
            gamma_bgl,
            gamma_sampler,
            gamma_bind_group: None,
            gamma_uniform_buffer,
            display_texture: None,
            display_view: None,
            output_format,
            depth_texture: None,
            depth_format,
            output_texture: None,
            output_view: None,
            reflection_texture: None,
            reflection_view: None,
            reflection_depth_view: None,
            refraction_texture: None,
            refraction_view: None,
            refraction_depth_view: None,
            width: 512,
            height: 512,
            height_scale: 0.3,
            height_range_elmos: 1.0,
            elmo_per_render_xz: [1.0, 1.0],
            water_y: -1.0,
            water_color: [0.2, 0.4, 0.7],
            smf_lighting: SmfLighting::default(),
            brush_cursor: None,
            x_extent: 0.5,
            z_extent: 0.5,
            time: 0.0,
            quality_high: true,
        }
    }

    // ── Public update methods ───────────────────────────────────────────────

    /// Full heightmap replacement. Rebuilds the terrain mesh (flat grid +
    /// skirts + water plane) and uploads the heightmap texture. Called on
    /// graph re-eval or project switch. Recreates the heightmap GPU texture
    /// if dimensions changed.
    pub fn update_heightmap(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        hm: &Heightmap,
        params: TerrainUpdateParams,
    ) {
        let TerrainUpdateParams {
            height_scale,
            x_extent,
            z_extent,
            water_y,
            water_color,
            grid_n,
            height_range_elmos,
            elmo_per_render_xz,
            include_edge_extension,
        } = params;
        self.height_scale = height_scale;
        self.x_extent = x_extent;
        self.z_extent = z_extent;
        self.water_y = water_y;
        self.water_color = water_color;
        self.grid_n = grid_n;
        self.height_range_elmos = height_range_elmos;
        self.elmo_per_render_xz = elmo_per_render_xz;

        // Build mesh: flat grid + edge skirts/cap + optional water plane.
        let (mut verts, mut idxs) = generate_flat_grid(grid_n);

        let skirt_base = verts.len() as u32;
        let (skirt_v, skirt_i) =
            generate_terrain_skirts_and_cap(hm, height_scale, x_extent, z_extent, grid_n);
        idxs.extend(skirt_i.iter().map(|i| i + skirt_base));
        verts.extend(skirt_v);

        // Optional mirrored map-edge extension -- Preview-only. Built
        // from the same dimensions; vertex shader samples the
        // heightmap at each vertex's encoded playable UV for Y.
        if include_edge_extension {
            let ext_base = verts.len() as u32;
            // Engine reference: BAR's `map_edge_extension2` widget
            // tessellates the extension at `gridSize = 32` elmos per
            // cell (`luaui/Widgets/map_edge_extension2.lua:32`), and
            // BAR's playable mesh has `SQUARE_SIZE = 8` elmos per cell
            // (`rts/Sim/Misc/GlobalConstants.h:24`). So the engine's
            // extension:playable density ratio is 1:4.
            //
            // BME's playable mesh is finer than BAR engine: `grid_n`
            // tracks the heightmap's native sample count (up to 2048),
            // so on a 4096-elmo map at native resolution the playable
            // cell is 2 elmos. Tessellating the extension at the
            // engine's literal 32-elmo cell preserves engine fidelity
            // in absolute terms but produces a visible 16:1 stepping
            // at the playable boundary relative to BME's finer
            // surface -- particularly on rough / mountainous terrain.
            //
            // Preserve the engine's 4:1 ratio against BME's actual
            // playable density instead. Floor at 8 elmos (engine
            // playable rate) -- going finer than that tessellates
            // beyond what the heightmap can resolve.
            let world_x_elmos = 2.0 * x_extent * elmo_per_render_xz[0];
            let world_z_elmos = 2.0 * z_extent * elmo_per_render_xz[1];
            let world_max_elmos = world_x_elmos.max(world_z_elmos);
            let playable_cell_elmos = world_max_elmos / grid_n.max(1) as f32;
            let ext_cell_elmos = (playable_cell_elmos * 4.0).max(8.0);
            let cells_x = (world_x_elmos / ext_cell_elmos).round().max(2.0) as u32;
            let cells_z = (world_z_elmos / ext_cell_elmos).round().max(2.0) as u32;
            // Cap at 1025 vertices per axis = ~1M verts per quadrant,
            // ~8M total for the eight-quadrant extension. Prevents
            // pathologically dense meshes on tiny playable-cell sizes.
            let ext_n = (cells_x.max(cells_z) + 1).min(1025);
            let (ext_v, ext_i) = generate_map_edge_extension(x_extent, z_extent, ext_n);
            idxs.extend(ext_i.iter().map(|i| i + ext_base));
            verts.extend(ext_v);
        }

        let water_base = verts.len() as u32;
        // Record where the water sub-range starts BEFORE pushing water indices
        // so the main pass can draw `[0, water_index_offset)` for ground and
        // `[water_index_offset, num_indices)` for water as separate calls
        // with the feature pass between.
        self.water_index_offset = idxs.len() as u32;
        // Water plane extends out to match the map-edge extension's
        // [-3x, +3x] footprint when the extension is enabled --
        // matches engine BumpWater rendering, where water at sea level
        // covers the entire visible scene and obscures lower mirrored
        // terrain plus reflects the upper skybox. Without this, the
        // extension's lower regions show through to bare sky / fog
        // and look unmoored relative to in-game appearance.
        let water_span = if include_edge_extension { 5.0 } else { 1.0 };
        let (water_v, water_i) = generate_water_plane(x_extent, z_extent, water_y, water_span);
        idxs.extend(water_i.iter().map(|i| i + water_base));
        verts.extend(water_v);

        self.num_indices = idxs.len() as u32;
        self.vertex_buffer = Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("terrain_vertices"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        );
        self.index_buffer = Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("terrain_indices"),
                contents: bytemuck::cast_slice(&idxs),
                usage: wgpu::BufferUsages::INDEX,
            }),
        );

        // Upload heightmap texture.
        let hm_w = hm.width();
        let hm_h = hm.height();
        let data: Vec<f32> = (0..hm_h)
            .flat_map(|y| (0..hm_w).map(move |x| hm.get(x, y).unwrap_or(0.0)))
            .collect();

        // Pre-bake the per-fragment surface normal map. Depends on the
        // heightmap content AND on height_scale + extents, so it has
        // to be regenerated on every `update_heightmap` call (not just
        // when dims change).
        let normal_bytes = build_normal_map_bytes(hm, height_scale, x_extent, z_extent);
        let normal_bytes_u8: &[u8] = bytemuck::cast_slice(&normal_bytes);

        let old_size = (
            self.heightmap_texture.width(),
            self.heightmap_texture.height(),
        );
        if old_size == (hm_w, hm_h) {
            // Same dimensions: write in place.
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.heightmap_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&data),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(hm_w * 4),
                    rows_per_image: Some(hm_h),
                },
                wgpu::Extent3d {
                    width: hm_w,
                    height: hm_h,
                    depth_or_array_layers: 1,
                },
            );
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.normal_map_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                normal_bytes_u8,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(hm_w * 2),
                    rows_per_image: Some(hm_h),
                },
                wgpu::Extent3d {
                    width: hm_w,
                    height: hm_h,
                    depth_or_array_layers: 1,
                },
            );
        } else {
            // Dimensions changed: recreate both textures and the bind group.
            let tex = device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some("heightmap_tex"),
                    size: wgpu::Extent3d {
                        width: hm_w,
                        height: hm_h,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R32Float,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                bytemuck::cast_slice(&data),
            );
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            let nm_tex = device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some("normal_map_tex"),
                    size: wgpu::Extent3d {
                        width: hm_w,
                        height: hm_h,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rg8Snorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                normal_bytes_u8,
            );
            let nm_view = nm_tex.create_view(&wgpu::TextureViewDescriptor::default());
            self.heightmap_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("group3_bind_group"),
                layout: &self.heightmap_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&self.water_normal_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.water_normal_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&nm_view),
                    },
                ],
            });
            // Shadow caster needs its own bind group pointing at the same view.
            self.shadow_caster_heightmap_bg =
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("shadow_caster_heightmap_bg"),
                    layout: &self.shadow_caster_heightmap_bgl,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    }],
                });
            self.heightmap_texture = tex;
            self.normal_map_texture = nm_tex;
        }
    }

    /// Sub-region heightmap upload. Called per brush dab to update only the
    /// dirty rectangle without rebuilding the mesh. `data` is row-major f32
    /// of length `w * h`.
    pub fn update_heightmap_region(
        &self,
        queue: &wgpu::Queue,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        data: &[f32],
    ) {
        if w == 0 || h == 0 {
            return;
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.heightmap_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Replace the skybox cubemap. `faces` is the 6-face array decoded by
    /// `bar_data::skybox::load_dds_cubemap`. All faces must be the same
    /// `width x height` and in `Rgba8Unorm` row-major layout. Setting a
    /// skybox flips `skybox_enabled` on so the sky shader samples the
    /// cubemap instead of the procedural ModernSky path.
    pub fn update_skybox(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cubemap: &bar_data::Cubemap,
    ) {
        // wgpu expects the 6 faces concatenated as `LayerMajor` data --
        // face 0 first, then face 1, etc. `bar_data::Cubemap` already
        // stores them in that order.
        let mut packed: Vec<u8> =
            Vec::with_capacity((cubemap.width * cubemap.height * 4 * 6) as usize);
        for face in &cubemap.faces {
            packed.extend_from_slice(face);
        }
        let tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("skybox_cubemap"),
                size: wgpu::Extent3d {
                    width: cubemap.width,
                    height: cubemap.height,
                    depth_or_array_layers: 6,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &packed,
        );
        let view = tex.create_view(&wgpu::TextureViewDescriptor {
            label: Some("skybox_cubemap_view"),
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });
        self.skybox_view = view;
        self.skybox_texture = Some(tex);
        self.skybox_enabled = true;
        self.rebuild_camera_bind_group(device);
    }

    /// Clear any uploaded skybox; the sky shader falls back to the
    /// procedural ModernSky path on the next frame.
    pub fn clear_skybox(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let zero: [u8; 4 * 6] = [0; 4 * 6];
        let tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("skybox_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 6,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &zero,
        );
        let view = tex.create_view(&wgpu::TextureViewDescriptor {
            label: Some("skybox_default_view"),
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });
        self.skybox_view = view;
        self.skybox_texture = None;
        self.skybox_enabled = false;
        self.rebuild_camera_bind_group(device);
    }

    fn rebuild_camera_bind_group(&mut self, device: &wgpu::Device) {
        self.camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera_bind_group"),
            layout: &self.camera_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.skybox_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.skybox_sampler),
                },
            ],
        });
    }

    /// Replace the albedo texture from a `ColorBuffer`. Sets the `has_albedo`
    /// flag so the shader takes the texture path instead of procedural colour.
    pub fn update_albedo(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, cb: &ColorBuffer) {
        let rgba = cb.to_rgba8();
        let tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("albedo_texture"),
                size: wgpu::Extent3d {
                    width: cb.width(),
                    height: cb.height(),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &rgba,
        );
        self.albedo_texture = tex;
        self.has_albedo = true;
        self.rebuild_material_bind_group(device);
    }

    /// Sub-region albedo upload. `data` is row-major RGBA8 of length `w * h * 4`.
    pub fn update_albedo_region(
        &self,
        queue: &wgpu::Queue,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        data: &[u8],
    ) {
        if w == 0 || h == 0 {
            return;
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.albedo_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Reset albedo to the 1x1 white default. Called when an eval produces
    /// no texture so the shader falls back to procedural height colour.
    pub fn clear_albedo(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let white: [u8; 4] = [255, 255, 255, 255];
        let tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("albedo_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &white,
        );
        self.albedo_texture = tex;
        self.has_albedo = false;
        self.rebuild_material_bind_group(device);
    }

    /// Upload a pre-assembled flat linear BC1/DXT1 image as the albedo texture.
    ///
    /// `bc1_data` must be in row-major BC1 block order covering `width x height`
    /// pixels (both must be multiples of 4). The device must have been created
    /// with `TEXTURE_COMPRESSION_BC` enabled -- check `GpuContext::supports_bc`
    /// before calling.
    pub fn upload_bc1_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bc1_data: &[u8],
        width: u32,
        height: u32,
    ) {
        let tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("albedo_bc1"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bc1RgbaUnorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bc1_data,
        );
        self.albedo_texture = tex;
        self.has_albedo = true;
        self.rebuild_material_bind_group(device);
    }

    /// Replace the metalmap texture from a `Heightmap` (values 0..1).
    pub fn update_metalmap(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, hm: &Heightmap) {
        let data: Vec<u8> = (0..hm.height())
            .flat_map(|y| {
                (0..hm.width()).map(move |x| {
                    (hm.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8
                })
            })
            .collect();
        self.metalmap_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("metalmap_tex"),
                size: wgpu::Extent3d {
                    width: hm.width(),
                    height: hm.height(),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &data,
        );
        self.rebuild_material_bind_group(device);
    }

    /// Sub-region metalmap upload. `data` is row-major f32 of length `w * h`.
    pub fn update_metalmap_region(
        &self,
        queue: &wgpu::Queue,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        data: &[f32],
    ) {
        if w == 0 || h == 0 {
            return;
        }
        let bytes: Vec<u8> = data
            .iter()
            .map(|&v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.metalmap_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Replace the typemap texture from a `Heightmap` (values 0..1).
    pub fn update_typemap(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, hm: &Heightmap) {
        let data: Vec<u8> = (0..hm.height())
            .flat_map(|y| {
                (0..hm.width()).map(move |x| {
                    (hm.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8
                })
            })
            .collect();
        self.typemap_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("typemap_tex"),
                size: wgpu::Extent3d {
                    width: hm.width(),
                    height: hm.height(),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &data,
        );
        self.rebuild_material_bind_group(device);
    }

    /// Sub-region typemap upload. `data` is row-major f32 of length `w * h`.
    pub fn update_typemap_region(
        &self,
        queue: &wgpu::Queue,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        data: &[f32],
    ) {
        if w == 0 || h == 0 {
            return;
        }
        let bytes: Vec<u8> = data
            .iter()
            .map(|&v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.typemap_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Rebuild the group-1 bind group after any of the three material textures change.
    fn rebuild_material_bind_group(&mut self, device: &wgpu::Device) {
        let av = self
            .albedo_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mv = self
            .metalmap_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let tv = self
            .typemap_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let dv = self
            .detail_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let sd1 = self
            .splat_detail_normal_1
            .create_view(&wgpu::TextureViewDescriptor::default());
        let sd2 = self
            .splat_detail_normal_2
            .create_view(&wgpu::TextureViewDescriptor::default());
        let sd3 = self
            .splat_detail_normal_3
            .create_view(&wgpu::TextureViewDescriptor::default());
        let sd4 = self
            .splat_detail_normal_4
            .create_view(&wgpu::TextureViewDescriptor::default());
        let sdv = self
            .splat_distr_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let srmv = self
            .sky_reflect_mod_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let specv = self
            .specular_tex_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let gstv = self
            .grass_shading_tex_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let letv = self
            .light_emission_tex_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture_bind_group"),
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&av),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.albedo_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&mv),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&tv),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.albedo_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&dv),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&self.detail_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&sd1),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(&sd2),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::TextureView(&sd3),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: wgpu::BindingResource::TextureView(&sd4),
                },
                wgpu::BindGroupEntry {
                    binding: 11,
                    resource: wgpu::BindingResource::TextureView(&sdv),
                },
                wgpu::BindGroupEntry {
                    binding: 12,
                    resource: wgpu::BindingResource::TextureView(&srmv),
                },
                wgpu::BindGroupEntry {
                    binding: 13,
                    resource: wgpu::BindingResource::TextureView(&specv),
                },
                wgpu::BindGroupEntry {
                    binding: 14,
                    resource: wgpu::BindingResource::TextureView(&gstv),
                },
                wgpu::BindGroupEntry {
                    binding: 15,
                    resource: wgpu::BindingResource::TextureView(&letv),
                },
            ],
        });
    }

    /// Replace the sky-reflection mod 2D texture. Flips
    /// `sky_reflect_mod_enabled` on so the next frame's shader applies
    /// the cubemap reflection on the terrain.
    pub fn update_sky_reflect_mod(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) {
        self.sky_reflect_mod_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("sky_reflect_mod"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SKY_REFLECT_MOD_TEX_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            rgba,
        );
        self.sky_reflect_mod_enabled = true;
        self.rebuild_material_bind_group(device);
    }

    /// Reset the sky-reflection mod to the 1x1 black default so the
    /// shader's mix factor goes to zero everywhere.
    pub fn clear_sky_reflect_mod(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let zero: [u8; 4] = [0, 0, 0, 255];
        self.sky_reflect_mod_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("sky_reflect_mod_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SKY_REFLECT_MOD_TEX_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &zero,
        );
        self.sky_reflect_mod_enabled = false;
        self.rebuild_material_bind_group(device);
    }

    /// Replace the per-pixel specular texture (mapinfo `specularTex`).
    /// Engine `SMF_SPECULAR_LIGHTING` path. Flips `specular_tex_enabled`
    /// on so the next frame's uniform tells the shader to sample
    /// per-pixel `specCol.rgb` + `specCol.a * 16` for exponent instead
    /// of using the global `groundSpecularColor`.
    pub fn update_specular_tex(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) {
        self.specular_tex_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("specular_tex"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SPECULAR_TEX_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            rgba,
        );
        self.specular_tex_enabled = true;
        self.rebuild_material_bind_group(device);
    }

    /// Reset the specular texture to the 1x1 black default so the shader's
    /// `SMF_SPECULAR_LIGHTING` gate goes off and spec falls back to the
    /// global `ground_specular` uniform.
    pub fn clear_specular_tex(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let zero: [u8; 4] = [0, 0, 0, 255];
        self.specular_tex_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("specular_tex_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SPECULAR_TEX_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &zero,
        );
        self.specular_tex_enabled = false;
        self.rebuild_material_bind_group(device);
    }

    /// Replace the grassShadingTex 2D texture. Sets
    /// `grass_shading_tex_enabled` on so the next frame's extension
    /// shader path samples this texture (BAR's `map_edge_extension2`
    /// widget's `$grass` lookup) instead of falling back to the
    /// playable albedo.
    pub fn update_grass_shading_tex(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) {
        // Generate a full mip chain CPU-side and upload every level. The
        // map-edge extension samples at oblique angles and across wide
        // spatial ranges (each mirror quadrant covers the same
        // playable-UV [0, 1] as the playable area, but at the screen-
        // space frequency the geometry demands), so the GPU needs
        // filtered downsamples to avoid sparkle / aliasing that reads
        // as "low quality". Without mips the sampler is stuck at the
        // base level no matter the derivative.
        let chain = ensure_full_mip_chain(vec![(rgba.to_vec(), width, height)]);
        let mip_count = chain.len() as u32;
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("grass_shading_tex"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: mip_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, (rgba, w, h)) in chain.into_iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.grass_shading_tex_texture = tex;
        self.grass_shading_tex_enabled = true;
        self.rebuild_material_bind_group(device);
    }

    /// Reset grassShadingTex to the 1x1 grey default; extension shader
    /// path will fall back to sampling the playable albedo.
    pub fn clear_grass_shading_tex(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let grey: [u8; 4] = [127, 127, 127, 255];
        self.grass_shading_tex_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("grass_shading_tex_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &grey,
        );
        self.grass_shading_tex_enabled = false;
        self.rebuild_material_bind_group(device);
    }

    /// Replace the `lightEmissionTex` 2D texture. Sets
    /// `light_emission_tex_enabled` on so the terrain shader's apply-
    /// emission stage actually runs. Engine path `SMF_LIGHT_EMISSION`
    /// (`bar-recoil/rts/Map/SMF/SMFFragProg.glsl:392-401`); unshadowed
    /// glow, alpha-gated additive blend.
    pub fn update_light_emission_tex(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) {
        self.light_emission_tex_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("light_emission_tex"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            rgba,
        );
        self.light_emission_tex_enabled = true;
        self.rebuild_material_bind_group(device);
    }

    /// Reset `lightEmissionTex` to the 1x1 `(0,0,0,0)` default so the
    /// emission blend collapses to identity (no glow contribution).
    pub fn clear_light_emission_tex(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let zero: [u8; 4] = [0, 0, 0, 0];
        self.light_emission_tex_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("light_emission_tex_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &zero,
        );
        self.light_emission_tex_enabled = false;
        self.rebuild_material_bind_group(device);
    }

    /// Replace all five splat-detail textures at once. Each `faces`-style
    /// entry is `(rgba_bytes, width, height)`. Order:
    /// `[splatDetailNormalTex1..4, splatDistrTex]`. Sets
    /// `advanced_splat_enabled = true` so the next frame's uniform
    /// flips the shader path on.
    /// Upload the four splat-detail-normal textures + the distribution
    /// texture. Each entry is a mip chain `(rgba, w, h)` with `mips[0]`
    /// being the base level. If a chain has only the base mip we
    /// synthesise the rest CPU-side via box filtering -- approximates
    /// `glGenerateMipmap`, sufficient to defeat aliasing on the splat
    /// detail-normal sample even when the source format had no mip data
    /// (PNG/TGA). DDS sources pass through their hand-tuned chain.
    pub fn update_splat_textures(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        textures: [Vec<(Vec<u8>, u32, u32)>; 5],
    ) {
        let make = |label: &str, mip_chain: Vec<(Vec<u8>, u32, u32)>| -> wgpu::Texture {
            let chain = ensure_full_mip_chain(mip_chain);
            let (_, base_w, base_h) = chain[0];
            let mip_count = chain.len() as u32;
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: base_w,
                    height: base_h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: mip_count,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // Both splat detail-normal and splat distribution feed
                // through this closure; their formats happen to be
                // identical (linear data, see module-level constants).
                format: SPLAT_DETAIL_NORMAL_TEX_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            for (level, (rgba, w, h)) in chain.into_iter().enumerate() {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &tex,
                        mip_level: level as u32,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &rgba,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(w * 4),
                        rows_per_image: Some(h),
                    },
                    wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                );
            }
            tex
        };

        // Unfortunately I can't deconstruct the array directly because the
        // closure captures `chain`. Hold the entries in a Vec long enough
        // to hand each to `make` in turn.
        let mut iter = textures.into_iter();
        self.splat_detail_normal_1 = make("splat_detail_normal_1", iter.next().unwrap());
        self.splat_detail_normal_2 = make("splat_detail_normal_2", iter.next().unwrap());
        self.splat_detail_normal_3 = make("splat_detail_normal_3", iter.next().unwrap());
        self.splat_detail_normal_4 = make("splat_detail_normal_4", iter.next().unwrap());
        self.splat_distr_texture = make("splat_distr", iter.next().unwrap());
        self.advanced_splat_enabled = true;
        self.rebuild_material_bind_group(device);
    }

    /// Drop any uploaded splat textures and revert to the 1x1
    /// defaults so the shader's advanced-splat path stays off.
    pub fn clear_splat_textures(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let default_data: [u8; 4] = [127, 127, 127, 127];
        let make = |label: &str| -> wgpu::Texture {
            device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: SPLAT_DETAIL_NORMAL_TEX_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &default_data,
            )
        };
        self.splat_detail_normal_1 = make("splat_dn_default_1");
        self.splat_detail_normal_2 = make("splat_dn_default_2");
        self.splat_detail_normal_3 = make("splat_dn_default_3");
        self.splat_detail_normal_4 = make("splat_dn_default_4");
        self.splat_distr_texture = make("splat_distr_default");
        self.advanced_splat_enabled = false;
        self.rebuild_material_bind_group(device);
    }

    /// Replace the detail texture (mapinfo `resources.detailTex`).
    /// Takes raw RGBA bytes + dimensions; the loader in bar-app decodes
    /// whatever format the map ships (BMP/PNG/TGA via `image`, DDS via
    /// `ddsfile`) and passes us the canonical RGBA8 layout.
    pub fn update_detail_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) {
        let tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("detail_texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            rgba,
        );
        self.detail_texture = tex;
        self.rebuild_material_bind_group(device);
    }

    // ── Camera and animation ────────────────────────────────────────────────

    fn set_time(&mut self, seconds: f32) {
        self.time = seconds % (std::f32::consts::TAU * 60.0);
    }

    fn set_quality_high(&mut self, enabled: bool) {
        self.quality_high = enabled;
    }

    pub fn set_brush_cursor(&mut self, cursor: Option<(f32, f32, f32)>) {
        self.brush_cursor = cursor;
    }

    /// Live-tune the gamma post-pass exponent. See
    /// `shaders/gamma_encode.wgsl` for the rationale; the viewport
    /// debug overlay surfaces this as a slider so the right value can
    /// be dialled visually against an in-engine reference.
    pub fn set_gamma_exponent(&self, queue: &wgpu::Queue, exponent: f32) {
        let padded = [exponent, 0.0, 0.0, 0.0];
        queue.write_buffer(&self.gamma_uniform_buffer, 0, bytemuck::bytes_of(&padded));
    }

    fn brush_cursor_uniform(&self) -> [f32; 4] {
        match self.brush_cursor {
            Some((x, z, r)) => [x, z, r, 1.0],
            None => [0.0, 0.0, 0.0, 0.0],
        }
    }

    // ── Resize / render ─────────────────────────────────────────────────────

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain_output"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let output_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Gamma-encoded copy of the perceptual render target. egui samples
        // this view, not the raw output; sRGB swapchain re-encoding then
        // cancels back to V^2.2 on display (matches BAR's in-game
        // appearance). Same dimensions, same format -- only the contents
        // are pre-darkened by the gamma post-pass.
        let display = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain_display"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.output_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let display_view = display.create_view(&wgpu::TextureViewDescriptor::default());
        let gamma_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gamma_encode_bg"),
            layout: &self.gamma_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&output_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.gamma_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.gamma_uniform_buffer.as_entire_binding(),
                },
            ],
        });

        self.output_view = Some(output_view);
        self.output_texture = Some(texture);
        self.display_view = Some(display_view);
        self.display_texture = Some(display);
        self.gamma_bind_group = Some(gamma_bg);

        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain_depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.depth_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.depth_texture =
            Some(depth_texture.create_view(&wgpu::TextureViewDescriptor::default()));

        let reflection_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("reflection_color"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let reflection_view = reflection_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let reflection_depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("reflection_depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.depth_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.reflection_depth_view =
            Some(reflection_depth.create_view(&wgpu::TextureViewDescriptor::default()));

        let refraction_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("refraction_color"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let refraction_view = refraction_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let refraction_depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("refraction_depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.depth_format,
            // The water shader samples this depth in the main pass to
            // do the engine's depth-aware refraction mixback
            // (`BumpWaterFS:304-314`): if the distorted refraction UV
            // pulls in a fragment that's *closer* than the water plane
            // (i.e. above-water terrain leaking through near a
            // shoreline), the shader replaces it with the undistorted
            // sample. That requires TEXTURE_BINDING on top of the
            // RENDER_ATTACHMENT we always needed.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.refraction_depth_view =
            Some(refraction_depth.create_view(&wgpu::TextureViewDescriptor::default()));

        // We need a view of the refraction-depth texture that's
        // separate from the one used for the depth attachment, so
        // capture it here before storing the attachment view on
        // self.refraction_depth_view.
        let refraction_depth_sample_view =
            refraction_depth.create_view(&wgpu::TextureViewDescriptor::default());

        self.water_planes_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water_planes_bind_group"),
            layout: &self.water_planes_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&reflection_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.reflection_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&refraction_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.refraction_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.water_params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&refraction_depth_sample_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&self.refraction_depth_sampler),
                },
            ],
        });
        self.reflection_view = Some(reflection_view);
        self.reflection_texture = Some(reflection_tex);
        self.refraction_view = Some(refraction_view);
        self.refraction_texture = Some(refraction_tex);
    }

    /// Render one frame. `None` clears the viewport; `Some(frame)` renders the
    /// scene. Heightmap and texture data flow through the `update_*` methods;
    /// `PreviewFrame` carries only per-frame uniform inputs (water, time, quality).
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        frame: Option<&PreviewFrame>,
    ) {
        match frame {
            None => {
                self.clear_mesh();
                self.render_empty(device, queue);
            }
            Some(f) => {
                self.sync_to_frame(f);
                self.render_internal(device, queue, camera);
            }
        }
    }

    /// Apply per-frame params from a `PreviewFrame`. No geometry or texture uploads.
    fn sync_to_frame(&mut self, f: &PreviewFrame) {
        self.height_scale = f.height_scale;
        self.height_range_elmos = f.height_range_elmos;
        self.elmo_per_render_xz = f.elmo_per_render_xz;
        self.water_y = f.water_y;
        self.water_color = f.water_color;
        // The skybox + splat upload state is owned by the renderer
        // (set by `update_skybox` / `update_splat_textures`), not by
        // the per-frame uniform. Echo them into `smf_lighting` before
        // storing so the per-frame uniform sees the correct enabled
        // flags regardless of what the caller set in `f.smf_lighting`.
        // Same for `elmo_per_render_xz` -- comes from `update_heightmap`
        // (host computes it from map dimensions).
        self.smf_lighting = bar_render_smf_with_runtime_overrides(
            f.smf_lighting,
            self.skybox_enabled,
            self.advanced_splat_enabled,
            self.sky_reflect_mod_enabled,
            self.specular_tex_enabled,
            self.grass_shading_tex_enabled,
            self.light_emission_tex_enabled,
            self.elmo_per_render_xz,
        );
        self.x_extent = f.x_extent;
        self.z_extent = f.z_extent;
        self.set_quality_high(f.quality_high);
        self.set_time(f.time);
    }

    fn render_empty(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let (Some(output_view), Some(depth_view)) =
            (self.output_view.as_ref(), self.depth_texture.as_ref())
        else {
            return;
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("clear_output_encoder"),
        });
        {
            let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_output_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.07,
                            g: 0.07,
                            b: 0.09,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        queue.submit(std::iter::once(encoder.finish()));
    }

    fn render_internal(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, camera: &Camera) {
        let Some(ref output_view) = self.output_view else {
            return;
        };
        let Some(ref depth_view) = self.depth_texture else {
            return;
        };
        let Some(ref vertex_buffer) = self.vertex_buffer else {
            return;
        };
        let Some(ref index_buffer) = self.index_buffer else {
            return;
        };

        let aspect = self.width as f32 / self.height.max(1) as f32;
        let view_proj = camera.view_projection(aspect);
        let cam_pos = camera.position();

        let smf = self.smf_lighting.to_uniform_slots();

        // Upload per-map BumpWater surface params. Cheap: 80-byte uniform.
        // Log only on change so moving Water-tab sliders surfaces a clean
        // signal in the BME log without spamming every frame.
        let water_params = WaterParamsUniform::from(&self.smf_lighting);
        if water_params != self.last_water_params {
            // DEBUG-level so this doesn't appear in the BME log by default;
            // toggle the panel's DBG button to see it. Fires on change only,
            // so dragging a Water-tab slider produces one row per stable
            // value rather than one per frame.
            tracing::debug!(
                fresnel_min = water_params.fresnel[0],
                fresnel_max = water_params.fresnel[1],
                fresnel_power = water_params.fresnel[2],
                surface_alpha = water_params.surface_color_alpha[3],
                specular_factor = water_params.factors[1],
                perlin_amplitude = water_params.factors[3],
                "water_params changed"
            );
            self.last_water_params = water_params;
        }
        queue.write_buffer(
            &self.water_params_buffer,
            0,
            bytemuck::bytes_of(&water_params),
        );

        // Refresh the shadow uniform from the current sun + scene bounds.
        // Cheap (a single 80-byte write_buffer); keeps the shadow frustum in
        // sync with map resizes and sun direction edits without any caching.
        self.shadow.update_light(
            queue,
            self.smf_lighting.sun_dir,
            self.x_extent,
            self.z_extent,
            self.height_scale,
        );

        let base_uniform = CameraUniform {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            camera_pos: [cam_pos.x, cam_pos.y, cam_pos.z],
            has_texture: self.has_albedo as u32,
            height_scale: self.height_scale,
            water_r: self.water_color[0],
            water_g: self.water_color[1],
            water_b: self.water_color[2],
            water_y: self.water_y,
            time: self.time,
            skip_water: 0.0,
            height_range_elmos: self.height_range_elmos,
            screen_w: self.width as f32,
            screen_h: self.height as f32,
            x_extent: self.x_extent,
            z_extent: self.z_extent,
            sun_dir_exp: smf.sun_dir_exp,
            ground_ambient: smf.ground_ambient,
            ground_diffuse: smf.ground_diffuse,
            ground_specular: smf.ground_specular,
            water_absorb: smf.water_absorb,
            water_base_color: smf.water_base_color,
            water_min_color: smf.water_min_color,
            brush_cursor: [0.0, 0.0, 0.0, 0.0],
            clip_plane: NO_CLIP,
            custom_fog_color_atten: smf.custom_fog_color_atten,
            custom_fog_params: smf.custom_fog_params,
            sun_color: smf.sun_color,
            sky_color_density: smf.sky_color_density,
            sky_dir: smf.sky_dir,
            cloud_color: smf.cloud_color,
            skybox_params: smf.skybox_params,
            splat_tex_scales: smf.splat_tex_scales,
            splat_tex_mults: smf.splat_tex_mults,
            splat_params: smf.splat_params,
            // Engine matches BAR's `UniformConstants` build: multiply
            // the mapinfo fractions by the camera far-plane host-side.
            fog_dists: [
                smf.atmosphere_fog[0] * camera.far,
                smf.atmosphere_fog[1] * camera.far,
                0.0,
                0.0,
            ],
            fog_color: smf.atmosphere_fog_color,
        };

        // ── Pass 0: shadow map ──────────────────────────────────────────────
        // Render terrain + features from the sun's POV into a single depth
        // texture. The receiver bind group built by `ShadowMap` is then bound
        // by every subsequent pass that uses the terrain or feature pipeline.
        // Write the main-pass camera uniform before the shadow pass; the
        // shadow_terrain shader only reads x_extent / z_extent / height_scale
        // and the buffer contents are stable across this whole encoder.
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&base_uniform));
        {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("shadow_pass_encoder"),
            });
            {
                let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shadow_pass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: self.shadow.depth_view(),
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                // Terrain caster.
                rp.set_pipeline(&self.shadow_terrain_pipeline);
                rp.set_bind_group(0, &self.camera_bind_group, &[]);
                rp.set_bind_group(1, self.shadow.caster_bind_group(), &[]);
                rp.set_bind_group(2, &self.shadow_caster_heightmap_bg, &[]);
                rp.set_vertex_buffer(0, vertex_buffer.slice(..));
                rp.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..self.num_indices, 0, 0..1);

                // Feature caster -- skipped if there are no features.
                if let Some(ref fr) = self.feature_renderer {
                    fr.draw_shadow(&mut rp, self.shadow.caster_bind_group());
                }
            }
            queue.submit(std::iter::once(enc.finish()));
        }

        // ── Pass 1: planar reflection ───────────────────────────────────────
        // Renders the world from a camera mirrored about y = water_y, keeping
        // only the half-space on the SAME side of the water as the camera.
        // Above-water cameras get a mirror image of above-water geometry
        // (standard planar reflection); below-water cameras get a mirror
        // image of the underwater scene (total internal reflection content).
        if self.water_y >= 0.0 {
            if let (Some(ref reflection_view), Some(ref reflection_depth_view)) =
                (&self.reflection_view, &self.reflection_depth_view)
            {
                let wy = self.water_y;
                let above_water = cam_pos.y >= wy;
                // Build the reflection matrix R about y = wy: world point
                // (x, y, z) maps to (x, 2*wy - y, z). As a 4x4 matrix this is
                // ScaleY(-1) followed by TranslateY(2*wy). Det(R) = -1, so R
                // is not a rigid transform and V*R cannot be re-expressed as
                // a look_at; multiply V*R directly.
                let reflect_y = Mat4::from_cols_array_2d(&[
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, -1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 2.0 * wy, 0.0, 1.0],
                ]);
                // Render the reflected world from the original camera; the
                // resulting framebuffer lines up 1:1 with main-pass screen
                // coordinates, so the water shader samples it at its own
                // screen UV (no Y flip).
                let view_proj_refl = view_proj * reflect_y;
                // camera_pos exposed to the shader is the *mirrored* eye --
                // i.e. the viewpoint after R has been applied to the world.
                let cam_pos_refl = glam::Vec3::new(cam_pos.x, 2.0 * wy - cam_pos.y, cam_pos.z);

                // Keep only the camera's own side of the water plane so the
                // reflected content is what *would* appear in the mirror at
                // each fragment's screen position.
                let clip_plane = if above_water {
                    [0.0, 1.0, 0.0, -wy] // keep world.y >= wy
                } else {
                    [0.0, -1.0, 0.0, wy] // keep world.y <= wy
                };

                let refl_uniform = CameraUniform {
                    view_proj: view_proj_refl.to_cols_array_2d(),
                    inv_view_proj: view_proj_refl.inverse().to_cols_array_2d(),
                    camera_pos: [cam_pos_refl.x, cam_pos_refl.y, cam_pos_refl.z],
                    skip_water: 1.0,
                    clip_plane,
                    brush_cursor: [0.0, 0.0, 0.0, 0.0],
                    ..base_uniform
                };
                queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&refl_uniform));

                let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("reflection_encoder"),
                });
                {
                    let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("reflection_pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: reflection_view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                // Reflection clear colour matches engine
                                // `BumpWater.cpp:1018`: `glClearColor(sky->fogColor)`.
                                // Pixels with no reflected content
                                // (above-water sky regions with no skybox
                                // cubemap, beyond-terrain pixels) fill with
                                // atmospheric fog colour. The prior
                                // hardcoded pale blue was the source of
                                // bright reflections on dark-themed maps.
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: self.smf_lighting.atmosphere_fog_color[0] as f64,
                                    g: self.smf_lighting.atmosphere_fog_color[1] as f64,
                                    b: self.smf_lighting.atmosphere_fog_color[2] as f64,
                                    a: 1.0,
                                }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: reflection_depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    rp.set_pipeline(&self.render_pipeline);
                    rp.set_bind_group(0, &self.camera_bind_group, &[]);
                    rp.set_bind_group(1, &self.texture_bind_group, &[]);
                    rp.set_bind_group(2, &self.water_planes_bind_group_dummy, &[]);
                    rp.set_bind_group(3, &self.heightmap_bind_group, &[]);
                    rp.set_bind_group(4, self.shadow.receiver_bind_group(), &[]);
                    rp.set_vertex_buffer(0, vertex_buffer.slice(..));
                    rp.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..self.num_indices, 0, 0..1);

                    rp.set_pipeline(&self.sky_pipeline);
                    rp.set_bind_group(0, &self.camera_bind_group, &[]);
                    rp.draw(0..3, 0..1);
                }
                queue.submit(std::iter::once(enc.finish()));
            }
        }

        // ── Pass 2: planar refraction ───────────────────────────────────────
        // Renders the world from the original (un-mirrored) camera with a
        // clip plane on the OPPOSITE side of the water from the camera, so:
        //   - camera above water: refraction texture contains below-water
        //     terrain (the lakebed seen through the water surface);
        //   - camera below water: refraction texture contains above-water
        //     terrain + sky (the world above the water, as squeezed into
        //     Snell's window).
        // The water plane itself is excluded via skip_water.
        if self.water_y >= 0.0 {
            if let (Some(ref refraction_view), Some(ref refraction_depth_view)) =
                (&self.refraction_view, &self.refraction_depth_view)
            {
                let wy = self.water_y;
                let above_water = cam_pos.y >= wy;
                let clip_plane = if above_water {
                    // keep world.y <= water_y (below-water half-space)
                    [0.0, -1.0, 0.0, wy]
                } else {
                    // keep world.y >= water_y (above-water half-space)
                    [0.0, 1.0, 0.0, -wy]
                };
                // Engine `BumpWater.cpp:1000-1001` shifts the sun lighting
                // for the refraction pass: diffuse *= (0.5, 0.7, 0.9),
                // ambient *= (0.6, 0.8, 1.0). Tints underwater terrain
                // cooler/bluer to read as "seen through water". Without
                // this the refraction texture has the same lighting as
                // the above-water render, which on a lit-warm map makes
                // the underwater terrain look incongruously warm.
                let mut tinted_diffuse = base_uniform.ground_diffuse;
                tinted_diffuse[0] *= 0.5;
                tinted_diffuse[1] *= 0.7;
                tinted_diffuse[2] *= 0.9;
                let mut tinted_ambient = base_uniform.ground_ambient;
                tinted_ambient[0] *= 0.6;
                tinted_ambient[1] *= 0.8;
                tinted_ambient[2] *= 1.0;
                let refr_uniform = CameraUniform {
                    skip_water: 1.0,
                    clip_plane,
                    brush_cursor: [0.0, 0.0, 0.0, 0.0],
                    ground_diffuse: tinted_diffuse,
                    ground_ambient: tinted_ambient,
                    ..base_uniform
                };
                queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&refr_uniform));

                let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("refraction_encoder"),
                });
                {
                    let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("refraction_pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: refraction_view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                // Refraction clear colour matches engine
                                // `BumpWater.cpp:988`: `glClearColor(sky->fogColor)`.
                                // Pixels with no underwater terrain rendered
                                // (open ocean beyond the playable mesh, the
                                // extension area, etc.) fill with the
                                // atmospheric fog colour from mapinfo.
                                // Previously we used `water_base * 0.5`,
                                // which is a brighter cyan than `fogColor`
                                // on most maps -- and the difference is
                                // dramatic on dark-themed maps where the
                                // engine intentionally chose a near-black
                                // fogColor.
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: self.smf_lighting.atmosphere_fog_color[0] as f64,
                                    g: self.smf_lighting.atmosphere_fog_color[1] as f64,
                                    b: self.smf_lighting.atmosphere_fog_color[2] as f64,
                                    a: 1.0,
                                }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: refraction_depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    rp.set_pipeline(&self.render_pipeline);
                    rp.set_bind_group(0, &self.camera_bind_group, &[]);
                    rp.set_bind_group(1, &self.texture_bind_group, &[]);
                    rp.set_bind_group(2, &self.water_planes_bind_group_dummy, &[]);
                    rp.set_bind_group(3, &self.heightmap_bind_group, &[]);
                    rp.set_bind_group(4, self.shadow.receiver_bind_group(), &[]);
                    rp.set_vertex_buffer(0, vertex_buffer.slice(..));
                    rp.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..self.num_indices, 0, 0..1);

                    rp.set_pipeline(&self.sky_pipeline);
                    rp.set_bind_group(0, &self.camera_bind_group, &[]);
                    rp.draw(0..3, 0..1);
                }
                // Feature pass into the refraction texture: lets the water
                // shader's refraction sample carry underwater features. The
                // refraction camera_uniform has `clip_plane` set to the
                // far-side half-space, and features.wgsl's fragment shader
                // discards fragments that fail it.
                if let Some(ref fr) = self.feature_renderer {
                    fr.draw(
                        &mut enc,
                        refraction_view,
                        refraction_depth_view,
                        &self.camera_bind_group,
                        self.shadow.receiver_bind_group(),
                    );
                }
                queue.submit(std::iter::once(enc.finish()));
            }
        }

        // ── Pass 3: main render ─────────────────────────────────────────────
        let camera_uniform = CameraUniform {
            brush_cursor: self.brush_cursor_uniform(),
            ..base_uniform
        };
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("terrain_render_encoder"),
        });
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("terrain_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.15,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            render_pass.set_bind_group(1, &self.texture_bind_group, &[]);
            render_pass.set_bind_group(2, &self.water_planes_bind_group, &[]);
            render_pass.set_bind_group(3, &self.heightmap_bind_group, &[]);
            render_pass.set_bind_group(4, self.shadow.receiver_bind_group(), &[]);
            render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            // Ground geometry only -- skirts + heightmap displacement grid.
            // Water plane is drawn last (after features) so underwater features
            // composite through the water's alpha blend instead of being
            // depth-culled by the water surface.
            render_pass.draw_indexed(0..self.water_index_offset, 0, 0..1);

            if self.quality_high {
                render_pass.set_pipeline(&self.sky_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.draw(0..3, 0..1);
            }
        }

        // Feature pass: writes color + depth for features, including any below
        // the water plane. Runs BEFORE the water draw so the water's alpha
        // blend layers cleanly over underwater features.
        if let (Some(ref fr), Some(ref depth_view)) = (&self.feature_renderer, &self.depth_texture)
        {
            fr.draw(
                &mut encoder,
                output_view,
                depth_view,
                &self.camera_bind_group,
                self.shadow.receiver_bind_group(),
            );
        }

        // Water pass: draws only the water-plane sub-range of the terrain
        // index buffer. LoadOp::Load keeps everything from the ground + feature
        // passes. The water shader's ALPHA_BLENDING composites the planar
        // reflection / refraction over what's already there, including
        // underwater features.
        if self.water_index_offset < self.num_indices {
            let mut water_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("water_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            water_pass.set_pipeline(&self.render_pipeline);
            water_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            water_pass.set_bind_group(1, &self.texture_bind_group, &[]);
            water_pass.set_bind_group(2, &self.water_planes_bind_group, &[]);
            water_pass.set_bind_group(3, &self.heightmap_bind_group, &[]);
            water_pass.set_bind_group(4, self.shadow.receiver_bind_group(), &[]);
            water_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            water_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            water_pass.draw_indexed(self.water_index_offset..self.num_indices, 0, 0..1);
        }

        queue.submit(std::iter::once(encoder.finish()));

        // ── Gamma-encode post-pass ───────────────────────────────────────
        // Sample the perceptual render target and write pow(c, 2.2) into
        // `display_texture`. egui binds the display view (see
        // `output_view()`), the sRGB swapchain re-encodes the gamma-
        // darkened pixels back to BAR's raw perceptual bytes, and the
        // display gamma decodes to V^2.2 -- the engine appearance. This
        // runs after every main-pass write so cross-pass intermediates
        // (refraction / reflection) stay perceptual for their samplers.
        if let (Some(display_view), Some(gamma_bg)) =
            (self.display_view.as_ref(), self.gamma_bind_group.as_ref())
        {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("gamma_encode_encoder"),
            });
            {
                let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("gamma_encode_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: display_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                rp.set_pipeline(&self.gamma_pipeline);
                rp.set_bind_group(0, gamma_bg, &[]);
                rp.draw(0..3, 0..1);
            }
            queue.submit(std::iter::once(enc.finish()));
        }
    }

    // ── Accessors ───────────────────────────────────────────────────────────

    pub fn has_mesh(&self) -> bool {
        self.vertex_buffer.is_some()
    }

    fn clear_mesh(&mut self) {
        self.vertex_buffer = None;
        self.index_buffer = None;
        self.num_indices = 0;
        self.water_index_offset = 0;
        self.water_y = -1.0;
        self.has_albedo = false;
    }

    /// World-space geometry extents used by the CPU ray-cast picker.
    /// Returns `(height_scale, x_extent, z_extent)`.
    pub fn mesh_extents(&self) -> (f32, f32, f32) {
        (self.height_scale, self.x_extent, self.z_extent)
    }

    /// Display-bound texture view. This is the gamma-encoded copy of the
    /// perceptual render target, written by the gamma post-pass at the end
    /// of `render_internal`. egui samples this view so the sRGB swapchain
    /// re-encoding cancels back to BAR's raw perceptual bytes, landing the
    /// final display intensity at V^2.2 -- matching the engine.
    pub fn output_view(&self) -> Option<&wgpu::TextureView> {
        self.display_view.as_ref()
    }

    pub fn depth_texture_view(&self) -> Option<&wgpu::TextureView> {
        self.depth_texture.as_ref()
    }

    pub fn camera_bind_group(&self) -> &wgpu::BindGroup {
        &self.camera_bind_group
    }

    pub fn depth_format(&self) -> wgpu::TextureFormat {
        self.depth_format
    }

    /// Mutable access to the feature renderer for uploading instance data.
    pub fn feature_renderer_mut(&mut self) -> Option<&mut crate::features::FeatureRenderer> {
        self.feature_renderer.as_mut()
    }

    /// Read-only access to the feature renderer for queries like
    /// `has_model`.
    pub fn feature_renderer(&self) -> Option<&crate::features::FeatureRenderer> {
        self.feature_renderer.as_ref()
    }

    /// Upload an S3O model and its two textures for a named feature type.
    /// `tex1` is the diffuse (rgb) + team mask (a) channel; `tex2` is the
    /// shading (rgb) + opacity (a) channel. When either is `None` the feature
    /// renderer substitutes a 1x1 white default so the mesh still draws.
    pub fn load_feature_mesh(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        name: &str,
        mesh: &bar_data::S3oMesh,
        tex1: Option<&crate::features::FeatureTexture>,
        tex2: Option<&crate::features::FeatureTexture>,
    ) {
        if let Some(ref mut fr) = self.feature_renderer {
            fr.load_mesh(device, queue, name, mesh, tex1, tex2);
        }
    }

    /// Upload grouped feature instances.
    pub fn update_feature_instances(
        &mut self,
        device: &wgpu::Device,
        groups: &std::collections::HashMap<String, Vec<crate::features::FeatureInstance>>,
        unknowns: &[crate::features::FeatureInstance],
    ) {
        if let Some(ref mut fr) = self.feature_renderer {
            fr.update_instances_grouped(device, groups, unknowns);
        }
    }

    /// Copy the rendered output back to a CPU RGBA8 buffer. Used by the
    /// headless CLI preview command. Returns `None` if no render has occurred.
    ///
    /// Reads the gamma-encoded display target so the resulting PNG matches
    /// what the editor viewport shows -- without this, the PNG would be
    /// the raw perceptual buffer (too bright when viewed in any sRGB-aware
    /// image viewer, the same mismatch the gamma post-pass corrects for
    /// the editor's sRGB swapchain).
    pub fn read_pixels(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Vec<u8>> {
        let texture = self.display_texture.as_ref()?;
        let w = self.width;
        let h = self.height;
        let bytes_per_pixel = 4u32;
        let unpadded_bpr = w * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bpr = unpadded_bpr.div_ceil(align) * align;
        let buffer_size = (padded_bpr * h) as wgpu::BufferAddress;

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terrain_readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("terrain_readback_encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bpr),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::Maintain::Wait);
        rx.recv().ok()?.ok()?;

        let raw = slice.get_mapped_range();
        let mut out = Vec::with_capacity((unpadded_bpr * h) as usize);
        for row in 0..h {
            let start = (row * padded_bpr) as usize;
            out.extend_from_slice(&raw[start..start + unpadded_bpr as usize]);
        }
        drop(raw);
        staging.unmap();
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn terrain_shader_wgsl_parses() {
        let modern_sky = include_str!("../../../shaders/recoil/modern_sky.wgsl");
        let smf_ground = include_str!("../../../shaders/recoil/smf_ground.wgsl");
        let water = include_str!("../../../shaders/water.wgsl");
        let terrain = include_str!("../../../shaders/terrain.wgsl");
        let combined = format!("{modern_sky}\n{smf_ground}\n{water}\n{terrain}");
        let module = naga::front::wgsl::parse_str(&combined);
        assert!(
            module.is_ok(),
            "terrain shader failed to parse: {:?}",
            module.err()
        );
    }

    #[test]
    fn feature_shader_wgsl_parses() {
        // FeatureRenderer concatenates the SMF ground-shade helper in front
        // of features.wgsl so feature shading is identical to terrain shading;
        // mirror that here so the parse test matches the runtime module.
        let smf_ground = include_str!("../../../shaders/recoil/smf_ground.wgsl");
        let features = include_str!("../../../shaders/features.wgsl");
        let combined = format!("{smf_ground}\n{features}");
        let module = naga::front::wgsl::parse_str(&combined);
        assert!(
            module.is_ok(),
            "feature shader failed to parse: {:?}",
            module.err()
        );
    }

    #[test]
    fn shadow_terrain_shader_wgsl_parses() {
        let s = include_str!("../../../shaders/shadow_terrain.wgsl");
        let module = naga::front::wgsl::parse_str(s);
        assert!(
            module.is_ok(),
            "shadow_terrain shader failed to parse: {:?}",
            module.err()
        );
    }

    #[test]
    fn shadow_feature_shader_wgsl_parses() {
        let s = include_str!("../../../shaders/shadow_feature.wgsl");
        let module = naga::front::wgsl::parse_str(s);
        assert!(
            module.is_ok(),
            "shadow_feature shader failed to parse: {:?}",
            module.err()
        );
    }

    #[test]
    fn minimap_shader_wgsl_parses() {
        let minimap = include_str!("../../../shaders/recoil/minimap.wgsl");
        let module = naga::front::wgsl::parse_str(minimap);
        assert!(
            module.is_ok(),
            "minimap shader failed to parse: {:?}",
            module.err()
        );
    }

    #[test]
    fn gamma_encode_shader_wgsl_parses() {
        let s = include_str!("../../../shaders/gamma_encode.wgsl");
        let module = naga::front::wgsl::parse_str(s);
        assert!(
            module.is_ok(),
            "gamma_encode shader failed to parse: {:?}",
            module.err()
        );
    }

    // ── Texture-format convention tests ────────────────────────────────
    //
    // Engine-faithful pipeline: BAR uploads every texture without the
    // sRGB flag (`bar-recoil/.../GL/State.h:185`), so its samplers
    // return raw `byte/255` to the shader and the shader's math runs
    // in sRGB-perceptual space throughout. BME mirrors this: every
    // texture uses a non-sRGB wgpu format so the GPU passes bytes
    // through unchanged. Pinned here so an accidental sRGB choice
    // surfaces immediately.

    #[test]
    fn colour_textures_use_linear_format() {
        // BAR-faithful: colour textures use the non-sRGB Rgba8Unorm
        // variant so the GPU returns raw byte/255 to the shader.
        // The shader's lighting math then operates on perceptual
        // values throughout, matching BAR's gamma-incorrect but
        // visually-consistent appearance.
        assert_eq!(
            super::COLOUR_TEX_FORMAT,
            wgpu::TextureFormat::Rgba8Unorm,
            "colour textures must use the non-sRGB variant -- BAR's \
             samplers return byte/255 and all shader math stays in \
             sRGB-perceptual space"
        );
    }

    #[test]
    fn bc1_colour_textures_use_linear_format() {
        // Same reasoning for the BC1-compressed SMT terrain atlas.
        assert_eq!(
            super::COLOUR_TEX_FORMAT_BC1,
            wgpu::TextureFormat::Bc1RgbaUnorm,
            "BC1 colour textures must use the non-sRGB variant -- \
             same gamma-incorrect-but-consistent reasoning as \
             COLOUR_TEX_FORMAT"
        );
    }

    #[test]
    fn splat_detail_normal_uses_linear_format() {
        // Splat detail-normals carry tangent-space normal coordinates
        // (RGB) and detail strength (A). `SMFFragProg.glsl:183` decodes
        // them via `(sample * 2 - 1)`. sRGB decode would corrupt the
        // [-1, 1] range; must be linear.
        assert_eq!(
            super::SPLAT_DETAIL_NORMAL_TEX_FORMAT,
            wgpu::TextureFormat::Rgba8Unorm,
            "splat detail-normal channels are normal-coordinate data, \
             not colour -- sRGB sampling would shift byte 128 from 0.5 \
             to ~0.22 and turn 'no perturbation' into -0.56"
        );
    }

    #[test]
    fn splat_distribution_uses_linear_format() {
        // Splat distribution channels are direct material weights
        // sampled and multiplied with `splatTexMults` per channel
        // (`SMFFragProg.glsl:168`). Mid-range weights would shrink to
        // ~44% of authored if sRGB-decoded.
        assert_eq!(
            super::SPLAT_DISTR_TEX_FORMAT,
            wgpu::TextureFormat::Rgba8Unorm,
            "splat distribution weights are data, not colour"
        );
    }

    #[test]
    fn sky_reflect_mod_uses_linear_format() {
        // Per-channel mix factor for `mix(diffuse, reflect, reflectMod)`
        // (`SMFFragProg.glsl:348`). Mix weights are interpolation data.
        assert_eq!(
            super::SKY_REFLECT_MOD_TEX_FORMAT,
            wgpu::TextureFormat::Rgba8Unorm,
            "sky-reflection mod channels are mix factors, not colour"
        );
    }

    #[test]
    fn splat_input_formats_coincide() {
        // The `make_splat_default` / `update_splat_textures::make`
        // closures build both detail-normal and distribution textures
        // through a single format choice. If these constants diverge
        // the closures need to grow a format parameter.
        assert_eq!(
            super::SPLAT_DETAIL_NORMAL_TEX_FORMAT,
            super::SPLAT_DISTR_TEX_FORMAT,
            "splat detail-normal and distribution share a closure -- their formats must match"
        );
    }

    #[test]
    fn specular_tex_uses_linear_format() {
        // RGB used as `specCol.rgb * specularPow`, A used as
        // `exp = A * 16` (`SMFFragProg.glsl:413,419`). Both channels
        // are face-value data; engine reads them direct (no
        // `GL_FRAMEBUFFER_SRGB`), so we match by using a linear format.
        assert_eq!(
            super::SPECULAR_TEX_FORMAT,
            wgpu::TextureFormat::Rgba8Unorm,
            "specular RGB is a face-value multiplier and A is the \
             exponent encoding; both must be sampled in linear domain"
        );
    }

    #[test]
    fn smf_lighting_passes_colours_through_in_perceptual_space() {
        // Engine-faithful pipeline: `SmfLighting::from(&MapSettings)`
        // passes every mapinfo colour triple through unchanged. BAR's
        // shaders treat textures and uniforms as sRGB-perceptual values
        // (no GPU decode on sample, no framebuffer encode on write), so
        // BME's shader receives the raw perceptual values too. Pin the
        // convention here -- any future "fix" to sRGB-decode at the
        // boundary will silently re-darken the playable area and shift
        // the colour balance.
        let mut ms = bar_project::MapSettings::default();
        ms.atmosphere.fog_color = [0.11, 0.13, 0.15];
        ms.atmosphere.sun_color = [1.0, 0.92, 0.78];
        ms.atmosphere.sky_color = [0.43, 0.58, 0.64];
        ms.atmosphere.cloud_color = [0.9, 0.9, 0.9];
        ms.lighting.ground_ambient = [0.56, 0.55, 0.55];
        ms.lighting.ground_diffuse = [0.75, 0.75, 0.8];
        ms.lighting.ground_specular = [0.5, 0.5, 0.5];
        ms.water.absorb = [0.011, 0.011, 0.015];
        ms.water.base_color = [0.5, 0.68, 0.68];
        ms.water.min_color = [0.022, 0.0035, 0.035];
        ms.water.surface_color = [0.5, 0.6, 0.65];
        ms.water.diffuse_color = [1.0, 1.0, 1.0];
        ms.water.specular_color = [0.65, 0.65, 0.7];
        ms.custom_fog.color = [0.3, 0.4, 0.5];

        let smf = super::SmfLighting::from(&ms);

        assert_eq!(smf.atmosphere_fog_color, ms.atmosphere.fog_color);
        assert_eq!(smf.sun_color, ms.atmosphere.sun_color);
        assert_eq!(smf.sky_color, ms.atmosphere.sky_color);
        assert_eq!(smf.cloud_color, ms.atmosphere.cloud_color);
        assert_eq!(smf.ground_ambient, ms.lighting.ground_ambient);
        assert_eq!(smf.ground_diffuse, ms.lighting.ground_diffuse);
        assert_eq!(smf.ground_specular, ms.lighting.ground_specular);
        assert_eq!(smf.water_absorb, ms.water.absorb);
        assert_eq!(smf.water_base, ms.water.base_color);
        assert_eq!(smf.water_min, ms.water.min_color);
        assert_eq!(smf.water_surface_color, ms.water.surface_color);
        assert_eq!(smf.water_diffuse_color, ms.water.diffuse_color);
        assert_eq!(smf.water_specular_color, ms.water.specular_color);
        assert_eq!(smf.custom_fog_color, ms.custom_fog.color);
    }
}

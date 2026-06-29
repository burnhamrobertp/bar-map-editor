//! Recipe format: a human-friendly serializable graph configuration.
//!
//! Recipes use stable string keys for nodes (not internal IDs) and validate
//! on load by constructing the graph through proper APIs.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use bar_graph::{GraphEngine, Node, NodeId, NodeType, ParamValue, PortId};

/// Default `depend` list for new recipes. Every BAR map references
/// `Map Helper v1` to get the common mapconfig loaders, so the
/// default is the conservative thing.
fn default_depend() -> Vec<String> {
    vec!["Map Helper v1".to_string()]
}

/// A complete pipeline recipe — the on-disk format for the editor's graphs.
///
/// Identity fields (`name`, `shortname`, `description`, `author`,
/// `version`) live here as the **single source of truth**. The
/// bundler reads them when generating `mapinfo.lua`; nothing else
/// should keep its own copy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    /// Human-readable map name. Becomes mapinfo's `name` and the
    /// stem of generated map files (`<name>.smf`, `<name>.smt`).
    pub name: String,
    /// Optional short identifier. When `None` the bundler uses
    /// `name` for mapinfo's `shortname`. Lets a long display name
    /// like "Kolmog Estuary 1v1" coexist with a tighter id like
    /// "kolmog_1v1".
    #[serde(default)]
    pub shortname: Option<String>,
    /// In-game map description (becomes mapinfo's `description`).
    /// Empty string is allowed.
    #[serde(default)]
    pub description: String,
    /// Author string for mapinfo. When `None` the bundler falls
    /// back to `"bar-editor"`.
    #[serde(default)]
    pub author: Option<String>,
    /// Map version string. Convention is "MAJOR.MINOR" or a
    /// content-flavoured tag ("v3", "playtest-2"). When `None` the
    /// bundler falls back to `"1.0"`.
    #[serde(default)]
    pub version: Option<String>,
    /// Optional short tooltip text shown by the lobby when the user
    /// hovers the map name. Free-form; typically a sentence or two.
    /// Becomes mapinfo's `tip` field.
    #[serde(default)]
    pub tip: Option<String>,
    /// Archives this map's content depends on. Becomes mapinfo's
    /// `depend = { ... }` table. Almost universally
    /// `["Map Helper v1"]` for BAR maps -- the helper archive
    /// supplies the boilerplate gadgets and includes mapconfig
    /// authors rely on. Empty vec writes no `depend` entry.
    #[serde(default = "default_depend")]
    pub depend: Vec<String>,
    /// Node definitions, keyed by stable string IDs.
    pub nodes: Vec<RecipeNode>,
    /// Connections between node ports.
    pub connections: Vec<RecipeConnection>,
    /// Output configuration.
    pub output: OutputConfig,
    /// Feature placements (trees, rocks, crystals, etc.).
    /// Preserved from .sd7 import; editable in the sculpt view in a future iteration.
    #[serde(default)]
    pub features: Vec<PlacedFeature>,
}

/// A feature (tree, rock, crystal, etc.) placed on the map.
///
/// Stored as explicit placement data on `Recipe` alongside `MapSettings`,
/// not as a graph node -- features are authored positions, not generated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacedFeature {
    /// Feature type name as referenced in game data (e.g. "arborreal").
    pub feature_type: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    #[serde(default)]
    pub angle: f32,
    #[serde(default)]
    pub taken_damage: i16,
    /// Where this feature originated -- and therefore where it must
    /// land on re-export. SMF-native placements go in the SMF feature
    /// section (capped at 31-char names by the engine reader at
    /// `SMFMapFile.h:62`). FeaturePlacer-set features go in
    /// `mapconfig/featureplacer/set.lua` (no length limit; spawned at
    /// runtime by the FP_featureplacer gadget). Conflating the two
    /// is what causes the engine's SMF feature reader to truncate
    /// long names mid-string and crash on the resulting garbled type.
    #[serde(default)]
    pub source: FeatureSource,
}

/// Where a `PlacedFeature` came from at import time. Determines the
/// destination on re-export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FeatureSource {
    /// Stored in the source SMF's feature section. Re-exported to the
    /// same section -- names MUST fit in 31 chars (engine constraint).
    #[default]
    Smf,
    /// Stored in `mapconfig/featureplacer/set.lua`, spawned at runtime
    /// by the FP_featureplacer gadget. Re-exported to set.lua; engine
    /// never reads it through the SMF feature section, so name length
    /// is unconstrained.
    FeaturePlacerSet,
}

/// A node in the recipe, identified by a stable string key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeNode {
    /// Stable string key used to reference this node in connections.
    pub key: String,
    /// Node type (e.g., "PerlinNoise", "Blur", "Bundler").
    #[serde(rename = "type")]
    pub node_type: NodeType,
    /// Human-readable label.
    #[serde(default)]
    pub label: String,
    /// Parameters for this node.
    #[serde(default)]
    pub params: HashMap<String, ParamValue>,
}

/// A connection between two ports in the recipe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeConnection {
    /// Source: "node_key.port_name"
    pub from: String,
    /// Destination: "node_key.port_name"
    pub to: String,
}

/// Output configuration for the pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    /// Output width in pixels (heightmap resolution).
    pub width: u32,
    /// Output height in pixels (heightmap resolution).
    pub height: u32,
    /// Map-level settings for export (mapinfo.lua generation, DNTS, etc.)
    #[serde(default)]
    pub map_settings: MapSettings,
}

/// Map-level export settings — controls the physics / atmosphere / lighting
/// / water sections of mapinfo.lua, plus DNTS references and team spawns.
/// All fields are optional; sensible defaults are used when not specified.
///
/// Identity fields (`name`, `shortname`, `description`, `author`, `version`)
/// and map dimensions (`width`, `height`) live on the parent `Recipe` /
/// `OutputConfig`, not here — keeping them in one place keeps the mapinfo
/// editor and project save/load agreed on a single source of truth.
///
/// The bundler generates `mapinfo.lua` from `Recipe` + this struct on every
/// SD7 export; nothing else may produce a mapinfo.lua (a PassThrough or
/// FileReference with that destination is rejected at validation time).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapSettings {
    /// Heights and physics scalars are `Option<T>` so the recipe
    /// captures "user-set / present in source mapinfo" vs "fall
    /// through to engine default" without a hidden equality check.
    /// See `engine_defaults.rs` for the values `None` resolves to.
    pub min_height: Option<f32>,
    pub max_height: Option<f32>,
    pub map_hardness: Option<u32>,
    pub gravity: Option<f32>,

    /// Detail Normal Texture Set (DNTS) — paths to tiling textures for each terrain type.
    /// Up to 4 entries (one per splat channel).
    #[serde(default)]
    pub detail_textures: Vec<DetailTexture>,

    pub deformable: Option<bool>,
    pub void_water: Option<bool>,
    pub void_ground: Option<bool>,
    pub tidal_strength: Option<f32>,
    pub max_metal: Option<f32>,
    pub extractor_radius: Option<f32>,
    /// Engine `autoShowMetal`. Toggles the F4 "metal map" overlay
    /// being shown by default at game start.
    pub auto_show_metal: Option<bool>,
    /// Engine `replace = { ... }` table -- string-keyed table of
    /// archive-name replacements. Almost always empty on real maps;
    /// modelled here so an empty `{}` survives round-trip.
    #[serde(default)]
    pub replace: ReplaceTable,
    /// Engine `terrainTypes` table. Per-type-index movement / hardness
    /// data the engine reads for each terrain type. Empty when the
    /// source mapinfo didn't define any.
    #[serde(default)]
    pub terrain_types: Vec<TerrainTypeEntry>,

    /// Atmosphere settings.
    #[serde(default)]
    pub atmosphere: AtmosphereSettings,
    /// Lighting settings.
    #[serde(default)]
    pub lighting: LightingSettings,
    /// Water settings -- always present; only consumed by the
    /// renderer when [`MapSettings::fluid_mode`] resolves to
    /// [`FluidMode::Water`].
    #[serde(default)]
    pub water: WaterSettings,
    /// Lava widget settings -- consumed when `fluid_mode == Lava`.
    #[serde(default)]
    pub lava: LavaSettings,
    /// Which fluid covers the map's water plane. `None` defers to
    /// the [`FluidMode`] default (Water). Mutually exclusive at the
    /// engine level -- see the type docs.
    #[serde(default)]
    pub fluid_mode: Option<FluidMode>,

    /// Height-based custom fog (mapinfo `custom.fog`).
    #[serde(default)]
    pub custom_fog: CustomFogSettings,

    /// Grass widget configuration (mapinfo `custom.grassConfig`).
    #[serde(default)]
    pub custom_grass: CustomGrassSettings,

    /// Volumetric-cloud widget settings (mapinfo `custom.clouds`).
    /// Drives the in-game cloud renderer; round-trips through the
    /// recipe so re-bundles preserve the source's authored values.
    #[serde(default)]
    pub custom_clouds: CustomCloudsSettings,

    /// EFX audio reverb / passfilter / preset (mapinfo `sound`).
    #[serde(default)]
    pub sound: SoundSettings,

    /// Per-map asset filenames from mapinfo `resources = { ... }`.
    #[serde(default)]
    pub resources: ResourcesSettings,

    /// User-chosen minimap source file (basename inside `passthrough/`).
    /// `None` falls back to the SMF-embedded minimap sidecar on import,
    /// and to a freshly generated minimap derived from the terrain
    /// texture on bundle. Not part of mapinfo.lua; the SMF binary
    /// embeds the minimap as a DXT1 chunk.
    #[serde(default)]
    pub minimap: Option<String>,

    /// Team start positions as [(x, z)] in Spring world coordinates.
    /// If empty, auto-generated at 25%/75% corners.
    #[serde(default)]
    pub start_positions: Vec<[u32; 2]>,
}

/// Volumetric clouds widget configuration (mapinfo `custom.clouds`).
/// Authored per-map; the in-game widget reads each field at runtime.
/// Every field is `Option<T>` so an unset cloud key in the source
/// stays unset on round-trip (the widget falls back to its own
/// defaults).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CustomCloudsSettings {
    pub speed: Option<f32>,
    pub color: Option<[f32; 3]>,
    pub height: Option<f32>,
    pub bottom: Option<f32>,
    pub fade_alt: Option<f32>,
    pub scale: Option<f32>,
    pub opacity: Option<f32>,
    pub clamp_to_map: Option<bool>,
    pub sun_penetration: Option<f32>,
}

/// Engine `replace = { [key] = "value" }` map. Almost always empty
/// on real maps; modelled as `Option<HashMap>` so an empty `{}` from
/// the source round-trips as a `Some(empty)` rather than a `None`.
/// Distinguishes "source had no replace block" from "source had an
/// empty replace block" -- the latter still emits `replace = {}` in
/// the bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ReplaceTable {
    /// Whether the source declared a `replace = ...` key at all.
    /// Drives whether we emit the (often empty) block on bundle.
    pub declared: bool,
    /// The key-value entries inside the table (rare in real maps).
    pub entries: Vec<(String, String)>,
}

/// One entry from the `terrainTypes` table. The engine indexes these
/// by terrain-type-id (0-255) to drive per-type movement / damage /
/// track-receiving behaviour. Real maps usually define a small number
/// of explicit entries plus a fall-through default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TerrainTypeEntry {
    /// Terrain-type index (the `[N] =` key inside the table).
    pub index: u32,
    pub name: Option<String>,
    pub hardness: Option<f32>,
    pub receive_tracks: Option<bool>,
    /// Per-unit-type movespeed multipliers (`moveSpeeds = { tank = 1.0,
    /// kbot = 1.0, hover = 1.0, ship = 1.0 }`). Stored as a list so
    /// the round-trip preserves declaration order; engine consumes by
    /// key name.
    pub move_speeds: Vec<(String, f32)>,
}

/// EFX audio settings (mapinfo `sound = { preset, passfilter, reverb }`).
/// Preserves the per-map EFX preset + passfilter so audio rendering
/// round-trips. The reverb sub-block (a dozen-plus OpenAL EFX
/// parameters) is rare on real maps -- most leave it as a comment
/// block -- so we don't model it field-by-field; it's an acceptable
/// round-trip loss until a map demands it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SoundSettings {
    pub preset: Option<String>,
    pub passfilter_gainlf: Option<f32>,
    pub passfilter_gainhf: Option<f32>,
}

/// Fully-resolved view of MapSettings -- every `Option<T>` replaced by
/// its effective value via the per-field engine default. The renderer
/// and the validator both consume this so they never need to branch on
/// `Option`. The bundler emits only the original `Option`-valued
/// fields when `Some`.
#[derive(Debug, Clone)]
pub struct ResolvedMapSettings {
    pub min_height: f32,
    pub max_height: f32,
    pub map_hardness: u32,
    pub gravity: f32,
    pub deformable: bool,
    pub void_water: bool,
    pub void_ground: bool,
    pub tidal_strength: f32,
    pub max_metal: f32,
    pub extractor_radius: f32,
    pub atmosphere: ResolvedAtmosphere,
    pub lighting: ResolvedLighting,
    pub water: ResolvedWater,
    pub lava: ResolvedLava,
    pub fluid_mode: FluidMode,
    pub custom_grass: ResolvedGrassSettings,
}

impl MapSettings {
    pub fn resolved(&self) -> ResolvedMapSettings {
        use crate::engine_defaults as ed;
        ResolvedMapSettings {
            min_height: self.min_height.unwrap_or(ed::MAP_MIN_HEIGHT),
            max_height: self.max_height.unwrap_or(ed::MAP_MAX_HEIGHT),
            map_hardness: self.map_hardness.unwrap_or(ed::MAP_HARDNESS),
            gravity: self.gravity.unwrap_or(ed::MAP_GRAVITY),
            deformable: self.deformable.unwrap_or(!ed::MAP_NOT_DEFORMABLE),
            void_water: self.void_water.unwrap_or(ed::MAP_VOID_WATER),
            void_ground: self.void_ground.unwrap_or(ed::MAP_VOID_GROUND),
            tidal_strength: self.tidal_strength.unwrap_or(ed::MAP_TIDAL_STRENGTH),
            max_metal: self.max_metal.unwrap_or(ed::MAP_MAX_METAL),
            extractor_radius: self.extractor_radius.unwrap_or(ed::MAP_EXTRACTOR_RADIUS),
            atmosphere: self.atmosphere.resolved(),
            lighting: self.lighting.resolved(),
            water: self.water.resolved(),
            lava: self.lava.resolved(),
            fluid_mode: self.fluid_mode.unwrap_or_default(),
            custom_grass: self.custom_grass.resolved(),
        }
    }
}

/// Detail texture entry for DNTS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetailTexture {
    /// Path to the diffuse/normal tiling texture (relative to map archive).
    pub path: String,
    /// Texture scale (smaller = more tiled repetition).
    #[serde(default = "default_tex_scale")]
    pub scale: f32,
}

fn default_tex_scale() -> f32 {
    0.02
}

/// Atmosphere configuration for mapinfo.lua.
///
/// The wind / fog fields drive gameplay + the engine's standard distance fog.
/// The `sun_*`, `sky_*`, `cloud_*` fields feed the procedural sky shader
/// (`shaders/recoil/modern_sky.wgsl`) so each map gets its authored sky
/// instead of a single hard-coded one. `skybox` is the asset name for a
/// cubemap DDS; not currently rendered, but stored so it round-trips
/// through `.barproj` and is available when cubemap skybox support lands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AtmosphereSettings {
    pub min_wind: Option<f32>,
    pub max_wind: Option<f32>,
    pub fog_start: Option<f32>,
    pub fog_end: Option<f32>,
    pub fog_color: Option<[f32; 3]>,
    pub sun_color: Option<[f32; 3]>,
    pub sky_color: Option<[f32; 3]>,
    pub sky_dir: Option<[f32; 3]>,
    pub cloud_density: Option<f32>,
    pub cloud_color: Option<[f32; 3]>,
    pub skybox: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedAtmosphere {
    pub min_wind: f32,
    pub max_wind: f32,
    pub fog_start: f32,
    pub fog_end: f32,
    pub fog_color: [f32; 3],
    pub sun_color: [f32; 3],
    pub sky_color: [f32; 3],
    pub sky_dir: [f32; 3],
    pub cloud_density: f32,
    pub cloud_color: [f32; 3],
    pub skybox: String,
}

impl AtmosphereSettings {
    pub fn resolved(&self) -> ResolvedAtmosphere {
        use crate::engine_defaults as ed;
        ResolvedAtmosphere {
            min_wind: self.min_wind.unwrap_or(ed::ATMOSPHERE_MIN_WIND),
            max_wind: self.max_wind.unwrap_or(ed::ATMOSPHERE_MAX_WIND),
            fog_start: self.fog_start.unwrap_or(ed::ATMOSPHERE_FOG_START),
            fog_end: self.fog_end.unwrap_or(ed::ATMOSPHERE_FOG_END),
            fog_color: self.fog_color.unwrap_or(ed::ATMOSPHERE_FOG_COLOR),
            sun_color: self.sun_color.unwrap_or(ed::ATMOSPHERE_SUN_COLOR),
            sky_color: self.sky_color.unwrap_or(ed::ATMOSPHERE_SKY_COLOR),
            sky_dir: self.sky_dir.unwrap_or(ed::ATMOSPHERE_SKY_DIR),
            cloud_density: self.cloud_density.unwrap_or(ed::ATMOSPHERE_CLOUD_DENSITY),
            cloud_color: self.cloud_color.unwrap_or(ed::ATMOSPHERE_CLOUD_COLOR),
            skybox: self.skybox.clone().unwrap_or_default(),
        }
    }
}

/// Asset references from mapinfo's `resources = { ... }` block. These
/// are filename-only references (`detailTex = "detailtexblurred.bmp"`);
/// the renderer resolves them against the `.barproj/passthrough/`
/// tree at load time. Empty string = unspecified, fall back to the
/// engine's "no detail" default in the shader.
///
/// Currently only `detail_tex` is wired -- the engine has many more
/// (`specularTex`, `splatDistrTex`, `splatDetailTex`, four detail
/// normal textures, etc). The struct exists so they can be added
/// incrementally without further schema migrations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ResourcesSettings {
    /// Legacy single detail texture (mapinfo `detailTex`). Tiles
    /// across the map; the shader subtracts 0.5 to centre it so the
    /// texture both lightens and darkens the base diffuse.
    pub detail_tex: String,
    /// 4-channel splat distribution map (mapinfo `splatDistrTex`).
    /// Each channel R/G/B/A weights one splat-detail texture.
    pub splat_distr_tex: String,
    /// Four splat-detail-normal textures (mapinfo `splatDetailNormalTex1`
    /// through `4`). Used by the engine's `SMF_DETAIL_NORMAL_TEXTURE_SPLATTING`
    /// path. RGB encodes a tangent-space normal perturbation; alpha
    /// (when `splat_detail_normal_diffuse_alpha = true`) is the
    /// per-pixel detail brightness contribution.
    pub splat_detail_normal_tex_1: String,
    pub splat_detail_normal_tex_2: String,
    pub splat_detail_normal_tex_3: String,
    pub splat_detail_normal_tex_4: String,
    /// When true, the alpha channel of each splat-detail-normal texture
    /// provides the detail brightness (Aurelia sets this). When false,
    /// only the normal perturbation is used and detail brightness is
    /// constant 0.
    pub splat_detail_normal_diffuse_alpha: bool,
    /// Per-channel UV scale for splat-detail sampling (mapinfo
    /// `splats.texScales`). `None` = source mapinfo didn't carry a
    /// `splats` block, so the runtime falls back to engine defaults
    /// (1.0 per channel). Aurelia: `{0.0032, 0.0063, 0.0044, 0.0055}`.
    pub splat_tex_scales: Option<[f32; 4]>,
    /// Per-channel mix multiplier (mapinfo `splats.texMults`).
    pub splat_tex_mults: Option<[f32; 4]>,
    /// Per-pixel reflection-strength mask (mapinfo `skyReflectModTex`).
    /// When set, the terrain shader samples this 2D texture to decide
    /// where the skybox cubemap reflects on the surface. Pixels with
    /// rgb=(0,0,0) get no reflection; (1,1,1) gets full reflection.
    /// Engine path: `SMF_SKY_REFLECTIONS`.
    pub sky_reflect_mod_tex: String,
    /// Per-pixel specular-strength texture (mapinfo `specularTex`).
    /// Engine path: `SMF_SPECULAR_LIGHTING`. When set, the terrain shader
    /// samples this texture instead of using the global
    /// `groundSpecularColor` -- `texture.rgb` = per-pixel specular color,
    /// `texture.a * 16` = per-pixel specular exponent. Most natural
    /// terrain has near-zero values here (only water pools / metal /
    /// ice are visibly reflective); without this texture, the shader
    /// applies the global `groundSpecularColor` everywhere, which is
    /// why maps like Ascendancy (`groundSpecularColor = {0.5, 0.5, 0.5}`)
    /// were producing sun-side hotspots across the whole map.
    pub specular_tex: String,
    /// Texture sampled by BAR's `map_edge_extension2` LuaUI widget to
    /// fill the area outside the playable map (mapinfo `grassShadingTex`).
    /// Engine declares this as the `MAP_BASE_GRASS_TEX` and falls back
    /// to the SMF-embedded minimap when unset. Maps like Onyx Cauldron
    /// set this to a custom rocky texture so the "off-map" region
    /// reads as cliffs / ocean rather than a mirror of the playable
    /// area. Independent of the engine grass renderer (BAR doesn't
    /// use that drawer).
    pub grass_shading_tex: String,
    /// Self-illumination texture (mapinfo `lightEmissionTex` — read by
    /// the engine from the `resources = { ... }` table even though the
    /// C++ struct stores it under `smf`, see
    /// `bar-recoil/rts/Map/MapInfo.cpp:357,368`). Engine path
    /// `SMF_LIGHT_EMISSION` (`SMFFragProg.glsl:392-401`): unshadowed
    /// glow term that overrides the underlying fragment colour by
    /// `(1 - emit.a)` weighted blend. Empty string == no emission.
    pub light_emission_tex: String,
    /// Tangent-space normal perturbation texture (mapinfo
    /// `detailNormalTex`, engine path `SMF_BLEND_NORMALS`,
    /// `SMFFragProg.glsl:299-307`). Sampled at the world-XZ
    /// normalised UV; alpha gates the mix weight that perturbs the
    /// surface normal. Empty string == no perturbation.
    pub detail_normal_tex: String,
    /// Basic 4-channel colour splat texture (mapinfo
    /// `splatDetailTex` singular, engine path
    /// `SMF_DETAIL_TEXTURE_SPLATTING`, `SMFFragProg.glsl:80-85,
    /// 159-169`). Mutually exclusive with the normal-splat path;
    /// modern BAR maps use the normal-splat variant. Empty string
    /// == no basic-splat contribution.
    pub splat_detail_tex: String,
}

/// `splatDetailTex` is a presence flag for the SSMF detail-normal splat
/// path, not a sampled texture: the engine enables the splat pipeline only
/// when it is non-empty, but never reads the file. BAR maps point it at a
/// throwaway name; this is the de-facto community value ("I want detail
/// normal textures"). Emitted when a map has splat detail normals but no
/// explicit flag, so the normals don't silently render nothing in-game.
pub const SPLAT_DETAIL_FLAG_PLACEHOLDER: &str = "iwantdnts.tga";

/// Lighting configuration for mapinfo.lua.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LightingSettings {
    pub sun_dir: Option<[f32; 3]>,
    pub sun_intensity: Option<f32>,
    pub ground_ambient: Option<[f32; 3]>,
    pub ground_diffuse: Option<[f32; 3]>,
    pub ground_specular: Option<[f32; 3]>,
    pub spec_exponent: Option<f32>,
    pub ground_shadow_density: Option<f32>,
    /// Per-unit-model shadow strength (mapinfo `unitShadowDensity`).
    /// Mirror of ground_shadow_density but applied to unit models;
    /// engine reads from `lightTable.GetFloat("unitShadowDensity")`.
    pub unit_shadow_density: Option<f32>,
    pub unit_ambient: Option<[f32; 3]>,
    pub unit_diffuse: Option<[f32; 3]>,
    pub unit_specular: Option<[f32; 3]>,
}

#[derive(Debug, Clone)]
pub struct ResolvedLighting {
    pub sun_dir: [f32; 3],
    pub sun_intensity: f32,
    pub ground_ambient: [f32; 3],
    pub ground_diffuse: [f32; 3],
    pub ground_specular: [f32; 3],
    pub spec_exponent: f32,
    pub ground_shadow_density: f32,
    pub unit_shadow_density: f32,
    pub unit_ambient: [f32; 3],
    pub unit_diffuse: [f32; 3],
    pub unit_specular: [f32; 3],
}

impl LightingSettings {
    pub fn resolved(&self) -> ResolvedLighting {
        use crate::engine_defaults as ed;
        ResolvedLighting {
            sun_dir: self.sun_dir.unwrap_or(ed::LIGHTING_SUN_DIR),
            sun_intensity: self.sun_intensity.unwrap_or(ed::LIGHTING_SUN_INTENSITY),
            ground_ambient: self.ground_ambient.unwrap_or(ed::LIGHTING_GROUND_AMBIENT),
            ground_diffuse: self.ground_diffuse.unwrap_or(ed::LIGHTING_GROUND_DIFFUSE),
            ground_specular: self.ground_specular.unwrap_or(ed::LIGHTING_GROUND_SPECULAR),
            spec_exponent: self.spec_exponent.unwrap_or(ed::LIGHTING_SPEC_EXPONENT),
            ground_shadow_density: self
                .ground_shadow_density
                .unwrap_or(ed::LIGHTING_GROUND_SHADOW_DENSITY),
            // Engine `unitShadowDensity` default mirrors ground_shadow_density
            // when omitted (`MapInfo.cpp::ReadLight`).
            unit_shadow_density: self
                .unit_shadow_density
                .unwrap_or(ed::LIGHTING_GROUND_SHADOW_DENSITY),
            unit_ambient: self.unit_ambient.unwrap_or(ed::LIGHTING_GROUND_AMBIENT),
            unit_diffuse: self.unit_diffuse.unwrap_or(ed::LIGHTING_GROUND_DIFFUSE),
            unit_specular: self.unit_specular.unwrap_or(ed::LIGHTING_GROUND_SPECULAR),
        }
    }
}

/// Height-based "custom" fog (mapinfo's `custom.fog = { color, height, fogatten }`
/// block). Not part of the engine's core SMF/BumpWater pipeline; in-game it's
/// applied by a Lua widget that tints fragments below `height` by `color`,
/// attenuated by `atten` per elmo of depth. We do the same thing here as a
/// final post-pass in the terrain and water fragment shaders so previews
/// match the in-game look (this is what gives underwater terrain its deep
/// blue cast on maps like Aurelia, where the SMF water-absorption alone
/// leaves the seabed too warm).
///
/// `enabled` gates the whole pass; when false the shaders bypass the mix.
/// `height_elmos` is the resolved height (mapinfo allows "40%" of MaxHeight
/// which the importer must resolve into absolute elmos before storing here).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomFogSettings {
    pub enabled: bool,
    pub color: [f32; 3],
    pub height_elmos: f32,
    pub atten: f32,
}

impl Default for CustomFogSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            color: [0.0, 0.0, 0.0],
            height_elmos: 0.0,
            atten: 0.0,
        }
    }
}

/// Grass widget configuration (mapinfo `custom.grassConfig`). Driven
/// by BAR's `map_grass_gl4` LuaUI widget (`bar-game/luaui/Widgets/
/// map_grass_gl4.lua`). When `dist_tga` is `None` or empty the widget
/// is considered disabled for the map -- BAR's widget similarly
/// requires this asset to be present before it generates any blades.
///
/// Every field is `Option<T>`: `None` means "not set in source mapinfo
/// and not edited by the user", which the renderer/emitter resolves
/// to the engine default at the point of use via [`resolved`]. The
/// emitter writes a field to the bundled mapinfo only when it is
/// `Some`, so a round-tripped map never carries values the original
/// author didn't explicitly set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CustomGrassSettings {
    pub dist_tga: Option<String>,
    pub blade_color_tex: Option<String>,
    pub max_size: Option<f32>,
    pub min_size: Option<f32>,
    pub patch_resolution: Option<u32>,
    pub patch_placement_jitter: Option<f32>,
    pub map_color_factor: Option<f32>,
    pub map_color_base: Option<f32>,
    pub alpha_threshold: Option<f32>,
    pub shadow_factor: Option<f32>,
    pub grass_brightness: Option<f32>,
    pub fade_start: Option<f32>,
    pub fade_end: Option<f32>,
    pub wind_strength: Option<f32>,
    pub wind_scale: Option<f32>,
    pub wind_sample_scale: Option<f32>,
    pub grass_wind_mult: Option<f32>,
}

/// Fully-resolved grass settings: every `Option<T>` field of
/// [`CustomGrassSettings`] is replaced by its effective value
/// (user-explicit -> source-mapinfo-parsed -> engine default).
/// Produced by [`CustomGrassSettings::resolved`]; consumed by the
/// renderer at the top of each frame so render code never branches
/// on `Option`.
#[derive(Debug, Clone)]
pub struct ResolvedGrassSettings {
    pub dist_tga: String,
    pub blade_color_tex: String,
    pub max_size: f32,
    pub min_size: f32,
    pub patch_resolution: u32,
    pub patch_placement_jitter: f32,
    pub map_color_factor: f32,
    pub map_color_base: f32,
    pub alpha_threshold: f32,
    pub shadow_factor: f32,
    pub grass_brightness: f32,
    pub fade_start: f32,
    pub fade_end: f32,
    pub wind_strength: f32,
    pub wind_scale: f32,
    pub wind_sample_scale: f32,
    pub grass_wind_mult: f32,
}

impl CustomGrassSettings {
    pub fn resolved(&self) -> ResolvedGrassSettings {
        use crate::engine_defaults as ed;
        ResolvedGrassSettings {
            dist_tga: self.dist_tga.clone().unwrap_or_default(),
            blade_color_tex: self.blade_color_tex.clone().unwrap_or_default(),
            max_size: self.max_size.unwrap_or(ed::GRASS_MAX_SIZE),
            min_size: self.min_size.unwrap_or(ed::GRASS_MIN_SIZE),
            patch_resolution: self.patch_resolution.unwrap_or(ed::GRASS_PATCH_RESOLUTION),
            patch_placement_jitter: self
                .patch_placement_jitter
                .unwrap_or(ed::GRASS_PATCH_PLACEMENT_JITTER),
            map_color_factor: self.map_color_factor.unwrap_or(ed::GRASS_MAP_COLOR_FACTOR),
            map_color_base: self.map_color_base.unwrap_or(ed::GRASS_MAP_COLOR_BASE),
            alpha_threshold: self.alpha_threshold.unwrap_or(ed::GRASS_ALPHA_THRESHOLD),
            shadow_factor: self.shadow_factor.unwrap_or(ed::GRASS_SHADOW_FACTOR),
            grass_brightness: self.grass_brightness.unwrap_or(ed::GRASS_BRIGHTNESS),
            fade_start: self.fade_start.unwrap_or(ed::GRASS_FADE_START),
            fade_end: self.fade_end.unwrap_or(ed::GRASS_FADE_END),
            wind_strength: self.wind_strength.unwrap_or(ed::GRASS_WIND_STRENGTH),
            wind_scale: self.wind_scale.unwrap_or(ed::GRASS_WIND_SCALE),
            wind_sample_scale: self
                .wind_sample_scale
                .unwrap_or(ed::GRASS_WIND_SAMPLE_SCALE),
            grass_wind_mult: self.grass_wind_mult.unwrap_or(ed::GRASS_WIND_MULT),
        }
    }

    /// True when the widget should render: a non-empty distribution mask
    /// is the BAR widget's minimum requirement (`map_grass_gl4.lua:857-892`).
    pub fn is_enabled(&self) -> bool {
        self.dist_tga
            .as_deref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }
}

/// Water configuration for mapinfo.lua. Defaults match Recoil's
/// `rts/Map/MapInfo.cpp` (where the engine's BumpWater shader gets its
/// per-map values from). Keys we don't parse from mapinfo yet still have
/// reasonable defaults so a freshly-created project renders sensibly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WaterSettings {
    pub damage: Option<f32>,
    pub absorb: Option<[f32; 3]>,
    pub base_color: Option<[f32; 3]>,
    pub min_color: Option<[f32; 3]>,
    pub surface_color: Option<[f32; 3]>,
    pub surface_alpha: Option<f32>,
    pub diffuse_color: Option<[f32; 3]>,
    pub specular_color: Option<[f32; 3]>,
    pub ambient_factor: Option<f32>,
    pub diffuse_factor: Option<f32>,
    pub specular_factor: Option<f32>,
    pub specular_power: Option<f32>,
    pub fresnel_min: Option<f32>,
    pub fresnel_max: Option<f32>,
    pub fresnel_power: Option<f32>,
    pub reflection_distortion: Option<f32>,
    pub perlin_amplitude: Option<f32>,
    pub blur_base: Option<f32>,
    pub blur_exponent: Option<f32>,
    pub caustics_resolution: Option<f32>,
    pub caustics_strength: Option<f32>,
    pub wave_offset_factor: Option<f32>,
    pub wave_foam_distortion: Option<f32>,
    pub wave_foam_intensity: Option<f32>,
    pub wave_length: Option<f32>,
    /// Engine `water.forceRendering`. Forces the water draw pass even
    /// when the camera is fully above the water plane.
    pub force_rendering: Option<bool>,
    /// `water.hasWaterPlane`. When true the engine draws a flat
    /// infinite water plane behind the map edges.
    pub has_water_plane: Option<bool>,
    /// `water.numTiles`. Tile-count multiplier for the water bump
    /// normal-map sample frequency.
    pub num_tiles: Option<u32>,
    pub perlin_start_freq: Option<f32>,
    pub perlin_lacunarity: Option<f32>,
    pub plane_color: Option<[f32; 3]>,
    /// `water.repeatX` / `water.repeatY`. Optional fixed-pixel UV
    /// repeat counts that override the engine's screen-space default.
    pub repeat_x: Option<f32>,
    pub repeat_y: Option<f32>,
    /// `water.shoreWaves`. Engine toggle for the foam ring along
    /// shorelines.
    pub shore_waves: Option<bool>,
    /// `water.normalTexture`. Filename of a 2D bump-normal tile
    /// override; engine falls back to its built-in pattern when unset.
    pub normal_texture: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedWater {
    pub damage: f32,
    pub absorb: [f32; 3],
    pub base_color: [f32; 3],
    pub min_color: [f32; 3],
    pub surface_color: [f32; 3],
    pub surface_alpha: f32,
    pub diffuse_color: [f32; 3],
    pub specular_color: [f32; 3],
    pub ambient_factor: f32,
    pub diffuse_factor: f32,
    pub specular_factor: f32,
    pub specular_power: f32,
    pub fresnel_min: f32,
    pub fresnel_max: f32,
    pub fresnel_power: f32,
    pub reflection_distortion: f32,
    pub perlin_amplitude: f32,
    pub blur_base: f32,
    pub blur_exponent: f32,
    pub caustics_resolution: f32,
    pub caustics_strength: f32,
    pub wave_offset_factor: f32,
    pub wave_foam_distortion: f32,
    pub wave_foam_intensity: f32,
    pub wave_length: f32,
    pub force_rendering: bool,
    pub has_water_plane: bool,
    pub num_tiles: u32,
    pub perlin_start_freq: f32,
    pub perlin_lacunarity: f32,
    pub plane_color: [f32; 3],
    pub repeat_x: f32,
    pub repeat_y: f32,
    pub shore_waves: bool,
    pub normal_texture: String,
}

impl WaterSettings {
    pub fn resolved(&self) -> ResolvedWater {
        use crate::engine_defaults as ed;
        ResolvedWater {
            damage: self.damage.unwrap_or(ed::WATER_DAMAGE),
            absorb: self.absorb.unwrap_or(ed::WATER_ABSORB),
            base_color: self.base_color.unwrap_or(ed::WATER_BASE_COLOR),
            min_color: self.min_color.unwrap_or(ed::WATER_MIN_COLOR),
            surface_color: self.surface_color.unwrap_or(ed::WATER_SURFACE_COLOR),
            surface_alpha: self.surface_alpha.unwrap_or(ed::WATER_SURFACE_ALPHA),
            diffuse_color: self.diffuse_color.unwrap_or(ed::WATER_DIFFUSE_COLOR),
            specular_color: self.specular_color.unwrap_or(ed::WATER_SPECULAR_COLOR),
            ambient_factor: self.ambient_factor.unwrap_or(ed::WATER_AMBIENT_FACTOR),
            diffuse_factor: self.diffuse_factor.unwrap_or(ed::WATER_DIFFUSE_FACTOR),
            specular_factor: self.specular_factor.unwrap_or(ed::WATER_SPECULAR_FACTOR),
            specular_power: self.specular_power.unwrap_or(ed::WATER_SPECULAR_POWER),
            fresnel_min: self.fresnel_min.unwrap_or(ed::WATER_FRESNEL_MIN),
            fresnel_max: self.fresnel_max.unwrap_or(ed::WATER_FRESNEL_MAX),
            fresnel_power: self.fresnel_power.unwrap_or(ed::WATER_FRESNEL_POWER),
            reflection_distortion: self
                .reflection_distortion
                .unwrap_or(ed::WATER_REFLECTION_DISTORTION),
            perlin_amplitude: self.perlin_amplitude.unwrap_or(ed::WATER_PERLIN_AMPLITUDE),
            blur_base: self.blur_base.unwrap_or(ed::WATER_BLUR_BASE),
            blur_exponent: self.blur_exponent.unwrap_or(ed::WATER_BLUR_EXPONENT),
            caustics_resolution: self
                .caustics_resolution
                .unwrap_or(ed::WATER_CAUSTICS_RESOLUTION),
            caustics_strength: self
                .caustics_strength
                .unwrap_or(ed::WATER_CAUSTICS_STRENGTH),
            // Shore-foam params don't have engine constants documented
            // here yet -- treat unset as zero so the foam pass stays
            // dormant for maps that didn't enable it.
            wave_offset_factor: self.wave_offset_factor.unwrap_or(0.0),
            wave_foam_distortion: self.wave_foam_distortion.unwrap_or(0.05),
            wave_foam_intensity: self.wave_foam_intensity.unwrap_or(0.5),
            wave_length: self.wave_length.unwrap_or(0.15),
            force_rendering: self.force_rendering.unwrap_or(false),
            has_water_plane: self.has_water_plane.unwrap_or(false),
            num_tiles: self.num_tiles.unwrap_or(4),
            perlin_start_freq: self.perlin_start_freq.unwrap_or(8.0),
            perlin_lacunarity: self.perlin_lacunarity.unwrap_or(3.0),
            plane_color: self.plane_color.unwrap_or([0.0, 0.5, 0.5]),
            repeat_x: self.repeat_x.unwrap_or(0.0),
            repeat_y: self.repeat_y.unwrap_or(0.0),
            shore_waves: self.shore_waves.unwrap_or(true),
            normal_texture: self.normal_texture.clone().unwrap_or_default(),
        }
    }
}

/// Which fluid (if any) covers the map's water plane. Mutually
/// exclusive at the engine level: bar-game's `map_lava` gadget calls
/// `Spring.SetDrawWater(false)` when lava is active, so only one
/// rendering pass ever draws. BME models the choice explicitly here
/// because the gadget's trigger is a chain of inputs (map archive
/// `mapconfig/lava.lua`, game-side `LavaMaps/<MapName>.lua` catalog
/// match, `mapinfo.water.damage > 0` fallback, mod option) -- the
/// underlying mapinfo damage field is NOT a reliable indicator on
/// its own.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FluidMode {
    /// Engine `BumpWater` pipeline -- the only mode that uses the
    /// `water = { ... }` table from mapinfo for rendering.
    #[default]
    Water,
    /// bar-game `map_lava` gadget overlay -- uses the `lava = { ... }`
    /// block on `MapSettings` for textured-emissive rendering. The
    /// engine water draw is disabled while lava is active.
    Lava,
}

/// Lava widget configuration -- mirrors the keys read by
/// bar-game's `modules/lava.lua` (with per-map overrides authored in
/// `mapconfig/lava.lua` inside the map archive, or in game-side
/// `common/configs/LavaMaps/<MapName>.lua`). Visual fields drive the
/// renderer's `LavaParamsUniform`; gameplay fields (damage, tide)
/// are persisted for export but unused by the editor preview.
///
/// Every field is `Option<T>` so an unset key on import stays unset
/// on export (round-trip parity), and the renderer falls through to
/// the engine defaults in `engine_defaults.rs::LAVA_*` whenever a
/// recipe leaves a field empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LavaSettings {
    /// `lava.diffuseEmitTex` -- RGB = base colour, A = emission /
    /// heat mask. Defaults to `lava2_diffuseemit.dds` (CC0, bundled).
    pub diffuse_emit_tex: Option<String>,
    /// `lava.normalHeightTex` -- RGB = tangent normal, A = parallax
    /// height. Defaults to `lava2_normalheight.dds`.
    pub normal_height_tex: Option<String>,
    /// Lava plane Y in elmos. Independent of `water.level`.
    pub level: Option<f32>,
    /// `lava.damage` -- per-second damage applied to units in lava.
    /// Distinct from `water.damage` (which the engine reads for the
    /// water-damage fallback trigger path).
    pub damage: Option<f32>,
    /// Tile count across the map's longer axis.
    pub uv_scale: Option<f32>,
    /// Final RGB multiplier on the lava colour (engine's
    /// `SWIZZLECOLORS`). The dominant per-map visual lever -- this
    /// is what makes some lava maps acid-green, others purple, etc.
    pub color_correction: Option<[f32; 3]>,
    /// RGB colour added at the coastline ramp (engine `COASTCOLOR`).
    pub coast_color: Option<[f32; 3]>,
    /// Coast ramp width in elmos.
    pub coast_width: Option<f32>,
    /// Extra brightness added at the coast (engine's
    /// `coastLightBoost`; consumed by the fog-light pass when wired).
    pub coast_light_boost: Option<f32>,
    /// Texture swirl animation frequency.
    pub swirl_freq: Option<f32>,
    /// Texture swirl animation amplitude.
    pub swirl_amp: Option<f32>,
    /// Sun specular exponent for the lava surface.
    pub specular_exp: Option<f32>,
    /// Peak brightness of specular highlights.
    pub specular_strength: Option<f32>,
    /// Out-of-LOS darkening (gameplay -- unused by BME preview).
    pub los_darkness: Option<f32>,
    /// Shadowed-fragment brightness floor.
    pub shadow_strength: Option<f32>,
    /// Parallax-mapping depth in shader units (>0 enables the
    /// pre-displaced sample path; BME's Core port skips it).
    pub parallax_depth: Option<f32>,
    /// Parallax centre, 0..1.
    pub parallax_offset: Option<f32>,
    /// Whether the additive height-fog pass renders above the lava.
    pub fog_enabled: Option<bool>,
    pub fog_color: Option<[f32; 3]>,
    pub fog_factor: Option<f32>,
    pub fog_height: Option<f32>,
    pub fog_above: Option<f32>,
    pub fog_distortion: Option<f32>,
    /// Tide animation amplitude (elmos).
    pub tide_amplitude: Option<f32>,
    /// Tide animation period (seconds).
    pub tide_period: Option<f32>,
}

/// Resolved counterpart of [`LavaSettings`] -- every field replaced
/// with its effective value (recipe value -> engine default in
/// `engine_defaults::LAVA_*`).
#[derive(Debug, Clone)]
pub struct ResolvedLava {
    pub diffuse_emit_tex: String,
    pub normal_height_tex: String,
    pub level: f32,
    pub damage: f32,
    pub uv_scale: f32,
    pub color_correction: [f32; 3],
    pub coast_color: [f32; 3],
    pub coast_width: f32,
    pub coast_light_boost: f32,
    pub swirl_freq: f32,
    pub swirl_amp: f32,
    pub specular_exp: f32,
    pub specular_strength: f32,
    pub los_darkness: f32,
    pub shadow_strength: f32,
    pub parallax_depth: f32,
    pub parallax_offset: f32,
    pub fog_enabled: bool,
    pub fog_color: [f32; 3],
    pub fog_factor: f32,
    pub fog_height: f32,
    pub fog_above: f32,
    pub fog_distortion: f32,
    pub tide_amplitude: f32,
    pub tide_period: f32,
}

impl LavaSettings {
    pub fn resolved(&self) -> ResolvedLava {
        use crate::engine_defaults as ed;
        ResolvedLava {
            diffuse_emit_tex: self
                .diffuse_emit_tex
                .clone()
                .unwrap_or_else(|| ed::LAVA_DIFFUSE_EMIT_TEX.to_string()),
            normal_height_tex: self
                .normal_height_tex
                .clone()
                .unwrap_or_else(|| ed::LAVA_NORMAL_HEIGHT_TEX.to_string()),
            level: self.level.unwrap_or(ed::LAVA_LEVEL),
            damage: self.damage.unwrap_or(ed::LAVA_DAMAGE),
            uv_scale: self.uv_scale.unwrap_or(ed::LAVA_UV_SCALE),
            color_correction: self.color_correction.unwrap_or(ed::LAVA_COLOR_CORRECTION),
            coast_color: self.coast_color.unwrap_or(ed::LAVA_COAST_COLOR),
            coast_width: self.coast_width.unwrap_or(ed::LAVA_COAST_WIDTH),
            coast_light_boost: self.coast_light_boost.unwrap_or(ed::LAVA_COAST_LIGHT_BOOST),
            swirl_freq: self.swirl_freq.unwrap_or(ed::LAVA_SWIRL_FREQ),
            swirl_amp: self.swirl_amp.unwrap_or(ed::LAVA_SWIRL_AMP),
            specular_exp: self.specular_exp.unwrap_or(ed::LAVA_SPECULAR_EXP),
            specular_strength: self.specular_strength.unwrap_or(ed::LAVA_SPECULAR_STRENGTH),
            los_darkness: self.los_darkness.unwrap_or(ed::LAVA_LOS_DARKNESS),
            shadow_strength: self.shadow_strength.unwrap_or(ed::LAVA_SHADOW_STRENGTH),
            parallax_depth: self.parallax_depth.unwrap_or(ed::LAVA_PARALLAX_DEPTH),
            parallax_offset: self.parallax_offset.unwrap_or(ed::LAVA_PARALLAX_OFFSET),
            fog_enabled: self.fog_enabled.unwrap_or(ed::LAVA_FOG_ENABLED),
            fog_color: self.fog_color.unwrap_or(ed::LAVA_FOG_COLOR),
            fog_factor: self.fog_factor.unwrap_or(ed::LAVA_FOG_FACTOR),
            fog_height: self.fog_height.unwrap_or(ed::LAVA_FOG_HEIGHT),
            fog_above: self.fog_above.unwrap_or(ed::LAVA_FOG_ABOVE),
            fog_distortion: self.fog_distortion.unwrap_or(ed::LAVA_FOG_DISTORTION),
            tide_amplitude: self.tide_amplitude.unwrap_or(ed::LAVA_TIDE_AMPLITUDE),
            tide_period: self.tide_period.unwrap_or(ed::LAVA_TIDE_PERIOD),
        }
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            // 8x8 BAR squares: (8 * 64 + 1) = 513 px, 4096 x 4096 elmos.
            width: 513,
            height: 513,
            map_settings: MapSettings::default(),
        }
    }
}

impl Recipe {
    /// Load a recipe from a JSON file.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read {}", path.display()))?;
        Self::from_json(&content)
    }

    /// Parse a recipe from a JSON string.
    pub fn from_json(json: &str) -> Result<Self> {
        let recipe: Self = serde_json::from_str(json).context("Failed to parse recipe JSON")?;
        recipe.validate()?;
        Ok(recipe)
    }

    /// Serialize this recipe to a pretty-printed JSON string.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).context("Failed to serialize recipe")
    }

    /// Save this recipe to a file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = self.to_json()?;
        std::fs::write(path, json).with_context(|| format!("Cannot write {}", path.display()))?;
        Ok(())
    }

    /// Validate the recipe is well-formed.
    pub fn validate(&self) -> Result<()> {
        if self.nodes.is_empty() {
            bail!("Recipe has no nodes");
        }

        // Check for duplicate keys
        let mut keys: HashMap<&str, usize> = HashMap::new();
        for (i, node) in self.nodes.iter().enumerate() {
            if node.key.is_empty() {
                bail!("Node at index {} has an empty key", i);
            }
            if let Some(prev) = keys.insert(&node.key, i) {
                bail!(
                    "Duplicate node key '{}' at indices {} and {}",
                    node.key,
                    prev,
                    i
                );
            }
        }

        // Validate connections reference existing nodes and ports
        for conn in &self.connections {
            Self::validate_port_ref(&conn.from, &keys, "from")?;
            Self::validate_port_ref(&conn.to, &keys, "to")?;
        }

        // Validate output dimensions
        if self.output.width == 0 || self.output.height == 0 {
            bail!("Output dimensions must be > 0");
        }

        // Validate every node's params against its schema. Type
        // mismatches are hard errors — a hand-edited recipe with the
        // wrong type would otherwise silently substitute a default
        // and produce wrong-but-not-broken evaluations. Unknown keys
        // are tolerated for now (they fall through to evaluation
        // unread); see `param_spec` module docs for the rationale.
        for (i, node) in self.nodes.iter().enumerate() {
            for err in bar_graph::validate_node_params(&node.node_type, &node.params) {
                if let bar_graph::ParamError::TypeMismatch {
                    key, expected, got, ..
                } = err
                {
                    bail!(
                        "Node {}/{} ({:?}): param `{}` is {:?}, expected {:?}",
                        i,
                        node.key,
                        node.node_type,
                        key,
                        got,
                        expected,
                    );
                }
            }
        }

        Ok(())
    }

    fn validate_port_ref(
        port_ref: &str,
        keys: &HashMap<&str, usize>,
        direction: &str,
    ) -> Result<()> {
        let parts: Vec<&str> = port_ref.splitn(2, '.').collect();
        if parts.len() != 2 {
            bail!(
                "Invalid {} port reference '{}': expected 'node_key.port_name'",
                direction,
                port_ref
            );
        }
        let node_key = parts[0];
        let port_name = parts[1];

        if !keys.contains_key(node_key) {
            bail!(
                "Connection {} references unknown node '{}'",
                direction,
                node_key
            );
        }
        if port_name.is_empty() {
            bail!(
                "Connection {} has empty port name for node '{}'",
                direction,
                node_key
            );
        }
        Ok(())
    }

    /// Build a `GraphEngine` from this recipe by constructing nodes and connections
    /// through the validated APIs.
    /// Normalized position of world height 0 within the map's height range (the
    /// waterline). 0 when the terrain sits entirely at/above sea level. Drives
    /// AutoTexture's water/beach bands so the texture matches where water is.
    pub fn sea_level(&self) -> f32 {
        let ms = self.output.map_settings.resolved();
        ((0.0f32 - ms.min_height) / (ms.max_height - ms.min_height).abs().max(1.0)).clamp(0.0, 1.0)
    }

    pub fn build_graph(&self) -> Result<GraphEngine> {
        let mut graph = GraphEngine::new();
        let mut key_to_id: HashMap<&str, NodeId> = HashMap::new();

        // Waterline stamped onto AutoTexture nodes so its water/beach bands sit
        // at the actual sea level instead of a fixed bottom fraction.
        let sea_level = self.sea_level();

        // Add nodes
        for recipe_node in &self.nodes {
            let label = if recipe_node.label.is_empty() {
                &recipe_node.key
            } else {
                &recipe_node.label
            };
            // Node::new pre-populates `params` with the type's default
            // values; recipe-specified params merge on top so explicit
            // values win and missing keys fall back to sensible defaults.
            let mut node = Node::new(NodeId(0), recipe_node.node_type.clone(), label);
            for (k, v) in recipe_node.params.iter() {
                node.params.insert(k.clone(), v.clone());
            }
            if node.node_type == NodeType::AutoTexture {
                node.params
                    .insert("sea_level".to_string(), ParamValue::Float(sea_level));
            }
            if node.node_type == NodeType::TextureWeightmap {
                if let Some(ParamValue::UInt(lc)) = node.params.get("layer_count") {
                    node.resize_texture_weightmap_ports(*lc);
                }
            }
            if matches!(
                node.node_type,
                NodeType::SubgraphInput | NodeType::SubgraphOutput
            ) {
                node.sync_subgraph_io_kind();
            }
            let id = graph.add_node(node);
            key_to_id.insert(&recipe_node.key, id);
        }

        // Add connections
        for conn in &self.connections {
            let (from_key, from_port) = parse_port_ref(&conn.from)?;
            let (to_key, to_port) = parse_port_ref(&conn.to)?;

            let from_id = key_to_id
                .get(from_key)
                .with_context(|| format!("Unknown node key in connection: '{}'", from_key))?;
            let to_id = key_to_id
                .get(to_key)
                .with_context(|| format!("Unknown node key in connection: '{}'", to_key))?;

            graph
                .connect(
                    PortId {
                        node_id: *from_id,
                        port_name: from_port.to_string(),
                    },
                    PortId {
                        node_id: *to_id,
                        port_name: to_port.to_string(),
                    },
                )
                .with_context(|| {
                    format!(
                        "Failed to connect {}.{} → {}.{}",
                        from_key, from_port, to_key, to_port
                    )
                })?;
        }

        // Verify no cycles
        graph
            .topological_sort()
            .context("Recipe graph contains a cycle")?;

        Ok(graph)
    }

    /// Generate a sample recipe demonstrating the format.
    pub fn sample() -> Self {
        Self {
            name: "Sample Terrain".to_string(),
            shortname: None,
            description: "A basic ridged noise terrain with blur smoothing".to_string(),
            author: None,
            version: None,
            tip: None,
            depend: vec!["Map Helper v1".to_string()],
            nodes: vec![
                RecipeNode {
                    key: "base_terrain".to_string(),
                    node_type: NodeType::RidgedNoise,
                    label: "Base Terrain".to_string(),
                    params: HashMap::from([
                        ("frequency".to_string(), ParamValue::Float(3.0)),
                        ("octaves".to_string(), ParamValue::UInt(6)),
                        ("lacunarity".to_string(), ParamValue::Float(2.0)),
                        ("seed".to_string(), ParamValue::UInt(42)),
                    ]),
                },
                RecipeNode {
                    key: "detail".to_string(),
                    node_type: NodeType::PerlinNoise,
                    label: "Detail Noise".to_string(),
                    params: HashMap::from([
                        ("frequency".to_string(), ParamValue::Float(8.0)),
                        ("octaves".to_string(), ParamValue::UInt(4)),
                        ("persistence".to_string(), ParamValue::Float(0.4)),
                        ("seed".to_string(), ParamValue::UInt(7)),
                    ]),
                },
                RecipeNode {
                    key: "blend".to_string(),
                    node_type: NodeType::Blend,
                    label: "Blend".to_string(),
                    params: HashMap::from([("factor".to_string(), ParamValue::Float(0.3))]),
                },
                RecipeNode {
                    key: "smooth".to_string(),
                    node_type: NodeType::Blur,
                    label: "Smooth".to_string(),
                    params: HashMap::from([("radius".to_string(), ParamValue::Float(1.5))]),
                },
                RecipeNode {
                    key: "output".to_string(),
                    node_type: NodeType::FinalComposition,
                    label: "Export".to_string(),
                    params: HashMap::new(),
                },
            ],
            connections: vec![
                RecipeConnection {
                    from: "base_terrain.output".to_string(),
                    to: "blend.a".to_string(),
                },
                RecipeConnection {
                    from: "detail.output".to_string(),
                    to: "blend.b".to_string(),
                },
                RecipeConnection {
                    from: "blend.output".to_string(),
                    to: "smooth.input".to_string(),
                },
                RecipeConnection {
                    from: "smooth.output".to_string(),
                    to: "output.heightmap".to_string(),
                },
            ],
            output: OutputConfig {
                width: 257,
                height: 257,
                map_settings: MapSettings::default(),
            },
            features: Vec::new(),
        }
    }
}

fn parse_port_ref(s: &str) -> Result<(&str, &str)> {
    let parts: Vec<&str> = s.splitn(2, '.').collect();
    if parts.len() != 2 {
        bail!("Invalid port reference '{}': expected 'node.port'", s);
    }
    Ok((parts[0], parts[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_recipe_validates() {
        let recipe = Recipe::sample();
        recipe.validate().unwrap();
    }

    #[test]
    fn test_sample_recipe_roundtrip_json() {
        let recipe = Recipe::sample();
        let json = recipe.to_json().unwrap();
        let parsed = Recipe::from_json(&json).unwrap();
        assert_eq!(parsed.name, "Sample Terrain");
        assert_eq!(parsed.nodes.len(), 5);
        assert_eq!(parsed.connections.len(), 4);
    }

    #[test]
    fn test_sample_recipe_builds_graph() {
        let recipe = Recipe::sample();
        let graph = recipe.build_graph().unwrap();
        assert_eq!(graph.nodes().len(), 5);
        assert_eq!(graph.connections().len(), 4);
        // Topological sort should work (no cycles)
        let order = graph.topological_sort().unwrap();
        assert_eq!(order.len(), 5);
    }

    #[test]
    fn sea_level_tracks_height_range() {
        let mut r = Recipe::sample();
        // world 0 sits a quarter up the [-100, 300] range.
        r.output.map_settings.min_height = Some(-100.0);
        r.output.map_settings.max_height = Some(300.0);
        assert!((r.sea_level() - 0.25).abs() < 1e-3, "got {}", r.sea_level());
        // Terrain entirely at/above sea level -> no water.
        r.output.map_settings.min_height = Some(0.0);
        assert_eq!(r.sea_level(), 0.0);
    }

    #[test]
    fn test_invalid_recipe_zero_dimensions() {
        let recipe = Recipe {
            name: "Bad".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            tip: None,
            depend: vec!["Map Helper v1".to_string()],
            nodes: vec![RecipeNode {
                key: "noise".to_string(),
                node_type: NodeType::PerlinNoise,
                label: String::new(),
                params: HashMap::new(),
            }],
            connections: vec![],
            output: OutputConfig {
                width: 0,
                height: 0,
                map_settings: MapSettings::default(),
            },
            features: Vec::new(),
        };
        assert!(recipe.validate().is_err());
    }

    #[test]
    fn test_invalid_recipe_bad_connection() {
        let recipe = Recipe {
            name: "Bad".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            tip: None,
            depend: vec!["Map Helper v1".to_string()],
            nodes: vec![
                RecipeNode {
                    key: "noise".to_string(),
                    node_type: NodeType::PerlinNoise,
                    label: String::new(),
                    params: HashMap::new(),
                },
                RecipeNode {
                    key: "out".to_string(),
                    node_type: NodeType::FinalComposition,
                    label: String::new(),
                    params: HashMap::new(),
                },
            ],
            connections: vec![RecipeConnection {
                from: "nonexistent.output".to_string(),
                to: "out.heightmap".to_string(),
            }],
            output: OutputConfig::default(),
            features: Vec::new(),
        };
        assert!(recipe.validate().is_err());
    }

    #[test]
    fn test_invalid_recipe_duplicate_keys() {
        let recipe = Recipe {
            name: "Bad".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            tip: None,
            depend: vec!["Map Helper v1".to_string()],
            nodes: vec![
                RecipeNode {
                    key: "dupe".to_string(),
                    node_type: NodeType::PerlinNoise,
                    label: String::new(),
                    params: HashMap::new(),
                },
                RecipeNode {
                    key: "dupe".to_string(),
                    node_type: NodeType::FinalComposition,
                    label: String::new(),
                    params: HashMap::new(),
                },
            ],
            connections: vec![],
            output: OutputConfig::default(),
            features: Vec::new(),
        };
        assert!(recipe.validate().is_err());
    }

    #[test]
    fn recipe_with_wrong_typed_param_is_rejected() {
        // A hand-edited recipe with `Blur.radius` typed as a String
        // (it's `Float`) used to silently fall back to the default
        // and produce wrong-but-not-broken evaluations. Now it
        // refuses to load with a clear error citing the node + key.
        // `ParamValue` serialises with external tagging
        // (`{"Float": 1.5}` etc.), so a String-typed value is
        // `{"String": "..."}` — that *parses* fine but fails the
        // schema validator we wired into `Recipe::validate`.
        let json = r#"{
            "name": "typo",
            "nodes": [
                {"key": "n", "type": "Blur",
                 "params": {"radius": {"String": "definitely_not_a_float"}}}
            ],
            "connections": [],
            "output": {"width": 256, "height": 256}
        }"#;
        let err = Recipe::from_json(json).expect_err("type mismatch must error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("radius") && msg.contains("Blur"),
            "error should cite the offending node + key; got: {msg}",
        );
    }

    #[test]
    fn recipe_with_unknown_param_loads_anyway() {
        // Unknown keys are tolerated so older / hand-edited recipes
        // with deprecated params still load. See `param_spec` module
        // docs for the per-node strictness story.
        let json = r#"{
            "name": "extra",
            "nodes": [
                {"key": "n", "type": "Blur",
                 "params": {
                    "radius": {"Float": 1.5},
                    "totally_legacy_param": {"UInt": 42}
                 }}
            ],
            "connections": [],
            "output": {"width": 256, "height": 256}
        }"#;
        Recipe::from_json(json).expect("unknown keys must not block load");
    }

    // Recipe round-trip scenarios: these mirror the three ways a user
    // creates or opens a project in the editor.

    /// A manually-built recipe (no preset, no SD7) round-trips through
    /// JSON and can have its graph evaluated without errors.
    #[test]
    fn manual_project_recipe_roundtrip() {
        let recipe = Recipe {
            name: "Manual test".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            tip: None,
            depend: vec!["Map Helper v1".to_string()],
            nodes: vec![
                RecipeNode {
                    key: "noise".to_string(),
                    node_type: NodeType::PerlinNoise,
                    label: "Noise".to_string(),
                    params: HashMap::new(),
                },
                RecipeNode {
                    key: "out".to_string(),
                    node_type: NodeType::FinalComposition,
                    label: String::new(),
                    params: HashMap::new(),
                },
            ],
            connections: vec![RecipeConnection {
                from: "noise.output".to_string(),
                to: "out.heightmap".to_string(),
            }],
            output: OutputConfig {
                width: 513,
                height: 513,
                map_settings: MapSettings::default(),
            },
            features: Vec::new(),
        };
        let json = recipe.to_json().unwrap();
        let loaded = Recipe::from_json(&json).unwrap();
        let graph = loaded.build_graph().unwrap();
        assert_eq!(graph.nodes().len(), 2);
        assert_eq!(graph.connections().len(), 1);
    }

    /// Preset project with semantic kind strings (display labels rather than
    /// PortKind names) round-trips and builds without IncompatiblePorts.
    #[test]
    fn preset_project_semantic_kind_roundtrip() {
        // SubgraphOutput kind params written as display labels ("Texture",
        // "Output", "Slope") must survive load via the io_value_bypass path.
        let make_subout = |key: &str, kind: &str| RecipeNode {
            key: key.to_string(),
            node_type: NodeType::SubgraphOutput,
            label: String::new(),
            params: {
                let mut p = HashMap::new();
                p.insert("kind".to_string(), ParamValue::String(kind.to_string()));
                p
            },
        };
        let recipe = Recipe {
            name: "Alpine 8x8".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            tip: None,
            depend: vec!["Map Helper v1".to_string()],
            nodes: vec![
                make_subout("sub_terrain", "Output"), // was "Heightmap" before recompute ran
                make_subout("sub_texture", "Texture"), // was "Color"
                make_subout("sub_slope", "Slope"),    // was "Heightmap"
                RecipeNode {
                    key: "out".to_string(),
                    node_type: NodeType::FinalComposition,
                    label: String::new(),
                    params: HashMap::new(),
                },
            ],
            connections: vec![
                RecipeConnection {
                    from: "sub_terrain.value".to_string(),
                    to: "out.heightmap".to_string(),
                },
                RecipeConnection {
                    from: "sub_texture.value".to_string(),
                    to: "out.texture".to_string(),
                },
                RecipeConnection {
                    from: "sub_slope.value".to_string(),
                    to: "out.metalmap".to_string(),
                },
            ],
            // Simulates the user changing width/height before saving.
            output: OutputConfig {
                width: 1025,
                height: 1025,
                map_settings: MapSettings::default(),
            },
            features: Vec::new(),
        };
        let json = recipe.to_json().unwrap();
        let loaded = Recipe::from_json(&json).unwrap();
        // Verify size change survives the round-trip.
        assert_eq!(loaded.output.width, 1025);
        assert_eq!(loaded.output.height, 1025);
        // Must not error with IncompatiblePorts despite semantic kind strings.
        let graph = loaded.build_graph().unwrap();
        assert_eq!(graph.connections().len(), 3);
    }

    /// A PaintedHeightmap -> Bundler pipeline round-trips through JSON and
    /// builds its graph cleanly.
    #[test]
    fn painted_heightmap_to_bundler_roundtrip() {
        let recipe = Recipe {
            name: "Import test".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            tip: None,
            depend: vec!["Map Helper v1".to_string()],
            nodes: vec![
                RecipeNode {
                    key: "hm".to_string(),
                    node_type: NodeType::PaintedHeightmap,
                    label: "Heightmap".to_string(),
                    params: {
                        let mut p = HashMap::new();
                        p.insert("data".to_string(), ParamValue::String(String::new()));
                        p.insert("resolution".to_string(), ParamValue::UInt(512));
                        p
                    },
                },
                RecipeNode {
                    key: "out".to_string(),
                    node_type: NodeType::FinalComposition,
                    label: String::new(),
                    params: HashMap::new(),
                },
            ],
            connections: vec![RecipeConnection {
                from: "hm.output".to_string(),
                to: "out.heightmap".to_string(),
            }],
            output: OutputConfig {
                width: 513,
                height: 513,
                map_settings: MapSettings::default(),
            },
            features: Vec::new(),
        };
        let json = recipe.to_json().unwrap();
        let loaded = Recipe::from_json(&json).unwrap();
        let graph = loaded.build_graph().unwrap();
        assert_eq!(graph.nodes().len(), 2);
        assert_eq!(graph.connections().len(), 1);
    }
}

//! Single source of truth for the engine values BME falls through to
//! when a mapinfo field is unset. Each constant is cited to the Recoil
//! engine source so updating drift means changing exactly one line
//! here. The recipe carries `Option<T>` for every modelled field;
//! `Some(v)` is the user-explicit or source-mapinfo value, `None`
//! resolves to the constant below at render and emit time.
//!
//! BME never copies these values into the bundled mapinfo -- the
//! engine already knows them. So drift between this table and the
//! engine only ever affects BME's preview, never the shipped map.

// ── Atmosphere (mapinfo `atmosphere = { ... }`) ───────────────────
// Citations: `bar-recoil/rts/Map/MapInfo.cpp::ReadAtmosphere` for
// engine defaults; `mapgenerator/mapinfo_template.lua` for the sky
// fields BAR ships in freshly generated maps.
pub const ATMOSPHERE_MIN_WIND: f32 = 5.0;
pub const ATMOSPHERE_MAX_WIND: f32 = 25.0;
pub const ATMOSPHERE_FOG_START: f32 = 0.1;
pub const ATMOSPHERE_FOG_END: f32 = 1.0;
pub const ATMOSPHERE_FOG_COLOR: [f32; 3] = [0.7, 0.7, 0.8];
pub const ATMOSPHERE_SUN_COLOR: [f32; 3] = [1.0, 1.0, 1.0];
pub const ATMOSPHERE_SKY_COLOR: [f32; 3] = [0.1, 0.15, 0.7];
pub const ATMOSPHERE_SKY_DIR: [f32; 3] = [0.0, 0.0, -1.0];
pub const ATMOSPHERE_CLOUD_DENSITY: f32 = 0.5;
pub const ATMOSPHERE_CLOUD_COLOR: [f32; 3] = [1.0, 1.0, 1.0];

// ── Lighting (mapinfo `lighting = { ... }`) ───────────────────────
// Citations: `bar-recoil/rts/Map/MapInfo.cpp::ReadLight`. Sun direction
// engine default is normalised at read-time so the raw store-direction
// matters less than the normalised result; we follow the engine's
// pre-normalisation source value here.
pub const LIGHTING_SUN_DIR: [f32; 3] = [0.0, 1.0, 2.0];
pub const LIGHTING_SUN_INTENSITY: f32 = 1.0;
pub const LIGHTING_GROUND_AMBIENT: [f32; 3] = [0.5, 0.5, 0.5];
pub const LIGHTING_GROUND_DIFFUSE: [f32; 3] = [0.5, 0.5, 0.5];
pub const LIGHTING_GROUND_SPECULAR: [f32; 3] = [0.1, 0.1, 0.1];
pub const LIGHTING_SPEC_EXPONENT: f32 = 100.0;
pub const LIGHTING_GROUND_SHADOW_DENSITY: f32 = 0.8;

// ── Water (mapinfo `water = { ... }`) ─────────────────────────────
// Citations: `bar-recoil/rts/Map/MapInfo.cpp::ReadWater` and the
// `BumpWater` shader uniforms (`BumpWaterFS.glsl`). Values BAR ships
// when a map omits the corresponding key.
pub const WATER_DAMAGE: f32 = 0.0;
pub const WATER_ABSORB: [f32; 3] = [0.0, 0.0, 0.0];
pub const WATER_BASE_COLOR: [f32; 3] = [0.0, 0.5, 0.5];
pub const WATER_MIN_COLOR: [f32; 3] = [0.0, 0.5, 0.5];
pub const WATER_SURFACE_COLOR: [f32; 3] = [0.75, 0.8, 0.85];
pub const WATER_SURFACE_ALPHA: f32 = 0.55;
pub const WATER_DIFFUSE_COLOR: [f32; 3] = [1.0, 1.0, 1.0];
pub const WATER_SPECULAR_COLOR: [f32; 3] = [0.5, 0.5, 0.5];
pub const WATER_AMBIENT_FACTOR: f32 = 1.0;
pub const WATER_DIFFUSE_FACTOR: f32 = 1.0;
pub const WATER_SPECULAR_FACTOR: f32 = 1.0;
pub const WATER_SPECULAR_POWER: f32 = 20.0;
pub const WATER_FRESNEL_MIN: f32 = 0.2;
pub const WATER_FRESNEL_MAX: f32 = 0.8;
pub const WATER_FRESNEL_POWER: f32 = 4.0;
pub const WATER_REFLECTION_DISTORTION: f32 = 1.0;
pub const WATER_PERLIN_AMPLITUDE: f32 = 0.9;
pub const WATER_BLUR_BASE: f32 = 2.0;
pub const WATER_BLUR_EXPONENT: f32 = 1.5;
pub const WATER_CAUSTICS_RESOLUTION: f32 = 75.0;
pub const WATER_CAUSTICS_STRENGTH: f32 = 0.08;

// ── Grass widget (mapinfo `custom.grassConfig`) ───────────────────
// Citations: `bar-game/luaui/Widgets/map_grass_gl4.lua` lines 87-110
// (grassConfig defaults) and 93-110 (grassShaderParams defaults).
pub const GRASS_MAX_SIZE: f32 = 1.7;
pub const GRASS_MIN_SIZE: f32 = 0.3;
pub const GRASS_PATCH_RESOLUTION: u32 = 32;
pub const GRASS_PATCH_PLACEMENT_JITTER: f32 = 0.66;
pub const GRASS_MAP_COLOR_FACTOR: f32 = 0.6;
pub const GRASS_MAP_COLOR_BASE: f32 = 1.0;
pub const GRASS_ALPHA_THRESHOLD: f32 = 0.1;
pub const GRASS_SHADOW_FACTOR: f32 = 0.25;
pub const GRASS_BRIGHTNESS: f32 = 1.0;
pub const GRASS_FADE_START: f32 = 5000.0;
pub const GRASS_FADE_END: f32 = 8000.0;
pub const GRASS_WIND_STRENGTH: f32 = 0.1;
pub const GRASS_WIND_SCALE: f32 = 0.33;
pub const GRASS_WIND_SAMPLE_SCALE: f32 = 0.001;
pub const GRASS_WIND_MULT: f32 = 4.5;

// ── MapSettings physics scalars (mapinfo top-level) ───────────────
// Citations: `bar-recoil/rts/Map/MapInfo.cpp` for each.
pub const MAP_HARDNESS: u32 = 100;
pub const MAP_GRAVITY: f32 = 130.0;
pub const MAP_TIDAL_STRENGTH: f32 = 0.0;
pub const MAP_MAX_METAL: f32 = 0.02;
pub const MAP_EXTRACTOR_RADIUS: f32 = 500.0;
pub const MAP_VOID_WATER: bool = false;
pub const MAP_VOID_GROUND: bool = false;
pub const MAP_NOT_DEFORMABLE: bool = false;
pub const MAP_MIN_HEIGHT: f32 = 0.0;
pub const MAP_MAX_HEIGHT: f32 = 800.0;

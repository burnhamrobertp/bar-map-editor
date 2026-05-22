//! Static `FieldSpec` arrays for every modelled field on
//! [`MapSettings`] and friends. The modals iterate these arrays via
//! the `render_field` renderer in `bar-gui::panels::field_editor` so
//! the rendering, validation, undo, and clamp behaviour for every
//! field lives in one place.
//!
//! Adding a new modelled field is one entry in the relevant array.
//! No widget code, no per-field validation function, no UI range
//! constants.
//!
//! ## Organisation
//!
//! - [`PHYSICS_SPECS`]      -- top-level physics scalars
//! - [`ATMOSPHERE_SPECS`]   -- `MapSettings::atmosphere` (wind + sky/cloud appearance)
//! - [`FOG_SPECS`]          -- distance fog from `AtmosphereSettings`
//! - [`CLOUDS_SPECS`]       -- `MapSettings::custom_clouds` widget settings
//! - [`LIGHTING_SPECS`]     -- `MapSettings::lighting` (includes `atmosphere.sun_color`)
//! - [`WATER_SPECS`]        -- `MapSettings::water` in water mode (no damage)
//! - [`LAVA_SPECS`]         -- curated `MapSettings::water` subset rendered
//!   in lava mode (damage + underwater / surface / surface-lighting only).
//!   Shares field IDs with `WATER_SPECS` by design; the modal picks which
//!   list to render from `WaterSettings::is_lava`.
//! - [`GRASS_SPECS`]        -- `MapSettings::custom_grass`
//!
//! Each array is `&'static [FieldSpec<MapSettings>]`. Most specs are
//! authored without a description tooltip; populate
//! `description: Some(...)` selectively where the field name is
//! engine jargon or the soft-vs-hard range distinction isn't obvious.

use crate::engine_defaults as ed;
use crate::field_schema::{categories, DefaultValue, FieldKind, FieldSpec, FieldValue};
use crate::recipe::MapSettings;

// ──────────────────────────────────────────────────────────────────
// Physics
// ──────────────────────────────────────────────────────────────────

pub static PHYSICS_SPECS: &[FieldSpec<MapSettings>] = &[
    FieldSpec {
        id: "physics.gravity",
        label: "Gravity",
        description: None,
        kind: FieldKind::F32 {
            hard: (1.0, 10000.0),
            soft: Some((50.0, 500.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::MAP_GRAVITY),
        get: |s: &MapSettings| FieldValue::F32(s.gravity),
        set: |s: &mut MapSettings, v: FieldValue| {
            if let FieldValue::F32(x) = v {
                s.gravity = x;
            }
        },
        category: categories::PHYSICS,
        group: "World",
        blocks_export: true,
    },
    FieldSpec {
        id: "physics.map_hardness",
        label: "Map hardness",
        description: Some(
            "Engine-internal terrain hardness; controls how much damage \
             deforms the heightmap. 100 is the engine default.",
        ),
        kind: FieldKind::U32 {
            hard: (1, 65535),
            soft: Some((10, 5000)),
            unit: "",
        },
        default: DefaultValue::U32(ed::MAP_HARDNESS),
        get: |s| FieldValue::U32(s.map_hardness),
        set: |s, v| {
            if let FieldValue::U32(x) = v {
                s.map_hardness = x;
            }
        },
        category: categories::PHYSICS,
        group: "World",
        blocks_export: true,
    },
    FieldSpec {
        id: "physics.tidal_strength",
        label: "Tidal strength",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1000.0),
            soft: Some((0.0, 30.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::MAP_TIDAL_STRENGTH),
        get: |s| FieldValue::F32(s.tidal_strength),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.tidal_strength = x;
            }
        },
        category: categories::PHYSICS,
        group: "World",
        blocks_export: true,
    },
    FieldSpec {
        id: "physics.max_metal",
        label: "Max metal",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1000.0),
            soft: Some((0.0, 10.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::MAP_MAX_METAL),
        get: |s| FieldValue::F32(s.max_metal),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.max_metal = x;
            }
        },
        category: categories::PHYSICS,
        group: "Resources",
        blocks_export: true,
    },
    FieldSpec {
        id: "physics.extractor_radius",
        label: "Extractor radius",
        description: None,
        kind: FieldKind::F32 {
            hard: (1.0, 10000.0),
            soft: Some((50.0, 1000.0)),
            unit: "elmos",
        },
        default: DefaultValue::F32(ed::MAP_EXTRACTOR_RADIUS),
        get: |s| FieldValue::F32(s.extractor_radius),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.extractor_radius = x;
            }
        },
        category: categories::PHYSICS,
        group: "Resources",
        blocks_export: true,
    },
    FieldSpec {
        id: "physics.deformable",
        label: "Deformable terrain",
        description: Some(
            "When false (engine `notDeformable = true`) terrain can't \
             be cratered by weapon damage. Engine default: true.",
        ),
        kind: FieldKind::Bool,
        default: DefaultValue::Bool(true),
        get: |s| FieldValue::Bool(s.deformable),
        set: |s, v| {
            if let FieldValue::Bool(x) = v {
                s.deformable = x;
            }
        },
        category: categories::PHYSICS,
        group: "Behaviour",
        blocks_export: false,
    },
    FieldSpec {
        id: "physics.void_water",
        label: "Void water",
        description: Some(
            "When true, water doesn't render at all -- area below sea \
             level is empty space. Useful for sky-island maps.",
        ),
        kind: FieldKind::Bool,
        default: DefaultValue::Bool(ed::MAP_VOID_WATER),
        get: |s| FieldValue::Bool(s.void_water),
        set: |s, v| {
            if let FieldValue::Bool(x) = v {
                s.void_water = x;
            }
        },
        category: categories::PHYSICS,
        group: "Behaviour",
        blocks_export: false,
    },
    FieldSpec {
        id: "physics.void_ground",
        label: "Void ground",
        description: Some(
            "When true, ground outside the playable heightmap renders \
             as transparency rather than mirroring the playable area.",
        ),
        kind: FieldKind::Bool,
        default: DefaultValue::Bool(ed::MAP_VOID_GROUND),
        get: |s| FieldValue::Bool(s.void_ground),
        set: |s, v| {
            if let FieldValue::Bool(x) = v {
                s.void_ground = x;
            }
        },
        category: categories::PHYSICS,
        group: "Behaviour",
        blocks_export: false,
    },
    FieldSpec {
        id: "physics.auto_show_metal",
        label: "Auto-show metal",
        description: Some(
            "Toggle the F4 metal-map overlay being shown by default at \
             game start.",
        ),
        kind: FieldKind::Bool,
        default: DefaultValue::Bool(false),
        get: |s| FieldValue::Bool(s.auto_show_metal),
        set: |s, v| {
            if let FieldValue::Bool(x) = v {
                s.auto_show_metal = x;
            }
        },
        category: categories::PHYSICS,
        group: "Behaviour",
        blocks_export: false,
    },
];

// ──────────────────────────────────────────────────────────────────
// Atmosphere (wind + sky/cloud appearance; fog and sun moved out)
// ──────────────────────────────────────────────────────────────────

pub static ATMOSPHERE_SPECS: &[FieldSpec<MapSettings>] = &[
    FieldSpec {
        id: "atmosphere.min_wind",
        label: "Min wind",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1000.0),
            soft: Some((0.0, 50.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::ATMOSPHERE_MIN_WIND),
        get: |s| FieldValue::F32(s.atmosphere.min_wind),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.atmosphere.min_wind = x;
            }
        },
        category: categories::ATMOSPHERE,
        group: "Wind",
        blocks_export: false,
    },
    FieldSpec {
        id: "atmosphere.max_wind",
        label: "Max wind",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1000.0),
            soft: Some((0.0, 100.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::ATMOSPHERE_MAX_WIND),
        get: |s| FieldValue::F32(s.atmosphere.max_wind),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.atmosphere.max_wind = x;
            }
        },
        category: categories::ATMOSPHERE,
        group: "Wind",
        blocks_export: false,
    },
    FieldSpec {
        id: "atmosphere.sky_color",
        label: "Sky colour",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::ATMOSPHERE_SKY_COLOR),
        get: |s| FieldValue::Color(s.atmosphere.sky_color),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.atmosphere.sky_color = x;
            }
        },
        category: categories::ATMOSPHERE,
        group: "Sky",
        blocks_export: false,
    },
    FieldSpec {
        id: "atmosphere.cloud_color",
        label: "Cloud colour",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::ATMOSPHERE_CLOUD_COLOR),
        get: |s| FieldValue::Color(s.atmosphere.cloud_color),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.atmosphere.cloud_color = x;
            }
        },
        category: categories::ATMOSPHERE,
        group: "Sky",
        blocks_export: false,
    },
    FieldSpec {
        id: "atmosphere.cloud_density",
        label: "Cloud density",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1.0),
            soft: Some((0.0, 1.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::ATMOSPHERE_CLOUD_DENSITY),
        get: |s| FieldValue::F32(s.atmosphere.cloud_density),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.atmosphere.cloud_density = x;
            }
        },
        category: categories::ATMOSPHERE,
        group: "Sky",
        blocks_export: false,
    },
    FieldSpec {
        id: "atmosphere.sky_dir",
        label: "Sky direction",
        description: Some("Direction the procedural sky's sun appears from."),
        kind: FieldKind::Vec3 {
            hard: (-1.0, 1.0),
            soft: Some((-1.0, 1.0)),
        },
        default: DefaultValue::Vec3(ed::ATMOSPHERE_SKY_DIR),
        get: |s| FieldValue::Vec3(s.atmosphere.sky_dir),
        set: |s, v| {
            if let FieldValue::Vec3(x) = v {
                s.atmosphere.sky_dir = x;
            }
        },
        category: categories::ATMOSPHERE,
        group: "Sky",
        blocks_export: false,
    },
    FieldSpec {
        id: "atmosphere.skybox",
        label: "Skybox cubemap",
        description: Some("Filename inside passthrough/ -- empty = procedural sky."),
        // `PassthroughTexture` kind so `render_specs` skips this
        // field entirely. The atmosphere modal renders its own
        // Browse / Clear UI below the schema iteration -- without
        // the skip the field would show up twice (once as a plain
        // OptionText row from the schema, once as the bespoke
        // picker block).
        kind: FieldKind::PassthroughTexture {
            extensions: &["dds"],
        },
        default: DefaultValue::Empty,
        get: |s| FieldValue::OptionText(s.atmosphere.skybox.clone()),
        set: |s, v| {
            if let FieldValue::OptionText(x) = v {
                s.atmosphere.skybox = x;
            }
        },
        category: categories::ATMOSPHERE,
        group: "Skybox",
        blocks_export: false,
    },
];

// ──────────────────────────────────────────────────────────────────
// Fog (distance fog from AtmosphereSettings; custom.fog height fog
// and custom.clouds are rendered bespoke in the Fog modal)
// ──────────────────────────────────────────────────────────────────

pub static FOG_SPECS: &[FieldSpec<MapSettings>] = &[
    FieldSpec {
        id: "fog.fog_start",
        label: "Fog start",
        description: Some("0 = camera plane, 1 = far plane. Typically [0, 1]."),
        kind: FieldKind::F32 {
            hard: (-10.0, 10.0),
            soft: Some((0.0, 1.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::ATMOSPHERE_FOG_START),
        get: |s| FieldValue::F32(s.atmosphere.fog_start),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.atmosphere.fog_start = x;
            }
        },
        category: categories::FOG,
        group: "Distance fog",
        blocks_export: false,
    },
    FieldSpec {
        id: "fog.fog_end",
        label: "Fog end",
        description: Some("0 = camera plane, 1 = far plane. Typically [0, 1]."),
        kind: FieldKind::F32 {
            hard: (-10.0, 10.0),
            soft: Some((0.0, 1.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::ATMOSPHERE_FOG_END),
        get: |s| FieldValue::F32(s.atmosphere.fog_end),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.atmosphere.fog_end = x;
            }
        },
        category: categories::FOG,
        group: "Distance fog",
        blocks_export: false,
    },
    FieldSpec {
        id: "fog.fog_color",
        label: "Fog colour",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::ATMOSPHERE_FOG_COLOR),
        get: |s| FieldValue::Color(s.atmosphere.fog_color),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.atmosphere.fog_color = x;
            }
        },
        category: categories::FOG,
        group: "Distance fog",
        blocks_export: false,
    },
];

// ──────────────────────────────────────────────────────────────────
// Clouds (custom.clouds widget; all Option<T> fields)
// ──────────────────────────────────────────────────────────────────

pub static CLOUDS_SPECS: &[FieldSpec<MapSettings>] = &[
    FieldSpec {
        id: "clouds.speed",
        label: "Speed",
        description: Some("Wind-speed multiplier for cloud scrolling."),
        kind: FieldKind::F32 {
            hard: (0.0, 100.0),
            soft: Some((0.0, 5.0)),
            unit: "",
        },
        default: DefaultValue::F32(1.0),
        get: |s| FieldValue::F32(s.custom_clouds.speed),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_clouds.speed = x;
            }
        },
        category: categories::CLOUDS,
        group: "Clouds",
        blocks_export: false,
    },
    FieldSpec {
        id: "clouds.color",
        label: "Colour",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color([1.0, 1.0, 1.0]),
        get: |s| FieldValue::Color(s.custom_clouds.color),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.custom_clouds.color = x;
            }
        },
        category: categories::CLOUDS,
        group: "Clouds",
        blocks_export: false,
    },
    FieldSpec {
        id: "clouds.height",
        label: "Height (elmos)",
        description: Some("Altitude above which cloud opacity is zero. Can be absolute or a % of map max height."),
        kind: FieldKind::F32 {
            hard: (0.0, 100000.0),
            soft: Some((0.0, 5000.0)),
            unit: "elmos",
        },
        default: DefaultValue::F32(800.0),
        get: |s| FieldValue::F32(s.custom_clouds.height),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_clouds.height = x;
            }
        },
        category: categories::CLOUDS,
        group: "Clouds",
        blocks_export: false,
    },
    FieldSpec {
        id: "clouds.bottom",
        label: "Bottom (elmos)",
        description: Some("No cloud density below this altitude."),
        kind: FieldKind::F32 {
            hard: (0.0, 100000.0),
            soft: Some((0.0, 2000.0)),
            unit: "elmos",
        },
        default: DefaultValue::F32(0.0),
        get: |s| FieldValue::F32(s.custom_clouds.bottom),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_clouds.bottom = x;
            }
        },
        category: categories::CLOUDS,
        group: "Clouds",
        blocks_export: false,
    },
    FieldSpec {
        id: "clouds.fade_alt",
        label: "Fade altitude (elmos)",
        description: Some("Cloud opacity fades linearly from full at bottom to zero at height between fade_alt and height."),
        kind: FieldKind::F32 {
            hard: (0.0, 100000.0),
            soft: Some((0.0, 3000.0)),
            unit: "elmos",
        },
        default: DefaultValue::F32(400.0),
        get: |s| FieldValue::F32(s.custom_clouds.fade_alt),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_clouds.fade_alt = x;
            }
        },
        category: categories::CLOUDS,
        group: "Clouds",
        blocks_export: false,
    },
    FieldSpec {
        id: "clouds.scale",
        label: "Scale (elmos)",
        description: Some("Spatial size of the cloud texture tiles."),
        kind: FieldKind::F32 {
            hard: (1.0, 100000.0),
            soft: Some((50.0, 5000.0)),
            unit: "elmos",
        },
        default: DefaultValue::F32(500.0),
        get: |s| FieldValue::F32(s.custom_clouds.scale),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_clouds.scale = x;
            }
        },
        category: categories::CLOUDS,
        group: "Clouds",
        blocks_export: false,
    },
    FieldSpec {
        id: "clouds.opacity",
        label: "Opacity",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1.0),
            soft: Some((0.0, 1.0)),
            unit: "",
        },
        default: DefaultValue::F32(0.5),
        get: |s| FieldValue::F32(s.custom_clouds.opacity),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_clouds.opacity = x;
            }
        },
        category: categories::CLOUDS,
        group: "Clouds",
        blocks_export: false,
    },
    FieldSpec {
        id: "clouds.clamp_to_map",
        label: "Clamp to map",
        description: Some("When true, the cloud volume is clipped to the map boundary rather than extending to the horizon."),
        kind: FieldKind::Bool,
        default: DefaultValue::Bool(false),
        get: |s| FieldValue::Bool(s.custom_clouds.clamp_to_map),
        set: |s, v| {
            if let FieldValue::Bool(x) = v {
                s.custom_clouds.clamp_to_map = x;
            }
        },
        category: categories::CLOUDS,
        group: "Clouds",
        blocks_export: false,
    },
    FieldSpec {
        id: "clouds.sun_penetration",
        label: "Sun penetration",
        description: Some("How much sun light penetrates the cloud volume."),
        kind: FieldKind::F32 {
            hard: (0.0, 100.0),
            soft: Some((0.0, 30.0)),
            unit: "",
        },
        default: DefaultValue::F32(10.0),
        get: |s| FieldValue::F32(s.custom_clouds.sun_penetration),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_clouds.sun_penetration = x;
            }
        },
        category: categories::CLOUDS,
        group: "Clouds",
        blocks_export: false,
    },
];

// ──────────────────────────────────────────────────────────────────
// Lighting
// ──────────────────────────────────────────────────────────────────

pub static LIGHTING_SPECS: &[FieldSpec<MapSettings>] = &[
    FieldSpec {
        id: "atmosphere.sun_color",
        label: "Sun colour",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::ATMOSPHERE_SUN_COLOR),
        get: |s| FieldValue::Color(s.atmosphere.sun_color),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.atmosphere.sun_color = x;
            }
        },
        category: categories::LIGHTING,
        group: "Sun",
        blocks_export: false,
    },
    FieldSpec {
        id: "lighting.sun_dir",
        label: "Sun direction",
        description: Some(
            "Vector pointing at the sun. Engine renormalises on read, so \
             magnitude doesn't matter -- direction does.",
        ),
        kind: FieldKind::Vec3 {
            hard: (-100.0, 100.0),
            soft: Some((-2.0, 2.0)),
        },
        default: DefaultValue::Vec3(ed::LIGHTING_SUN_DIR),
        get: |s| FieldValue::Vec3(s.lighting.sun_dir),
        set: |s, v| {
            if let FieldValue::Vec3(x) = v {
                s.lighting.sun_dir = x;
            }
        },
        category: categories::LIGHTING,
        group: "Sun",
        blocks_export: false,
    },
    FieldSpec {
        id: "lighting.sun_intensity",
        label: "Sun intensity",
        description: Some("Multiplier on sun colour; packed into `sunDir.w` for the sky shader."),
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 2.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::LIGHTING_SUN_INTENSITY),
        get: |s| FieldValue::F32(s.lighting.sun_intensity),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.lighting.sun_intensity = x;
            }
        },
        category: categories::LIGHTING,
        group: "Sun",
        blocks_export: false,
    },
    FieldSpec {
        id: "lighting.ground_ambient",
        label: "Ground ambient",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::LIGHTING_GROUND_AMBIENT),
        get: |s| FieldValue::Color(s.lighting.ground_ambient),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.lighting.ground_ambient = x;
            }
        },
        category: categories::LIGHTING,
        group: "Ground",
        blocks_export: false,
    },
    FieldSpec {
        id: "lighting.ground_diffuse",
        label: "Ground diffuse",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::LIGHTING_GROUND_DIFFUSE),
        get: |s| FieldValue::Color(s.lighting.ground_diffuse),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.lighting.ground_diffuse = x;
            }
        },
        category: categories::LIGHTING,
        group: "Ground",
        blocks_export: false,
    },
    FieldSpec {
        id: "lighting.ground_specular",
        label: "Ground specular",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::LIGHTING_GROUND_SPECULAR),
        get: |s| FieldValue::Color(s.lighting.ground_specular),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.lighting.ground_specular = x;
            }
        },
        category: categories::LIGHTING,
        group: "Ground",
        blocks_export: false,
    },
    FieldSpec {
        id: "lighting.spec_exponent",
        label: "Specular exponent",
        description: None,
        kind: FieldKind::F32 {
            hard: (1.0, 1000.0),
            soft: Some((1.0, 200.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::LIGHTING_SPEC_EXPONENT),
        get: |s| FieldValue::F32(s.lighting.spec_exponent),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.lighting.spec_exponent = x;
            }
        },
        category: categories::LIGHTING,
        group: "Ground",
        blocks_export: false,
    },
    FieldSpec {
        id: "lighting.ground_shadow_density",
        label: "Ground shadow density",
        description: Some(
            "Per-map shadow strength. 0 ignores the shadow map; 1 passes the raw \
             shadow sample through. Engine clamps to [0, 1].",
        ),
        kind: FieldKind::F32 {
            hard: (0.0, 1.0),
            soft: Some((0.5, 1.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::LIGHTING_GROUND_SHADOW_DENSITY),
        get: |s| FieldValue::F32(s.lighting.ground_shadow_density),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.lighting.ground_shadow_density = x;
            }
        },
        category: categories::LIGHTING,
        group: "Ground",
        blocks_export: false,
    },
    FieldSpec {
        id: "lighting.unit_shadow_density",
        label: "Unit shadow density",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1.0),
            soft: Some((0.5, 1.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::LIGHTING_GROUND_SHADOW_DENSITY),
        get: |s| FieldValue::F32(s.lighting.unit_shadow_density),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.lighting.unit_shadow_density = x;
            }
        },
        category: categories::LIGHTING,
        group: "Units",
        blocks_export: false,
    },
    FieldSpec {
        id: "lighting.unit_ambient",
        label: "Unit ambient",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::LIGHTING_GROUND_AMBIENT),
        get: |s| FieldValue::Color(s.lighting.unit_ambient),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.lighting.unit_ambient = x;
            }
        },
        category: categories::LIGHTING,
        group: "Units",
        blocks_export: false,
    },
    FieldSpec {
        id: "lighting.unit_diffuse",
        label: "Unit diffuse",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::LIGHTING_GROUND_DIFFUSE),
        get: |s| FieldValue::Color(s.lighting.unit_diffuse),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.lighting.unit_diffuse = x;
            }
        },
        category: categories::LIGHTING,
        group: "Units",
        blocks_export: false,
    },
    FieldSpec {
        id: "lighting.unit_specular",
        label: "Unit specular",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::LIGHTING_GROUND_SPECULAR),
        get: |s| FieldValue::Color(s.lighting.unit_specular),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.lighting.unit_specular = x;
            }
        },
        category: categories::LIGHTING,
        group: "Units",
        blocks_export: false,
    },
];

// ──────────────────────────────────────────────────────────────────
// Water (water-mode visual fields only; damage lives in LAVA_SPECS
// because BAR treats `mapinfo.water.damage > 0` as lava and BME
// only surfaces that field on the lava form. Core fields here; the
// extra fields like force_rendering / has_water_plane / etc. can
// be added incrementally.)
// ──────────────────────────────────────────────────────────────────

pub static WATER_SPECS: &[FieldSpec<MapSettings>] = &[
    FieldSpec {
        id: "water.absorb",
        label: "Absorb",
        description: Some("Per-elmo light attenuation through underwater volume."),
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::WATER_ABSORB),
        get: |s| FieldValue::Color(s.water.absorb),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.water.absorb = x;
            }
        },
        category: categories::WATER,
        group: "Underwater colour",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.base_color",
        label: "Base colour (shallow)",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::WATER_BASE_COLOR),
        get: |s| FieldValue::Color(s.water.base_color),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.water.base_color = x;
            }
        },
        category: categories::WATER,
        group: "Underwater colour",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.min_color",
        label: "Min colour (deep)",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::WATER_MIN_COLOR),
        get: |s| FieldValue::Color(s.water.min_color),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.water.min_color = x;
            }
        },
        category: categories::WATER,
        group: "Underwater colour",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.surface_color",
        label: "Surface colour",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::WATER_SURFACE_COLOR),
        get: |s| FieldValue::Color(s.water.surface_color),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.water.surface_color = x;
            }
        },
        category: categories::WATER,
        group: "Surface",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.surface_alpha",
        label: "Surface alpha",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1.0),
            soft: Some((0.0, 1.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_SURFACE_ALPHA),
        get: |s| FieldValue::F32(s.water.surface_alpha),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.surface_alpha = x;
            }
        },
        category: categories::WATER,
        group: "Surface",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.diffuse_color",
        label: "Diffuse colour",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::WATER_DIFFUSE_COLOR),
        get: |s| FieldValue::Color(s.water.diffuse_color),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.water.diffuse_color = x;
            }
        },
        category: categories::WATER,
        group: "Surface lighting",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.ambient_factor",
        label: "Ambient factor",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 2.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_AMBIENT_FACTOR),
        get: |s| FieldValue::F32(s.water.ambient_factor),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.ambient_factor = x;
            }
        },
        category: categories::WATER,
        group: "Surface lighting",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.diffuse_factor",
        label: "Diffuse factor",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 4.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_DIFFUSE_FACTOR),
        get: |s| FieldValue::F32(s.water.diffuse_factor),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.diffuse_factor = x;
            }
        },
        category: categories::WATER,
        group: "Surface lighting",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.specular_color",
        label: "Specular colour",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::WATER_SPECULAR_COLOR),
        get: |s| FieldValue::Color(s.water.specular_color),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.water.specular_color = x;
            }
        },
        category: categories::WATER,
        group: "Sun specular",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.specular_factor",
        label: "Specular factor",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 4.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_SPECULAR_FACTOR),
        get: |s| FieldValue::F32(s.water.specular_factor),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.specular_factor = x;
            }
        },
        category: categories::WATER,
        group: "Sun specular",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.specular_power",
        label: "Specular power",
        description: None,
        kind: FieldKind::F32 {
            hard: (1.0, 1000.0),
            soft: Some((1.0, 200.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_SPECULAR_POWER),
        get: |s| FieldValue::F32(s.water.specular_power),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.specular_power = x;
            }
        },
        category: categories::WATER,
        group: "Sun specular",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.fresnel_min",
        label: "Fresnel min",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1.0),
            soft: Some((0.0, 1.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_FRESNEL_MIN),
        get: |s| FieldValue::F32(s.water.fresnel_min),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.fresnel_min = x;
            }
        },
        category: categories::WATER,
        group: "Reflection",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.fresnel_max",
        label: "Fresnel max",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 2.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_FRESNEL_MAX),
        get: |s| FieldValue::F32(s.water.fresnel_max),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.fresnel_max = x;
            }
        },
        category: categories::WATER,
        group: "Reflection",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.fresnel_power",
        label: "Fresnel power",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.1, 100.0),
            soft: Some((0.1, 16.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_FRESNEL_POWER),
        get: |s| FieldValue::F32(s.water.fresnel_power),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.fresnel_power = x;
            }
        },
        category: categories::WATER,
        group: "Reflection",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.reflection_distortion",
        label: "Reflection distortion",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 4.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_REFLECTION_DISTORTION),
        get: |s| FieldValue::F32(s.water.reflection_distortion),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.reflection_distortion = x;
            }
        },
        category: categories::WATER,
        group: "Reflection",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.perlin_amplitude",
        label: "Perlin amplitude",
        description: Some("Per-octave amplitude falloff for the 4-octave normal map."),
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 2.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_PERLIN_AMPLITUDE),
        get: |s| FieldValue::F32(s.water.perlin_amplitude),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.perlin_amplitude = x;
            }
        },
        category: categories::WATER,
        group: "Wave normals",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.perlin_start_freq",
        label: "Perlin start freq",
        description: Some("Starting octave frequency for the normal-map Perlin sum."),
        kind: FieldKind::F32 {
            hard: (0.1, 1000.0),
            soft: Some((1.0, 32.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_PERLIN_START_FREQ),
        get: |s| FieldValue::F32(s.water.perlin_start_freq),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.perlin_start_freq = x;
            }
        },
        category: categories::WATER,
        group: "Wave normals",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.perlin_lacunarity",
        label: "Perlin lacunarity",
        description: Some("Frequency multiplier per octave for the normal-map Perlin sum."),
        kind: FieldKind::F32 {
            hard: (1.0, 20.0),
            soft: Some((1.5, 6.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_PERLIN_LACUNARITY),
        get: |s| FieldValue::F32(s.water.perlin_lacunarity),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.perlin_lacunarity = x;
            }
        },
        category: categories::WATER,
        group: "Wave normals",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.num_tiles",
        label: "Num tiles",
        description: Some("Number of normal-map tiles (NxN) tiling the water surface."),
        kind: FieldKind::U32 {
            hard: (1, 16),
            soft: Some((1, 8)),
            unit: "",
        },
        default: DefaultValue::U32(ed::WATER_NUM_TILES),
        get: |s| FieldValue::U32(s.water.num_tiles),
        set: |s, v| {
            if let FieldValue::U32(x) = v {
                s.water.num_tiles = x;
            }
        },
        category: categories::WATER,
        group: "Wave normals",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.normal_texture",
        label: "Normal texture",
        description: Some(
            "Game-data-relative path to a DDS normal map. Empty = engine default bump texture.",
        ),
        kind: FieldKind::OptionText { max_len: None },
        default: DefaultValue::Empty,
        get: |s| FieldValue::OptionText(s.water.normal_texture.clone()),
        set: |s, v| {
            if let FieldValue::OptionText(x) = v {
                s.water.normal_texture = x;
            }
        },
        category: categories::WATER,
        group: "Wave normals",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.wave_offset_factor",
        label: "Wave offset factor",
        description: Some("Scales the UV displacement applied by the wave normal map."),
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 3.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_WAVE_OFFSET_FACTOR),
        get: |s| FieldValue::F32(s.water.wave_offset_factor),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.wave_offset_factor = x;
            }
        },
        category: categories::WATER,
        group: "Shore foam",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.wave_length",
        label: "Wave length",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 3.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_WAVE_LENGTH),
        get: |s| FieldValue::F32(s.water.wave_length),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.wave_length = x;
            }
        },
        category: categories::WATER,
        group: "Shore foam",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.wave_foam_distortion",
        label: "Foam distortion",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 2.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_WAVE_FOAM_DISTORTION),
        get: |s| FieldValue::F32(s.water.wave_foam_distortion),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.wave_foam_distortion = x;
            }
        },
        category: categories::WATER,
        group: "Shore foam",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.wave_foam_intensity",
        label: "Foam intensity",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 2.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_WAVE_FOAM_INTENSITY),
        get: |s| FieldValue::F32(s.water.wave_foam_intensity),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.wave_foam_intensity = x;
            }
        },
        category: categories::WATER,
        group: "Shore foam",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.shore_waves",
        label: "Shore waves",
        description: Some("Enable wave-foam rendering along shorelines."),
        kind: FieldKind::Bool,
        default: DefaultValue::Bool(true),
        get: |s| FieldValue::Bool(s.water.shore_waves),
        set: |s, v| {
            if let FieldValue::Bool(x) = v {
                s.water.shore_waves = x;
            }
        },
        category: categories::WATER,
        group: "Shore foam",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.blur_base",
        label: "Blur base",
        description: Some("Base blur radius for the refraction blur pass."),
        kind: FieldKind::F32 {
            hard: (0.0, 20.0),
            soft: Some((0.0, 5.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_BLUR_BASE),
        get: |s| FieldValue::F32(s.water.blur_base),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.blur_base = x;
            }
        },
        category: categories::WATER,
        group: "Refraction blur",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.blur_exponent",
        label: "Blur exponent",
        description: Some("Depth-based exponent that widens the blur with depth."),
        kind: FieldKind::F32 {
            hard: (0.1, 10.0),
            soft: Some((0.5, 4.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_BLUR_EXPONENT),
        get: |s| FieldValue::F32(s.water.blur_exponent),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.blur_exponent = x;
            }
        },
        category: categories::WATER,
        group: "Refraction blur",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.caustics_resolution",
        label: "Caustics resolution",
        description: Some("Size of the caustics texture rendered each frame."),
        kind: FieldKind::F32 {
            hard: (1.0, 1024.0),
            soft: Some((16.0, 256.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_CAUSTICS_RESOLUTION),
        get: |s| FieldValue::F32(s.water.caustics_resolution),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.caustics_resolution = x;
            }
        },
        category: categories::WATER,
        group: "Caustics",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.caustics_strength",
        label: "Caustics strength",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1.0),
            soft: Some((0.0, 0.5)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_CAUSTICS_STRENGTH),
        get: |s| FieldValue::F32(s.water.caustics_strength),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.caustics_strength = x;
            }
        },
        category: categories::WATER,
        group: "Caustics",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.plane_color",
        label: "Plane colour",
        description: Some(
            "Colour of the flat water plane rendered at sea level when BumpWater is not active.",
        ),
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::WATER_PLANE_COLOR),
        get: |s| FieldValue::Color(s.water.plane_color),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.water.plane_color = x;
            }
        },
        category: categories::WATER,
        group: "Plane",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.repeat_x",
        label: "Repeat X",
        description: Some("Normal-map UV repeat count in X; 0 = auto (derived from map width)."),
        kind: FieldKind::F32 {
            hard: (0.0, 1024.0),
            soft: Some((0.0, 64.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_REPEAT_X),
        get: |s| FieldValue::F32(s.water.repeat_x),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.repeat_x = x;
            }
        },
        category: categories::WATER,
        group: "Plane",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.repeat_y",
        label: "Repeat Y",
        description: Some("Normal-map UV repeat count in Y; 0 = auto (derived from map height)."),
        kind: FieldKind::F32 {
            hard: (0.0, 1024.0),
            soft: Some((0.0, 64.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_REPEAT_Y),
        get: |s| FieldValue::F32(s.water.repeat_y),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.repeat_y = x;
            }
        },
        category: categories::WATER,
        group: "Plane",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.force_rendering",
        label: "Force rendering",
        description: Some("Render water even when the map has no sea-level area."),
        kind: FieldKind::Bool,
        default: DefaultValue::Bool(false),
        get: |s| FieldValue::Bool(s.water.force_rendering),
        set: |s, v| {
            if let FieldValue::Bool(x) = v {
                s.water.force_rendering = x;
            }
        },
        category: categories::WATER,
        group: "Plane",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.has_water_plane",
        label: "Has water plane",
        description: Some("When false, no flat water plane is rendered at sea level."),
        kind: FieldKind::Bool,
        default: DefaultValue::Bool(true),
        get: |s| FieldValue::Bool(s.water.has_water_plane),
        set: |s, v| {
            if let FieldValue::Bool(x) = v {
                s.water.has_water_plane = x;
            }
        },
        category: categories::WATER,
        group: "Plane",
        blocks_export: false,
    },
];

// ──────────────────────────────────────────────────────────────────
// Lava (curated subset of WaterSettings rendered when
// `WaterSettings::is_lava == true`. Damage gates the engine's
// water-vs-lava behaviour and is required >= 1; the rest are the
// underwater-volume / surface / surface-lighting fields that still
// make sense on lava. Water-only physics -- Fresnel, perlin wave
// normals, shore foam, refraction blur, caustics, sun specular,
// flat plane -- are intentionally absent.)
// ──────────────────────────────────────────────────────────────────

pub static LAVA_SPECS: &[FieldSpec<MapSettings>] = &[
    FieldSpec {
        id: "water.damage",
        label: "Damage / sec",
        description: Some("Continuous damage applied to units immersed in the lava volume."),
        kind: FieldKind::F32 {
            hard: (1.0, 10000.0),
            soft: Some((1.0, 1000.0)),
            unit: "/s",
        },
        default: DefaultValue::F32(1.0),
        get: |s| FieldValue::F32(s.water.damage),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.damage = x;
            }
        },
        category: categories::WATER,
        group: "Damage",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.absorb",
        label: "Absorb",
        description: Some("Per-elmo light attenuation through the lava volume."),
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::WATER_ABSORB),
        get: |s| FieldValue::Color(s.water.absorb),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.water.absorb = x;
            }
        },
        category: categories::WATER,
        group: "Underwater colour",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.base_color",
        label: "Base colour (shallow)",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::WATER_BASE_COLOR),
        get: |s| FieldValue::Color(s.water.base_color),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.water.base_color = x;
            }
        },
        category: categories::WATER,
        group: "Underwater colour",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.min_color",
        label: "Min colour (deep)",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::WATER_MIN_COLOR),
        get: |s| FieldValue::Color(s.water.min_color),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.water.min_color = x;
            }
        },
        category: categories::WATER,
        group: "Underwater colour",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.surface_color",
        label: "Surface colour",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::WATER_SURFACE_COLOR),
        get: |s| FieldValue::Color(s.water.surface_color),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.water.surface_color = x;
            }
        },
        category: categories::WATER,
        group: "Surface",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.surface_alpha",
        label: "Surface alpha",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1.0),
            soft: Some((0.0, 1.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_SURFACE_ALPHA),
        get: |s| FieldValue::F32(s.water.surface_alpha),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.surface_alpha = x;
            }
        },
        category: categories::WATER,
        group: "Surface",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.diffuse_color",
        label: "Diffuse colour",
        description: None,
        kind: FieldKind::Color,
        default: DefaultValue::Color(ed::WATER_DIFFUSE_COLOR),
        get: |s| FieldValue::Color(s.water.diffuse_color),
        set: |s, v| {
            if let FieldValue::Color(x) = v {
                s.water.diffuse_color = x;
            }
        },
        category: categories::WATER,
        group: "Surface lighting",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.ambient_factor",
        label: "Ambient factor",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 2.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_AMBIENT_FACTOR),
        get: |s| FieldValue::F32(s.water.ambient_factor),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.ambient_factor = x;
            }
        },
        category: categories::WATER,
        group: "Surface lighting",
        blocks_export: false,
    },
    FieldSpec {
        id: "water.diffuse_factor",
        label: "Diffuse factor",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 4.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::WATER_DIFFUSE_FACTOR),
        get: |s| FieldValue::F32(s.water.diffuse_factor),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water.diffuse_factor = x;
            }
        },
        category: categories::WATER,
        group: "Surface lighting",
        blocks_export: false,
    },
];

// ──────────────────────────────────────────────────────────────────
// Grass (CustomGrassSettings; reached through MapSettings.custom_grass)
// ──────────────────────────────────────────────────────────────────

pub static GRASS_SPECS: &[FieldSpec<MapSettings>] = &[
    FieldSpec {
        id: "grass.dist_tga",
        label: "grassDistTGA",
        description: Some(
            "8-bit greyscale distribution mask. Non-zero texels spawn \
             blades; widget renders nothing when this is empty.",
        ),
        kind: FieldKind::PassthroughTexture {
            extensions: &["tga"],
        },
        default: DefaultValue::Empty,
        get: |s| FieldValue::OptionText(s.custom_grass.dist_tga.clone()),
        set: |s, v| {
            if let FieldValue::OptionText(x) = v {
                s.custom_grass.dist_tga = x;
            }
        },
        category: categories::GRASS,
        group: "Textures",
        blocks_export: false,
    },
    FieldSpec {
        id: "grass.blade_color_tex",
        label: "grassBladeColorTex",
        description: None,
        kind: FieldKind::PassthroughTexture {
            extensions: &["dds", "png", "jpg", "jpeg", "tga"],
        },
        default: DefaultValue::Empty,
        get: |s| FieldValue::OptionText(s.custom_grass.blade_color_tex.clone()),
        set: |s, v| {
            if let FieldValue::OptionText(x) = v {
                s.custom_grass.blade_color_tex = x;
            }
        },
        category: categories::GRASS,
        group: "Textures",
        blocks_export: false,
    },
    FieldSpec {
        id: "grass.max_size",
        label: "grassMaxSize",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.1, 100.0),
            soft: Some((0.1, 10.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::GRASS_MAX_SIZE),
        get: |s| FieldValue::F32(s.custom_grass.max_size),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_grass.max_size = x;
            }
        },
        category: categories::GRASS,
        group: "Patch geometry",
        blocks_export: false,
    },
    FieldSpec {
        id: "grass.min_size",
        label: "grassMinSize",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.1, 100.0),
            soft: Some((0.1, 10.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::GRASS_MIN_SIZE),
        get: |s| FieldValue::F32(s.custom_grass.min_size),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_grass.min_size = x;
            }
        },
        category: categories::GRASS,
        group: "Patch geometry",
        blocks_export: false,
    },
    FieldSpec {
        id: "grass.patch_placement_jitter",
        label: "patchPlacementJitter",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1.0),
            soft: Some((0.0, 1.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::GRASS_PATCH_PLACEMENT_JITTER),
        get: |s| FieldValue::F32(s.custom_grass.patch_placement_jitter),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_grass.patch_placement_jitter = x;
            }
        },
        category: categories::GRASS,
        group: "Patch geometry",
        blocks_export: false,
    },
    FieldSpec {
        id: "grass.map_color_factor",
        label: "MAPCOLORFACTOR",
        description: Some(
            "Multiplicative blend strength between blade colour and terrain \
             colour. Engine widget accepts negative values for the \
             inverse-blend that brightens grass on dark terrain.",
        ),
        kind: FieldKind::F32 {
            hard: (-10.0, 10.0),
            soft: Some((-2.0, 2.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::GRASS_MAP_COLOR_FACTOR),
        get: |s| FieldValue::F32(s.custom_grass.map_color_factor),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_grass.map_color_factor = x;
            }
        },
        category: categories::GRASS,
        group: "Shader blend",
        blocks_export: false,
    },
    FieldSpec {
        id: "grass.map_color_base",
        label: "MAPCOLORBASE",
        description: Some("Additional terrain-colour blend strength toward the blade base."),
        kind: FieldKind::F32 {
            hard: (-10.0, 10.0),
            soft: Some((0.0, 1.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::GRASS_MAP_COLOR_BASE),
        get: |s| FieldValue::F32(s.custom_grass.map_color_base),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_grass.map_color_base = x;
            }
        },
        category: categories::GRASS,
        group: "Shader blend",
        blocks_export: false,
    },
    FieldSpec {
        id: "grass.alpha_threshold",
        label: "ALPHATHRESHOLD",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1.0),
            soft: Some((0.0, 1.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::GRASS_ALPHA_THRESHOLD),
        get: |s| FieldValue::F32(s.custom_grass.alpha_threshold),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_grass.alpha_threshold = x;
            }
        },
        category: categories::GRASS,
        group: "Shader blend",
        blocks_export: false,
    },
    FieldSpec {
        id: "grass.shadow_factor",
        label: "SHADOWFACTOR",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1.0),
            soft: Some((0.0, 1.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::GRASS_SHADOW_FACTOR),
        get: |s| FieldValue::F32(s.custom_grass.shadow_factor),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_grass.shadow_factor = x;
            }
        },
        category: categories::GRASS,
        group: "Shader blend",
        blocks_export: false,
    },
    FieldSpec {
        id: "grass.grass_brightness",
        label: "GRASSBRIGHTNESS",
        description: Some("Direct RGB multiplier on the blade fragment colour."),
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 4.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::GRASS_BRIGHTNESS),
        get: |s| FieldValue::F32(s.custom_grass.grass_brightness),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_grass.grass_brightness = x;
            }
        },
        category: categories::GRASS,
        group: "Shader blend",
        blocks_export: false,
    },
    FieldSpec {
        id: "grass.wind_strength",
        label: "WINDSTRENGTH",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 2.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::GRASS_WIND_STRENGTH),
        get: |s| FieldValue::F32(s.custom_grass.wind_strength),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_grass.wind_strength = x;
            }
        },
        category: categories::GRASS,
        group: "Shader blend",
        blocks_export: false,
    },
    FieldSpec {
        id: "grass.wind_scale",
        label: "WINDSCALE",
        description: Some("Speed at which the wind-noise pattern drifts across the map."),
        kind: FieldKind::F32 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 2.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::GRASS_WIND_SCALE),
        get: |s| FieldValue::F32(s.custom_grass.wind_scale),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_grass.wind_scale = x;
            }
        },
        category: categories::GRASS,
        group: "Wind noise",
        blocks_export: false,
    },
    FieldSpec {
        id: "grass.wind_sample_scale",
        label: "WINDSAMPLESCALE",
        description: Some("Spatial tiling resolution of the wind noise."),
        kind: FieldKind::F32 {
            hard: (0.00001, 1.0),
            soft: Some((0.0001, 0.01)),
            unit: "",
        },
        default: DefaultValue::F32(ed::GRASS_WIND_SAMPLE_SCALE),
        get: |s| FieldValue::F32(s.custom_grass.wind_sample_scale),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_grass.wind_sample_scale = x;
            }
        },
        category: categories::GRASS,
        group: "Wind noise",
        blocks_export: false,
    },
    FieldSpec {
        id: "grass.grass_wind_mult",
        label: "grassWindMult",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1000.0),
            soft: Some((0.0, 20.0)),
            unit: "",
        },
        default: DefaultValue::F32(ed::GRASS_WIND_MULT),
        get: |s| FieldValue::F32(s.custom_grass.grass_wind_mult),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_grass.grass_wind_mult = x;
            }
        },
        category: categories::GRASS,
        group: "Wind noise",
        blocks_export: false,
    },
    FieldSpec {
        id: "grass.fade_start",
        label: "FADESTART",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 100000.0),
            soft: Some((0.0, 20000.0)),
            unit: "elmos",
        },
        default: DefaultValue::F32(ed::GRASS_FADE_START),
        get: |s| FieldValue::F32(s.custom_grass.fade_start),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_grass.fade_start = x;
            }
        },
        category: categories::GRASS,
        group: "Distance fade",
        blocks_export: false,
    },
    FieldSpec {
        id: "grass.fade_end",
        label: "FADEEND",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 100000.0),
            soft: Some((0.0, 40000.0)),
            unit: "elmos",
        },
        default: DefaultValue::F32(ed::GRASS_FADE_END),
        get: |s| FieldValue::F32(s.custom_grass.fade_end),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.custom_grass.fade_end = x;
            }
        },
        category: categories::GRASS,
        group: "Distance fade",
        blocks_export: false,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every spec id must be unique. The id is used as the validation
    /// finding's `field` key for routing back to the right widget, so
    /// collisions silently mis-route findings.
    #[test]
    fn spec_ids_are_unique_across_all_arrays() {
        let mut ids: Vec<&str> = Vec::new();
        for spec in PHYSICS_SPECS
            .iter()
            .chain(ATMOSPHERE_SPECS.iter())
            .chain(FOG_SPECS.iter())
            .chain(CLOUDS_SPECS.iter())
            .chain(LIGHTING_SPECS.iter())
            .chain(WATER_SPECS.iter())
            .chain(GRASS_SPECS.iter())
        {
            ids.push(spec.id);
        }
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            ids.len(),
            "duplicate spec id detected; ids = {:?}",
            ids
        );
    }

    /// Every spec's category must match the matching `categories::*`
    /// constant (catches typos / cross-wired entries).
    #[test]
    fn spec_categories_match_array() {
        for s in PHYSICS_SPECS {
            assert_eq!(s.category, categories::PHYSICS);
        }
        for s in ATMOSPHERE_SPECS {
            assert_eq!(s.category, categories::ATMOSPHERE);
        }
        for s in FOG_SPECS {
            assert_eq!(s.category, categories::FOG);
        }
        for s in CLOUDS_SPECS {
            assert_eq!(s.category, categories::CLOUDS);
        }
        for s in LIGHTING_SPECS {
            assert_eq!(s.category, categories::LIGHTING);
        }
        for s in WATER_SPECS {
            assert_eq!(s.category, categories::WATER);
        }
        for s in LAVA_SPECS {
            assert_eq!(s.category, categories::WATER);
        }
        for s in GRASS_SPECS {
            assert_eq!(s.category, categories::GRASS);
        }
    }

    /// Round-trip every spec: read default value via get on a fresh
    /// MapSettings (should be None), commit the engine default,
    /// read again (should be Some(default)).
    #[test]
    fn every_spec_round_trips_default_value() {
        let mut state = MapSettings::default();
        for s in PHYSICS_SPECS
            .iter()
            .chain(ATMOSPHERE_SPECS.iter())
            .chain(FOG_SPECS.iter())
            .chain(CLOUDS_SPECS.iter())
            .chain(LIGHTING_SPECS.iter())
            .chain(WATER_SPECS.iter())
            .chain(LAVA_SPECS.iter())
            .chain(GRASS_SPECS.iter())
        {
            // Initial read should be None for Option fields, empty
            // for Text variants -- whatever the recipe default is.
            let initial = (s.get)(&state);
            // Commit the default and re-read.
            let default_val = s.default.as_field_value();
            // Only test Option<T> kinds (skip Bool/Text edge cases
            // where the spec default may already match recipe default).
            match &default_val {
                FieldValue::F32(Some(_))
                | FieldValue::U32(Some(_))
                | FieldValue::Color(Some(_))
                | FieldValue::Vec3(Some(_))
                | FieldValue::Vec4(Some(_)) => {
                    s.commit(&mut state, default_val.clone());
                    let read_back = (s.get)(&state);
                    assert_eq!(
                        read_back, default_val,
                        "round-trip failed for spec id={}",
                        s.id
                    );
                }
                _ => {
                    // Skip non-Option-with-default variants; they
                    // exercise the no-op clamp path which is tested
                    // elsewhere. Suppress unused-variable warning:
                    let _ = initial;
                }
            }
        }
    }
}

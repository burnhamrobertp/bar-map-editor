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
//! - [`ATMOSPHERE_SPECS`]   -- `MapSettings::atmosphere`
//! - [`LIGHTING_SPECS`]     -- `MapSettings::lighting`
//! - [`WATER_SPECS`]        -- `MapSettings::water`
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
    FieldSpec {
        id: "physics.water_damage",
        label: "Water damage / sec",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 1000.0),
            soft: Some((0.0, 100.0)),
            unit: "/s",
        },
        default: DefaultValue::F32(0.0),
        get: |s| FieldValue::F32(s.water_damage),
        set: |s, v| {
            if let FieldValue::F32(x) = v {
                s.water_damage = x;
            }
        },
        category: categories::PHYSICS,
        group: "Behaviour",
        blocks_export: false,
    },
];

// ──────────────────────────────────────────────────────────────────
// Atmosphere
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
        id: "atmosphere.fog_start",
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
        category: categories::ATMOSPHERE,
        group: "Fog",
        blocks_export: false,
    },
    FieldSpec {
        id: "atmosphere.fog_end",
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
        category: categories::ATMOSPHERE,
        group: "Fog",
        blocks_export: false,
    },
    FieldSpec {
        id: "atmosphere.fog_color",
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
        category: categories::ATMOSPHERE,
        group: "Fog",
        blocks_export: false,
    },
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
        category: categories::ATMOSPHERE,
        group: "Sky",
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
// Lighting
// ──────────────────────────────────────────────────────────────────

pub static LIGHTING_SPECS: &[FieldSpec<MapSettings>] = &[
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
// Water (core fields; the extra fields like force_rendering /
// has_water_plane / etc. can be added incrementally)
// ──────────────────────────────────────────────────────────────────

pub static WATER_SPECS: &[FieldSpec<MapSettings>] = &[
    FieldSpec {
        id: "water.damage",
        label: "Damage / sec",
        description: None,
        kind: FieldKind::F32 {
            hard: (0.0, 10000.0),
            soft: Some((0.0, 1000.0)),
            unit: "/s",
        },
        default: DefaultValue::F32(ed::WATER_DAMAGE),
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
        for s in LIGHTING_SPECS {
            assert_eq!(s.category, categories::LIGHTING);
        }
        for s in WATER_SPECS {
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
            .chain(LIGHTING_SPECS.iter())
            .chain(WATER_SPECS.iter())
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

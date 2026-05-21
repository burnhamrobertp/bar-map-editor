//! # bar-project
//!
//! BAR - Map Editor project format layer — the shared schema between the GUI
//! editor and the engine runtime.
//!
//! Contains:
//! - `recipe`: graph serialization format (Recipe, RecipeNode, MapSettings, …)
//! - `project`: full project file I/O (Project, EditorLayout, Position)
//! - `scan`: SD7 work-directory scan result type (WorkDirScan)
//!
//! This crate deliberately has no dependency on `bar-engine` (executors, export
//! pipeline, archive I/O).  Both the GUI and the engine depend on it.

pub mod engine_defaults;
pub mod fc;
pub mod field_schema;
pub mod fs_util;
pub mod mapinfo;
pub mod package;
pub mod project;
pub mod recipe;
pub mod recipe_fields;
pub mod scan;
pub mod validation;

pub use fc::{mint_fc_layer_ids, populate_fc_layer_paths, FC_LAYER_KINDS};
pub use fs_util::find_file_in_dir;
pub use mapinfo::{
    apply_mapinfo_overrides, parse_mapinfo_number, parse_mapinfo_smf_heights, parse_mapinfo_string,
    parse_mapinfo_string_list, parse_mapinfo_vec3,
};
pub use package::{
    read_asset_file, write_asset_file, AssetHeader, AssetId, AssetKind, AssetStat, Fingerprint,
    PackageDir,
};
pub use project::{
    EditorLayout, MacroParamSpec, NodeGroup, NodeSize, PersistedCanvasView, Position, Project,
    SubgraphPort,
};
pub use recipe::{
    AtmosphereSettings, CustomCloudsSettings, CustomFogSettings, CustomGrassSettings,
    DetailTexture, FeatureSource, LightingSettings, MapSettings, OutputConfig, PlacedFeature,
    Recipe, RecipeConnection, RecipeNode, ReplaceTable, ResolvedAtmosphere, ResolvedGrassSettings,
    ResolvedLighting, ResolvedMapSettings, ResolvedWater, ResourcesSettings, SoundSettings,
    TerrainTypeEntry, WaterSettings, RECIPE_SCHEMA_VERSION,
};
pub use scan::{scan_to_project, PendingAsset, PendingRawFile, WorkDirScan, SMF_MINIMAP_SIDE_CAR};
pub use validation::{has_errors, validate_project, Finding, Severity};

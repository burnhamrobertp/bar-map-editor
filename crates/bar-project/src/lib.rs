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

pub mod package;
pub mod project;
pub mod recipe;
pub mod scan;
pub mod validation;

pub use package::{
    read_asset_file, write_asset_file, AssetHeader, AssetId, AssetKind, AssetStat, Fingerprint,
    PackageDir,
};
pub use project::{
    EditorLayout, MacroParamSpec, NodeGroup, NodeSize, PersistedCanvasView, Position, Project,
    SubgraphPort,
};
pub use recipe::{
    AtmosphereSettings, DetailTexture, LightingSettings, MapSettings, OutputConfig, Recipe,
    RecipeConnection, RecipeNode, WaterSettings, RECIPE_SCHEMA_VERSION,
};
pub use scan::{scan_to_project, PendingAsset, PendingRawFile, WorkDirScan};
pub use validation::{has_errors, validate_project, Finding, Severity};

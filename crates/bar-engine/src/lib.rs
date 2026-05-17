//! # bar-engine
//!
//! BAR map editor runtime engine: the shared core between the GUI application and
//! the CLI tool. Contains:
//! - `CpuExecutor`: bridges graph nodes to compute operations
//! - `HybridExecutor`: GPU-accelerated noise + CPU fallback
//! - `Recipe`: versioned serializable graph configuration format
//! - `Project`: full project save/load (recipe + editor layout)
//! - Export orchestration: graph → .smf / PNG output

pub mod bundler;
pub mod compile;
pub mod executor;
pub mod export;
pub mod extract;
pub mod feature_catalog;
pub mod hybrid_executor;
pub mod importer;
pub mod project;
pub mod recipe;
pub mod targets;

pub use bar_project::scan_to_project;
pub use bar_project::{
    write_asset_file, AssetHeader, AssetStat, Fingerprint, PackageDir, PendingAsset, PendingRawFile,
};
pub use bundler::{execute_bundlers, find_bundler_nodes, BundlerResult};
pub use compile::{compile_from_outputs, compile_project, read_compiled_heightmap, CompileDims};
pub use executor::CpuExecutor;
pub use export::{
    export_grassmap_png, export_heightmap_png, export_normalmap_png, export_sd7_directory,
    export_smf, export_smt, export_texture_png, export_with_target, write_color_png, write_smf,
};
pub use extract::{
    extract_sd7_to_work_dir, extract_sd7_to_work_dir_with_progress, prune_old_work_dirs,
    work_dir_root, WorkDirScan,
};
pub use feature_catalog::{
    read_file_from_archive, read_file_from_rapid_pool, sibling_zip_archives, FeatureCatalog,
    FeatureDef,
};
pub use hybrid_executor::HybridExecutor;
pub use importer::{import_sd7, import_sd7_to_project, ImportResult};
pub use project::{EditorLayout, Position, Project};
pub use recipe::{
    AtmosphereSettings, DetailTexture, LightingSettings, MapSettings, OutputConfig, Recipe,
    RecipeConnection, RecipeNode, WaterSettings,
};
// Validation lives in bar-project so the GUI can use it without pulling
// in bar-engine. Re-exported here for callers that already depend on
// bar-engine (CLI, app shell).
pub use bar_project::validation::{has_errors, validate_project, Finding, Severity};

// Re-export core graph types for convenience
pub use bar_graph::{NodeType, ParamValue};

// Export target system
pub use targets::{
    ExportCodec, ExportPlan, SpringSmfCodec, TargetConfig, TargetRegistry, WrittenFiles,
};

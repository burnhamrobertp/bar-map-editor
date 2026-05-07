//! Result type for .sd7 work-directory scanning.
//!
//! `WorkDirScan` is produced by `bar_engine::extract_sd7_to_work_dir` and
//! consumed by the GUI's import workflow.  Keeping it here (rather than in
//! bar-engine) lets the GUI depend on this lightweight type without pulling in
//! the full engine.

use std::path::PathBuf;

/// Result of scanning an extracted .sd7 work directory.
#[derive(Debug)]
pub struct WorkDirScan {
    /// Absolute path to the work directory.
    pub work_dir: PathBuf,
    /// Map name derived from the archive filename stem.
    pub map_name: String,
    /// Absolute path to the first `.smf` file found (if any).
    pub smf_abs: Option<PathBuf>,
    /// Archive-relative path to the `.smf` file (e.g. `maps/mymap.smf`).
    pub smf_rel: Option<PathBuf>,
    /// Absolute path to the first `.smt` file found (if any).
    pub smt_abs: Option<PathBuf>,
    /// Archive-relative path to the `.smt` file.
    pub smt_rel: Option<PathBuf>,
    /// Tile grid dimensions `(tiles_x, tiles_y)` read from the SMF header.
    pub tile_grid: Option<(u32, u32)>,
    /// Heightmap pixel dimensions read from the SMF header (`map_x + 1` × `map_y + 1`).
    /// `None` when no SMF file is present.
    pub map_dims: Option<(u32, u32)>,
    /// Terrain height range from the SMF header (world units, same coordinate space as X/Z).
    /// Used to compute an accurate vertical scale for the 3D preview.
    /// `None` when no SMF file is present.
    pub height_range: Option<(f32, f32)>,
    /// All other files as `(absolute_path, archive_relative_path)` pairs.
    pub passthrough_files: Vec<(PathBuf, PathBuf)>,
}

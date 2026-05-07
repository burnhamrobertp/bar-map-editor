//! Export codec trait: the interface for format-specific export logic.

use std::path::Path;

use anyhow::Result;

use super::config::TargetConfig;
use super::dimensions::DimensionSet;
use super::layers::LayerSet;
use super::validation::ValidationError;
use crate::recipe::MapSettings;

/// Files written by a codec during export.
#[derive(Debug, Default)]
pub struct WrittenFiles {
    /// Paths of files written (relative to output directory).
    pub files: Vec<String>,
}

/// The export plan computed before writing.
#[derive(Debug)]
pub struct ExportPlan {
    /// Map name (used in file names and the `name` field of mapinfo.lua).
    pub map_name: String,
    /// Optional shortname used by mapinfo.lua. Falls back to `map_name`.
    pub shortname: Option<String>,
    /// Free-form description used by mapinfo.lua.
    pub description: String,
    /// Optional author used by mapinfo.lua.
    pub author: Option<String>,
    /// Optional version string used by mapinfo.lua.
    pub version: Option<String>,
    /// Resolved dimensions for all layers.
    pub dimensions: DimensionSet,
    /// Map settings (heights, atmosphere, lighting, etc.).
    pub settings: MapSettings,
}

/// Trait for format-specific export implementations.
///
/// A codec handles the binary format writing for a specific target
/// (e.g., Spring SMF, raw PNG layers). It is parameterized by a
/// `TargetConfig` that provides format-specific values.
pub trait ExportCodec: Send + Sync {
    /// Unique codec identifier (must match `codec` field in target configs).
    fn id(&self) -> &str;

    /// Human-readable description.
    fn description(&self) -> &str;

    /// Validate that a target config + export plan combination is legal.
    /// Called before export to catch issues early.
    fn validate(
        &self,
        config: &TargetConfig,
        plan: &ExportPlan,
        layers: &LayerSet,
    ) -> Result<Vec<ValidationError>>;

    /// Compute the dimension set for an export given the heightmap dimensions.
    fn compute_dimensions(
        &self,
        config: &TargetConfig,
        heightmap_width: u32,
        heightmap_height: u32,
    ) -> DimensionSet;

    /// Write all format-specific files to the output directory.
    fn write(
        &self,
        config: &TargetConfig,
        plan: &ExportPlan,
        layers: &LayerSet,
        output_dir: &Path,
    ) -> Result<WrittenFiles>;
}

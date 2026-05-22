//! Export target configuration.

use super::dimensions::DimensionConstraint;
use super::layers::LayerRequirement;
use super::packaging::PackagingConfig;

/// Complete export target configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TargetConfig {
    /// Unique target identifier (e.g., "spring-smf", "raw-layers").
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Codec identifier (selects the Rust implementation).
    pub codec: String,
    /// Codec-specific parameters.
    #[serde(default)]
    pub codec_params: CodecParams,
    /// Constraints on the base map dimensions (in map squares).
    #[serde(default)]
    pub dimension_constraint: DimensionConstraint,
    /// Layer requirements for this target.
    #[serde(default)]
    pub layers: Vec<LayerRequirement>,
    /// Packaging configuration.
    #[serde(default)]
    pub packaging: PackagingConfig,
    /// Path to metadata template file (relative to target config).
    #[serde(default)]
    pub metadata_template: Option<String>,
}

/// Codec-specific parameters (varies by codec).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodecParams {
    /// Distance between heightmap vertices in world units.
    #[serde(default = "default_square_size")]
    pub square_size: u32,
    /// Texels per square for texture mapping.
    #[serde(default = "default_texels_per_square")]
    pub texels_per_square: u32,
    /// Tile size for SMT tiles.
    #[serde(default = "default_tile_size")]
    pub tile_size: u32,
    /// Default minimum terrain height.
    #[serde(default = "default_min_height")]
    pub min_height: f32,
    /// Default maximum terrain height.
    #[serde(default = "default_max_height")]
    pub max_height: f32,
}

impl Default for CodecParams {
    fn default() -> Self {
        Self {
            square_size: default_square_size(),
            texels_per_square: default_texels_per_square(),
            tile_size: default_tile_size(),
            min_height: default_min_height(),
            max_height: default_max_height(),
        }
    }
}

fn default_square_size() -> u32 {
    8
}
fn default_texels_per_square() -> u32 {
    8
}
fn default_tile_size() -> u32 {
    32
}
fn default_min_height() -> f32 {
    -200.0
}
fn default_max_height() -> f32 {
    800.0
}

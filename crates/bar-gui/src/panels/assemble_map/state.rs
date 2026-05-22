//! Per-session state for the Assemble Map wizard. Lives on
//! `BarEditorApp` for the duration of the wizard; cleared on Finish /
//! Cancel.
//!
//! Every "pick" is the absolute filesystem path the user chose with
//! the file dialog. The wizard delays any copying / decoding until
//! Finish runs -- pages are render-only and can be revisited via
//! Back without re-prompting.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Identity,
    Heightmap,
    Surface,
    Extras,
}

impl Page {
    pub fn next(self) -> Option<Self> {
        match self {
            Self::Identity => Some(Self::Heightmap),
            Self::Heightmap => Some(Self::Surface),
            Self::Surface => Some(Self::Extras),
            Self::Extras => None,
        }
    }
    pub fn prev(self) -> Option<Self> {
        match self {
            Self::Identity => None,
            Self::Heightmap => Some(Self::Identity),
            Self::Surface => Some(Self::Heightmap),
            Self::Extras => Some(Self::Surface),
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Identity => "Identity",
            Self::Heightmap => "Heightmap",
            Self::Surface => "Surface layers",
            Self::Extras => "Optional extras",
        }
    }
    pub fn step_index(self) -> usize {
        match self {
            Self::Identity => 0,
            Self::Heightmap => 1,
            Self::Surface => 2,
            Self::Extras => 3,
        }
    }
    pub const COUNT: usize = 4;
}

/// All inputs the wizard collects, populated incrementally as the user
/// moves through pages. Defaults are "empty / unset" -- the Finish
/// handler treats absent optional picks as "skip; let the bundler /
/// renderer fall back to the engine default".
#[derive(Default, Debug, Clone)]
pub struct AssembleMapPicks {
    // Identity
    pub name: String,
    pub author: String,
    pub description: String,
    pub version: String,

    // Heightmap + dimensions
    pub heightmap_path: Option<PathBuf>,
    /// Derived from the heightmap resolution at pick-time.
    /// `width_cells = squares_x * 64 + 1`.
    pub squares_x: u32,
    pub squares_z: u32,
    pub min_height: f32,
    pub max_height: f32,

    // Surface layers
    pub diffuse_path: Option<PathBuf>,
    pub metalmap_path: Option<PathBuf>,
    pub typemap_path: Option<PathBuf>,
    pub grass_distribution_path: Option<PathBuf>,

    // Optional extras
    pub splat_distribution_path: Option<PathBuf>,
    pub splat_detail_normal_1_path: Option<PathBuf>,
    pub splat_detail_normal_2_path: Option<PathBuf>,
    pub splat_detail_normal_3_path: Option<PathBuf>,
    pub splat_detail_normal_4_path: Option<PathBuf>,
    pub specular_path: Option<PathBuf>,
    pub sky_reflect_mod_path: Option<PathBuf>,
    pub detail_normal_path: Option<PathBuf>,
    pub light_emission_path: Option<PathBuf>,
    pub minimap_path: Option<PathBuf>,
    pub skybox_path: Option<PathBuf>,
}

#[derive(Default, Debug, Clone)]
pub struct AssembleMapState {
    pub page: Page,
    pub picks: AssembleMapPicks,
    /// Last error from the heightmap decode step, surfaced on the
    /// Heightmap page. Cleared on a successful pick.
    pub heightmap_error: Option<String>,
}

impl AssembleMapState {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

//! Project-level map metadata: dimensions, MapSettings, recipe
//! identity, and spawn-marker drag state.
//!
//! `MapSettings` is the source of truth (it's what `validate_project`
//! reads and what gets serialised in the .barproj recipe). The
//! `width`/`height`/`min_h`/`max_h` shadow fields are bound to egui
//! widgets in the Map Settings modal and the `mapinfo_editor` panel;
//! they're kept in sync with `settings` whenever either side is
//! edited.

use bar_project::recipe::PlacedFeature;
use bar_project::MapSettings;

/// Plain-data snapshot of SMF ground-shading inputs (lighting +
/// water-absorption). Returned by `BarEditorApp::smf_lighting` and
/// consumed by `bar-app` to populate the renderer's per-frame
/// uniforms. There is exactly one shape for this; `bar-render`
/// defines it and we re-export the same type so the GUI, the
/// renderer, and the CLI cannot drift out of sync on field order
/// or units. (Previously a copy lived here as
/// `SmfLightingSnapshot`, the renderer had its own
/// `SmfLighting`, and the CLI handcopied between them — easy
/// for one site to forget a field.)
pub type SmfLightingSnapshot = bar_render::SmfLighting;

/// Recipe identity block: the values that show up in the Map Info /
/// About dialog and end up in the `.barproj` recipe header.
#[derive(Clone, Debug)]
pub struct RecipeMeta {
    /// Human-readable map name (`mapinfo.name`). The engine builds the
    /// archive identifier from `name .. " " .. version`, so this is
    /// the value players see and which the script's `MapName=` must
    /// match. When `None`, the bundler falls back to the `.barproj`
    /// directory stem -- only sensible for fresh projects that never
    /// had a source mapinfo to import a real name from.
    pub name: Option<String>,
    /// Optional shortname (`mapinfo.shortname`). Omitted from mapinfo.lua when `None`.
    pub shortname: Option<String>,
    /// Free-form description (`mapinfo.description`). Empty string is omitted.
    pub description: String,
    /// Optional author. Omitted from mapinfo.lua when `None`.
    pub author: Option<String>,
    /// Optional version string. Omitted from mapinfo.lua when `None`.
    /// When set, becomes part of the Spring archive identity: `name .. " " .. version`.
    pub version: Option<String>,
    /// Optional short tooltip text shown by the lobby on hover.
    /// Becomes mapinfo's `tip`. Omitted when `None`.
    pub tip: Option<String>,
    /// Archive dependencies (`mapinfo.depend`). Default is
    /// `["Map Helper v1"]`; rarely changed.
    pub depend: Vec<String>,
}

impl Default for RecipeMeta {
    fn default() -> Self {
        Self {
            name: None,
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            tip: None,
            depend: vec!["Map Helper v1".to_string()],
        }
    }
}

/// Project map metadata + UI shadow state. See module docs.
#[derive(Default, Debug, Clone)]
pub struct MapState {
    /// Map width in samples (heightmap pixels). Shadows
    /// `settings.map_width_in_chunks * 64 + 1` for direct widget bind.
    pub width: u32,
    pub height: u32,
    /// Spring world-unit height range. Shadows `settings.min_height` /
    /// `settings.max_height` for direct widget bind in the Map
    /// Settings modal.
    pub min_height: f32,
    pub max_height: f32,
    /// Live MapSettings being edited. The .barproj recipe persists
    /// this verbatim.
    pub settings: MapSettings,
    /// Recipe identity (shortname / description / author / version) as
    /// shown in the Map Info / About dialog.
    pub recipe_meta: RecipeMeta,
    /// Index of the spawn marker currently being dragged in the 2D
    /// inspector (None if no drag in progress).
    pub dragging_spawn: Option<usize>,
    /// Feature placements (imported from .sd7 and/or placed by the user).
    pub features: Vec<PlacedFeature>,
    /// Index into `features` of the currently selected feature (for deletion).
    pub selected_feature_idx: Option<usize>,
    /// Set to true when features are added/removed via the placement tool.
    /// Consumed by the layout manager to trigger a GPU instance rebuild.
    pub features_placement_dirty: bool,
}

impl MapState {
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn dimensions_mut(&mut self) -> (&mut u32, &mut u32) {
        (&mut self.width, &mut self.height)
    }

    pub fn height_range(&self) -> (f32, f32) {
        (self.min_height, self.max_height)
    }

    pub fn height_range_mut(&mut self) -> (&mut f32, &mut f32) {
        (&mut self.min_height, &mut self.max_height)
    }

    pub fn settings_mut(&mut self) -> &mut MapSettings {
        &mut self.settings
    }

    pub fn recipe_meta_mut(&mut self) -> &mut RecipeMeta {
        &mut self.recipe_meta
    }
}

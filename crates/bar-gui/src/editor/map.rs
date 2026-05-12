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
/// `SmfLighting`. Lives in `bar-gui` so callers can read it without
/// pulling in `bar-render` as a transitive dep.
#[derive(Clone, Copy, Debug)]
pub struct SmfLightingSnapshot {
    pub sun_dir: [f32; 3],
    pub ground_ambient: [f32; 3],
    pub ground_diffuse: [f32; 3],
    pub ground_specular: [f32; 3],
    pub specular_exponent: f32,
    pub water_absorb: [f32; 3],
    pub water_base: [f32; 3],
    pub water_min: [f32; 3],
}

/// Recipe identity block: the values that show up in the Map Info /
/// About dialog and end up in the `.barproj` recipe header.
#[derive(Default, Clone, Debug)]
pub struct RecipeMeta {
    /// Optional shortname (`mapinfo.shortname`). Omitted from mapinfo.lua when `None`.
    pub shortname: Option<String>,
    /// Free-form description (`mapinfo.description`). Empty string is omitted.
    pub description: String,
    /// Optional author. Omitted from mapinfo.lua when `None`.
    pub author: Option<String>,
    /// Optional version string. Omitted from mapinfo.lua when `None`.
    /// When set, becomes part of the Spring archive identity: `name .. " " .. version`.
    pub version: Option<String>,
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
    /// Feature placements preserved from the last .sd7 import.
    /// Editable in the sculpt view in a future iteration.
    pub features: Vec<PlacedFeature>,
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

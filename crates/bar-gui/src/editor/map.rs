//! Project-level map metadata: dimensions, MapSettings, recipe
//! identity, and spawn-marker drag state.
//!
//! `MapSettings` is the source of truth (it's what `validate_project`
//! reads and what gets serialised in the .barproj recipe). The
//! `width`/`height`/`min_h`/`max_h` shadow fields are bound to egui
//! widgets in the Map Settings modal and the `mapinfo_editor` panel;
//! they're kept in sync with `settings` whenever either side is
//! edited.

use bar_project::MapSettings;

use crate::app::RecipeMeta;

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

//! Pack and unpack the sculpt-overlay sidecar layer.
//!
//! When the user saves a project, any in-memory sculpt deltas
//! (height, metal, type, texture) are serialised to PNG sidecar
//! files under `<project>.assets/`. Saved as `bar://...` URLs
//! inside a `bar_project::SculptRecord` and resolved back to disk
//! paths on load via `crate::project::path::resolve_project_path`.

use crate::app::BarEditorApp;
use crate::io::png::{
    load_color_buffer_from_png, load_heightmap_from_png16, load_heightmap_from_png16_biased,
    save_color_buffer_as_png, save_heightmap_as_png16, save_heightmap_as_png16_biased,
};
use crate::project::path::{resolve_project_path, PROJECT_RELATIVE_PREFIX};

impl BarEditorApp {
    /// Save sculpt overlays (height delta, metal, type, texture) as
    /// PNG files in `assets_dir`, then stash a `SculptRecord` carrying
    /// project-relative `bar://` URLs in `pending_sculpt_record`.
    /// No-op when no sculpt data exists.
    pub(crate) fn pack_sculpt_record(
        &mut self,
        assets_dir: &std::path::Path,
        project_dir: &std::path::Path,
    ) -> Result<(), String> {
        if !self.paint.sculpt.dirty {
            return Ok(());
        }
        let assets_name = assets_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("assets")
            .to_string();
        let bar_url =
            |name: &str| -> String { format!("{PROJECT_RELATIVE_PREFIX}{assets_name}/{name}") };
        std::fs::create_dir_all(assets_dir).map_err(|e| format!("create assets dir: {e}"))?;

        let mut record = bar_project::SculptRecord::default();

        if let Some(ref hm) = self.paint.sculpt.height_delta {
            let p = assets_dir.join("sculpt-height.png");
            save_heightmap_as_png16_biased(hm, &p)?;
            record.height = Some(bar_url("sculpt-height.png"));
        }
        if let Some(ref hm) = self.paint.sculpt.metal_overlay {
            let p = assets_dir.join("sculpt-metal.png");
            save_heightmap_as_png16(hm, &p)?;
            record.metal = Some(bar_url("sculpt-metal.png"));
        }
        if let Some(ref hm) = self.paint.sculpt.type_overlay {
            let p = assets_dir.join("sculpt-type.png");
            save_heightmap_as_png16(hm, &p)?;
            record.type_map = Some(bar_url("sculpt-type.png"));
        }
        if let Some(ref cb) = self.paint.sculpt.texture_overlay {
            let p = assets_dir.join("sculpt-texture.png");
            save_color_buffer_as_png(cb, &p)?;
            record.texture = Some(bar_url("sculpt-texture.png"));
        }

        self.paint.pending_sculpt_record = Some(record);
        let _ = project_dir;
        Ok(())
    }

    /// Returns the most recently packed `SculptRecord` and its project
    /// directory for use by export threads. Returns `None` when the
    /// project has never been saved (no record exists) or has no path
    /// on disk.
    pub fn sculpt_export_snapshot(
        &self,
    ) -> Option<(bar_project::SculptRecord, std::path::PathBuf)> {
        let record = self.paint.pending_sculpt_record.as_ref()?.clone();
        let dir = self.project.path.as_ref()?.parent()?.to_path_buf();
        Some((record, dir))
    }

    /// Restore sculpt layers from a loaded `SculptRecord`. Resolves
    /// `bar://` URLs against `project_dir` and populates `paint.sculpt`.
    /// Missing or unreadable sidecar files are skipped with a warning.
    pub(crate) fn unpack_sculpt_record(
        &mut self,
        record: &bar_project::SculptRecord,
        project_dir: &std::path::Path,
    ) {
        let resolve = |url: &str| -> String { resolve_project_path(url, project_dir) };

        if let Some(ref url) = record.height {
            match load_heightmap_from_png16_biased(std::path::Path::new(&resolve(url))) {
                Ok(hm) => self.paint.sculpt.height_delta = Some(hm),
                Err(e) => tracing::warn!("sculpt height sidecar unreadable: {e}"),
            }
        }
        if let Some(ref url) = record.metal {
            match load_heightmap_from_png16(std::path::Path::new(&resolve(url))) {
                Ok(hm) => self.paint.sculpt.metal_overlay = Some(hm),
                Err(e) => tracing::warn!("sculpt metal sidecar unreadable: {e}"),
            }
        }
        if let Some(ref url) = record.type_map {
            match load_heightmap_from_png16(std::path::Path::new(&resolve(url))) {
                Ok(hm) => self.paint.sculpt.type_overlay = Some(hm),
                Err(e) => tracing::warn!("sculpt type sidecar unreadable: {e}"),
            }
        }
        if let Some(ref url) = record.texture {
            match load_color_buffer_from_png(std::path::Path::new(&resolve(url))) {
                Ok(cb) => self.paint.sculpt.texture_overlay = Some(cb),
                Err(e) => tracing::warn!("sculpt texture sidecar unreadable: {e}"),
            }
        }
        if self.paint.sculpt.height_delta.is_some()
            || self.paint.sculpt.metal_overlay.is_some()
            || self.paint.sculpt.type_overlay.is_some()
            || self.paint.sculpt.texture_overlay.is_some()
        {
            self.paint.sculpt.dirty = false;
        }
    }
}

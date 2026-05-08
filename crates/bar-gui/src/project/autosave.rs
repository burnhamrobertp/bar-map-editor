//! Periodic autosave to a sidecar file. Distributed
//! `impl BarEditorApp` block.
//!
//! Autosave is best-effort: it never modifies `is_dirty`, never
//! touches `project.path`, and never enters the recent files list.
//! It writes to `<project>.autosave` next to the live `.barproj`
//! when one exists, or to the platform autosave dir for an untitled
//! project.

use std::time::Instant;

use crate::app::BarEditorApp;
use crate::settings::Settings;

impl BarEditorApp {
    /// Auto-save the current project to a sidecar file
    /// (`<project>.autosave`) when a project file path is set, or to
    /// the platform autosave dir when the project is untitled.
    /// Best-effort: never updates is_dirty, never touches
    /// project_path, never enters the recent files list.
    pub(crate) fn autosave_now(&mut self) {
        if !self.project.is_dirty {
            return;
        }
        let target = match self.project.path.as_ref() {
            Some(p) => {
                let mut q = p.clone();
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("project");
                q.set_file_name(format!("{name}.autosave"));
                Some(q)
            }
            None => Settings::autosave_dir().map(|d| {
                let _ = std::fs::create_dir_all(&d);
                d.join("untitled.barproj.autosave")
            }),
        };
        let Some(target) = target else {
            return;
        };
        let project = self.build_project(&target);
        match project.save(&target) {
            Ok(()) => {
                self.project.last_autosave_at = Some(Instant::now());
                self.dialog.toast = Some((
                    "Autosaved".to_string(),
                    Instant::now() + std::time::Duration::from_secs(2),
                ));
            }
            Err(e) => {
                tracing::warn!(?target, error = %e, "Autosave failed");
            }
        }
    }
}

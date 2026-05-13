//! Periodic autosave to the project's `autosave/` subdirectory. Distributed
//! `impl BarEditorApp` block.
//!
//! Autosave only runs when a `.barproj` directory path is set
//! (`project.path` is Some). Opening a `.sd7` or working on a brand-new
//! unsaved project does not trigger autosave.
//!
//! Each autosave writes `autosave/recipe_<timestamp>.json` via
//! `PackageDir::save_autosave`. Rolling pruning keeps at most
//! `settings.autosave_slot_count` files (oldest removed first).
//!
//! Autosave is best-effort: it never modifies `is_dirty`, never
//! touches `project.path`, and never enters the recent files list.

use std::time::Instant;

use bar_project::PackageDir;

use crate::app::BarEditorApp;
use crate::t;

impl BarEditorApp {
    /// Auto-save the current project recipe to `<project>.barproj/autosave/`.
    ///
    /// No-op when:
    ///   - the project has no unsaved changes (`is_dirty == false`), or
    ///   - no `.barproj` directory path is set yet.
    pub(crate) fn autosave_now(&mut self) {
        if !self.project.is_dirty {
            return;
        }
        let Some(project_path) = self.project.path.clone() else {
            return;
        };

        let pkg = match PackageDir::open(&project_path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "Autosave: cannot open package dir");
                return;
            }
        };

        let project = self.build_project(&project_path);
        let recipe_json = match project.recipe_to_json() {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!(error = %e, "Autosave: recipe serialisation failed");
                return;
            }
        };

        match pkg.save_autosave(&recipe_json) {
            Ok(()) => {
                let keep = self.settings.autosave_slot_count.max(1) as usize;
                pkg.prune_autosaves(keep);
                self.project.last_autosave_at = Some(Instant::now());
                self.dialog.toast = Some((
                    t!("editor.project.autosaved"),
                    Instant::now() + std::time::Duration::from_secs(2),
                ));
            }
            Err(e) => {
                tracing::warn!(error = %e, "Autosave failed");
            }
        }
    }
}

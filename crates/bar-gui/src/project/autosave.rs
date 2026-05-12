//! Periodic autosave to rotating sidecar files. Distributed
//! `impl BarEditorApp` block.
//!
//! Autosave only runs when a `.barproj` path is set (`project.path`
//! is Some). Opening a `.sd7` or working on a brand-new unsaved
//! project does not trigger autosave -- there is no project file to
//! save alongside, and writing an anonymous dump to the autosave dir
//! provides no reliable recovery path.
//!
//! When a project path is set, writes rotate through N slots:
//!   `<project>.autosave1`, `<project>.autosave2`, ... `<project>.autosaveN`
//! N is `settings.autosave_slot_count` (default 3, configurable in
//! Preferences). The oldest slot is always overwritten next so the
//! most recent N writes are always recoverable. The slot counter
//! resets to 0 whenever a new project is opened.
//!
//! Autosave is best-effort: it never modifies `is_dirty`, never
//! touches `project.path`, and never enters the recent files list.

use std::time::Instant;

use crate::app::BarEditorApp;
use crate::t;

impl BarEditorApp {
    /// Auto-save the current project to the next rotating sidecar slot.
    ///
    /// No-op when:
    ///   - the project has no unsaved changes (`is_dirty == false`), or
    ///   - no `.barproj` path is set yet (new project or unsaved `.sd7` import).
    pub(crate) fn autosave_now(&mut self) {
        if !self.project.is_dirty {
            return;
        }
        let Some(project_path) = self.project.path.clone() else {
            return;
        };

        let slot_count = self.settings.autosave_slot_count.max(1);
        let slot = self.project.autosave_slot;
        self.project.autosave_slot = (slot + 1) % slot_count;
        let slot_num = slot + 1; // 1-indexed in filename

        let name = project_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("project");
        let mut target = project_path.clone();
        target.set_file_name(format!("{name}.autosave{slot_num}"));

        let project = self.build_project(&target);
        match project.save(&target) {
            Ok(()) => {
                self.project.last_autosave_at = Some(Instant::now());
                self.dialog.toast = Some((
                    t!("editor.project.autosaved"),
                    Instant::now() + std::time::Duration::from_secs(2),
                ));
            }
            Err(e) => {
                tracing::warn!(?target, error = %e, "Autosave failed");
            }
        }
    }
}

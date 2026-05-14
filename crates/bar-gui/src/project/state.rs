//! Project lifecycle state owned by `BarEditorApp`.
//!
//! Holds where the project is on disk, whether it has unsaved
//! changes, the autosave timer, and the receivers for the
//! background tasks that feed project loads (file-open dialog
//! polling and `.sd7` extraction). Reset together when a project
//! is closed or replaced.

use std::time::Instant;

use crate::app::PassthroughEdit;

/// Grouped project lifecycle state. See module docs.
#[derive(Default, Debug)]
pub struct ProjectState {
    /// Absolute path to the loaded `.barproj` file (None until the
    /// project has been saved at least once).
    pub path: Option<std::path::PathBuf>,
    /// Display name shown in the title bar. Mirrors `path`'s stem for
    /// project files; for `.sd7` opens it holds the imported map's
    /// name until the user saves the project.
    pub loaded_name: Option<String>,
    /// `true` whenever the editor state diverges from what's on disk.
    /// Drives the title bar's dirty asterisk and the unsaved-changes
    /// confirmation dialog.
    pub is_dirty: bool,
    /// Timestamp of the last successful autosave, used as the gate
    /// for the autosave-interval timer.
    pub last_autosave_at: Option<Instant>,
    /// Bundle path (archive-relative, forward slashes) of the file
    /// the user has designated as the project's map-info file. None
    /// means the user hasn't picked one yet.
    pub map_info_file: Option<String>,
    /// Set by `bar-app` after a `.sd7` extraction lands on disk; the
    /// editor's per-frame poll picks it up and opens the resulting
    /// project.
    pub sd7_open_request: Option<std::path::PathBuf>,
    /// Receiver for an in-flight Open dialog. The native file dialog
    /// runs on a worker thread and the result lands here.
    pub pending_open_rx: Option<std::sync::mpsc::Receiver<Option<std::path::PathBuf>>>,
    /// Active inline text editor (PassThrough or Map Info file body).
    pub passthrough_edit: Option<PassthroughEdit>,
    /// Pulsed `true` whenever the graph is replaced (new map / project
    /// open). `bar-app` consumes this via `take_graph_reset` to flush
    /// GPU preview state.
    pub graph_reset: bool,
    /// `true` when the graph or params have changed since the last
    /// successful compile. Cleared by `bar-app` after `compile_project`
    /// succeeds.
    pub compile_dirty: bool,
    /// Pulsed `true` when features are populated from a new project load
    /// (barproj or sd7). `bar-app` consumes this to trigger S3O model loading.
    pub features_changed: bool,
    /// Timestamp of the last successful compile in this session. `None`
    /// until the user has compiled at least once.
    pub compiled_at: Option<Instant>,
    /// Absolute path to the SD7 archive the current map was imported from.
    /// Restored from `recipe.source_sd7` on barproj load so the model
    /// loader can find the map work dir without a fresh re-import.
    pub source_sd7: Option<std::path::PathBuf>,
}

impl ProjectState {
    pub fn is_dirty(&self) -> bool {
        self.is_dirty
    }

    pub fn loaded_name(&self) -> Option<&str> {
        self.loaded_name.as_deref()
    }

    /// Consume the one-frame "graph was replaced" pulse. Returns
    /// `true` once after a project load, then resets.
    pub fn take_graph_reset(&mut self) -> bool {
        std::mem::take(&mut self.graph_reset)
    }
}

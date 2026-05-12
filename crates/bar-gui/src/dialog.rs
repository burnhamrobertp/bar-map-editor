//! Modal / popup / transient-feedback UI state.
//!
//! Anything that's "is some dialog or transient overlay currently
//! visible / queued?" lives here. The state struct (`DialogState`) is
//! a sub-state on `BarEditorApp`; the supporting enums describe
//! pending-action queues, confirm-dialog payloads, and the inline
//! file-editor state.

use std::time::Instant;

use crate::editor::PendingPropsOpen;
use crate::log::{LogBuffer, LogLevel};

/// Outcome of the "delete group" confirmation modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GroupDeleteChoice {
    /// Dissolve the group; member nodes stay where they are.
    GroupOnly,
    /// Dissolve the group AND delete its member nodes from the graph.
    GroupAndMembers,
    /// Close the dialog without changing anything.
    Cancel,
}

/// State for the inline text editor inside the PassThrough properties panel.
#[derive(Debug, Clone)]
pub struct PassthroughEdit {
    pub node_id: bar_graph::NodeId,
    pub abs_path: String,
    pub archive_path: String,
    pub content: String,
    pub is_dirty: bool,
}

/// Floating in-app text editor -- used by the Edit Map Info button and any
/// future "open this file" action. Lives outside the side panels so it can
/// be resized and scrolled freely.
#[derive(Debug, Clone)]
pub struct FileEditor {
    /// Absolute path on disk; what we read from and write back to.
    pub(crate) abs_path: String,
    /// Bundle-relative path (forward slashes) for display.
    pub(crate) archive_path: String,
    pub(crate) content: String,
    pub(crate) is_dirty: bool,
}

/// Action waiting on the user's response to an unsaved-changes confirmation.
/// Once the dialog resolves, the chosen action is performed.
#[derive(Clone, Debug)]
pub enum PendingAction {
    /// The OS or the user asked to close the window.
    Close,
    /// The user clicked New Project (Ctrl+N or menu).
    NewProject,
    /// The user picked an Open target (file path) and we need to load it.
    OpenPath(std::path::PathBuf),
    /// The user picked a built-in macro from the File menu.
    LoadMacro { name: String },
}

/// Generic yes/no/cancel modal state.
#[derive(Clone, Debug)]
pub struct ConfirmDialog {
    pub(crate) title: String,
    pub(crate) message: String,
    /// Action label for the affirmative button (e.g. "Delete", "Discard").
    pub(crate) affirm_label: String,
    /// What the affirmative button should trigger.
    pub(crate) on_affirm: ConfirmAction,
    /// When `Some`, render a "Don't ask again" checkbox.
    pub(crate) suppression_key: Option<String>,
    /// Live state of the "Don't ask again" checkbox.
    pub(crate) dont_ask_again: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum ConfirmAction {
    /// Delete the selected node (already captured in app state).
    DeleteSelected,
}

/// Result of the unsaved-changes modal.
#[derive(Clone, Copy, Debug)]
pub(crate) enum UnsavedDecision {
    Save,
    Discard,
    Cancel,
}

/// Suppression key for the "delete a connected node" confirm modal.
/// One per modal type -- extending: add a new const here, set it on
/// the dialog when opening, give it a display name in
/// `confirm_key_display_name`, and the preferences "clear" button
/// picks it up automatically.
pub(crate) const CONFIRM_KEY_DELETE_CONNECTED_NODE: &str = "delete_connected_node";

/// Friendly label for one of the confirmation keys.
pub(crate) fn confirm_key_display_name(key: &str) -> String {
    match key {
        CONFIRM_KEY_DELETE_CONNECTED_NODE => "Delete a node that has wires connected".to_string(),
        other => other.to_string(),
    }
}

#[derive(Default)]
pub struct DialogState {
    pub show_validation_panel: bool,
    pub show_inspector: bool,
    pub show_mapinfo_editor: bool,
    pub show_settings: bool,
    pub show_about: bool,
    /// True while the "pick which file is the map info" picker modal is open.
    pub show_map_info_picker: bool,
    /// True for one frame after the user accepts an unsaved-changes
    /// close so `bar-app` can let the window actually close.
    pub allow_close: bool,
    /// Generic confirm-dialog state (delete confirmation, etc.).
    pub(crate) confirm_dialog: Option<ConfirmDialog>,
    /// Pending action blocked on the unsaved-changes confirm dialog.
    /// `Some` means a modal is currently open.
    pub(crate) pending_action: Option<PendingAction>,
    /// In-app floating text editor (Edit Map Info / future "open
    /// file" triggers). `None` when no editor is open.
    pub(crate) file_editor: Option<FileEditor>,
    /// True when the log window is visible.
    pub(crate) show_log: bool,
    /// Level of the most recent status_message (drives footer color).
    pub(crate) status_level: LogLevel,
    /// Session-scoped ring buffer of all logged messages.
    pub(crate) log_buffer: LogBuffer,
    /// Transient toast message shown over the canvas.
    pub toast: Option<(String, Instant)>,
    /// Status bar message -- replaces toast for non-time-bound feedback.
    pub status_message: Option<String>,
    /// In-flight click waiting on the post-click hover gate before
    /// the contextual properties panel pops open.
    pub pending_props_open: Option<PendingPropsOpen>,
}

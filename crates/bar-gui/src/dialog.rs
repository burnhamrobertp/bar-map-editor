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
    /// True while the map-info editor modal is in an active session.
    /// Used by `panels::mapinfo_editor::draw` to detect the
    /// closed-to-open transition so it can capture a pre-edit
    /// snapshot exactly once per session (rather than every frame).
    /// Cleared when the modal closes.
    pub(crate) mapinfo_editor_session_active: bool,
    /// Snapshot captured the moment the map-info editor opened. Pushed
    /// onto the undo stack the first time any field is dirtied during
    /// this session (so opening + closing the modal without edits
    /// doesn't bloat undo history). Discarded if the user closes the
    /// modal without changing anything.
    pub(crate) mapinfo_editor_pending_undo: Option<crate::undo::Snapshot>,
    pub show_settings: bool,
    pub show_about: bool,
    /// True for one frame after the user accepts an unsaved-changes
    /// close so `bar-app` can let the window actually close.
    pub allow_close: bool,
    /// Generic confirm-dialog state (delete confirmation, etc.).
    pub(crate) confirm_dialog: Option<ConfirmDialog>,
    /// Pending action blocked on the unsaved-changes confirm dialog.
    /// `Some` means a modal is currently open.
    pub(crate) pending_action: Option<PendingAction>,
    /// True when the log window is visible.
    pub(crate) show_log: bool,
    /// Level filter for the log window (None = show all).
    pub(crate) log_level_filter: Option<LogLevel>,
    /// Text search filter for the log window.
    pub(crate) log_search: String,
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

//! Modal / popup / transient-feedback UI state.
//!
//! Anything that's "is some dialog or transient overlay currently
//! visible / queued?" lives here. The state struct (`DialogState`) is
//! a sub-state on `BarEditorApp`; the supporting enums describe
//! pending-action queues, confirm-dialog payloads, and the inline
//! file-editor state.

use std::time::Instant;

use crate::editor::PendingPropsOpen;
use crate::log::{LogBuffer, LogLevel, LogLevelVisibility};

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
        CONFIRM_KEY_DELETE_CONNECTED_NODE => {
            crate::t!("editor.prefs.confirmations.key.delete_connected_node")
        }
        other => other.to_string(),
    }
}

#[derive(Default)]
pub struct DialogState {
    pub show_validation_panel: bool,
    pub show_inspector: bool,
    // ── Action-bar modals ────────────────────────────────────────
    // Each `show_<name>_editor` toggles its own modal in
    // `panels::action_bar_modals`. They used to share a single
    // `show_mapinfo_editor` + `MapInfoTab`, but the tab strip is
    // gone -- every action-bar button now opens / closes its own
    // modal directly.
    pub show_identity_editor: bool,
    pub show_dimensions_editor: bool,
    pub show_physics_editor: bool,
    pub show_atmosphere_editor: bool,
    pub show_fog_editor: bool,
    pub show_lighting_editor: bool,
    pub show_water_editor: bool,
    pub show_resources_editor: bool,
    /// Bump strength for the surface-detail (detailNormalTex) picker. 0 = use
    /// the 1.0 default (Default derives 0; the panel lazily seeds it).
    pub detail_normal_strength: f32,
    pub show_grass_editor: bool,
    pub show_map_edge_editor: bool,
    /// Snapshot captured the moment a `render_field` widget began
    /// editing (drag started or text input gained focus). Pushed
    /// onto the undo stack when the same widget commits (drag
    /// stopped, lost focus, or atomic change like a checkbox
    /// toggle). One slot is enough because only one widget can be
    /// in active-edit state at a time -- if the user clicks a
    /// different field, that field's `gained_focus` fires AFTER the
    /// previous field's `lost_focus`, so the slot is empty before
    /// the next edit starts. See `panels::field_editor` for the
    /// state machine.
    pub(crate) field_edit_in_progress: Option<crate::undo::Snapshot>,
    /// Pre-drag snapshot for a spawn marker the user is currently
    /// dragging on the canvas inspector. Mirrors
    /// `field_edit_in_progress` -- one drag = one undo entry,
    /// captured at drag-start and pushed at drag-stop.
    pub(crate) spawn_drag_in_progress: Option<crate::undo::Snapshot>,
    /// Live mirror of the Layout edit view's currently-selected item,
    /// keyed by node id. Captured into the undo snapshot so undo / redo
    /// can re-select the affected shape on restore. Updated every frame
    /// the layout editor runs; consumed (and cleared) when applied to
    /// the editor's canvas state on the frame after a restore.
    pub(crate) layout_selection_hint: Option<(bar_graph::NodeId, Option<usize>)>,
    /// Active creation tool for the Layout edit view's canvas: which
    /// kind of shape a drag-to-create gesture produces. One of
    /// `ellipse` / `rectangle` / `ridge` / `spline`. Defaults to
    /// `ellipse`. Session-scoped; not persisted across project loads.
    pub(crate) layout_creation_tool: Option<String>,
    /// Whether the Assemble Map wizard is currently open. The wizard's
    /// per-page state (current page, accumulated picks) lives on
    /// [`crate::panels::assemble_map::AssembleMapState`].
    pub show_assemble_map: bool,
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
    /// Per-level visibility toggles for the log window. Each level is
    /// independently shown or hidden; default is "show all". The button
    /// row in `panels::log` flips the matching entry on click.
    pub(crate) log_levels_visible: LogLevelVisibility,
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

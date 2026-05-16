//! Preview-and-export state owned by `BarEditorApp`.
//!
//! Tracks one-frame "run" / "test in BAR" / "compile" pulses that
//! `bar-app` polls each frame to kick off background jobs.
//!
//! These all reset to defaults on project switch (cleared by
//! `BarEditorApp::reset_session_state` via `PreviewState::reset`).

use bar_graph::NodeId;

/// UI state for the BAR version picker (game archive + engine binary).
/// Populated at startup by `bar-app` once the install is detected.
/// Indices are safe to use as-is: `launch_skirmish` bounds-checks them.
#[derive(Debug, Clone, Default)]
pub struct BarVersionState {
    /// Display labels for available game archives. `game_labels[0]` is
    /// always "byar:stable (rapid)"; subsequent entries are local archives.
    pub game_labels: Vec<String>,
    /// Display labels for available engine versions, newest first.
    pub engine_labels: Vec<String>,
    pub selected_game: usize,
    pub selected_engine: usize,
}

impl BarVersionState {
    /// True when there are multiple game or engine versions to choose from,
    /// which is when the dropdown chevron is shown on the BAR button.
    pub fn has_choice(&self) -> bool {
        self.game_labels.len() > 1 || self.engine_labels.len() > 1
    }
}

/// Live export busy state. `bar-app` updates this each frame so the
/// GUI can render busy state on the bundle buttons (and gate clicks).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportStatus {
    #[default]
    Idle,
    /// "Run all" pressed; every Bundler is exporting.
    All,
    /// One specific Bundler is exporting; others remain idle.
    One(NodeId),
}

impl ExportStatus {
    /// True when any export is currently running.
    pub fn is_running(self) -> bool {
        !matches!(self, ExportStatus::Idle)
    }

    /// True when the export currently running affects this node.
    pub fn affects(self, id: NodeId) -> bool {
        matches!(self, ExportStatus::All) || matches!(self, ExportStatus::One(n) if n == id)
    }
}

/// Grouped editor preview / export state. See module docs.
#[derive(Default, Debug, Clone)]
pub struct PreviewState {
    /// Set to `true` for one frame when the user clicks the toolbar
    /// "Run" button. `bar-app` consumes this via `take_run_requested`.
    pub run_requested: bool,
    /// Set to `true` for one frame when the user clicks "Test in BAR".
    /// `bar-app` consumes via `take_test_in_bar`.
    pub test_in_bar_requested: bool,
    /// Set when the user runs a single bundler node (rather than all).
    /// `bar-app` consumes via `take_run_export_node`.
    pub run_export_node: Option<NodeId>,
    /// Live export busy-state. Set by `bar-app` to gate the run buttons
    /// in the GUI.
    pub export_status: ExportStatus,
    /// Set to `true` for one frame when the user clicks "Compile".
    /// `bar-app` consumes via `take_compile_requested`.
    pub compile_requested: bool,
    /// True while a compile is running. Set by `bar-app`.
    pub compile_running: bool,
    /// Set to `true` for one frame when the Preview layout wants bar-app
    /// to load the compiled SMT as a BC1 GPU texture. Consumed via
    /// `take_bc_texture_requested`.
    pub bc_texture_requested: bool,
}

impl PreviewState {
    /// Consume the one-frame "run" pulse. Returns `true` once after
    /// the user clicks Run, then resets.
    pub fn take_run_requested(&mut self) -> bool {
        std::mem::take(&mut self.run_requested)
    }

    /// Consume the one-frame "test in BAR" pulse.
    pub fn take_test_in_bar(&mut self) -> bool {
        std::mem::take(&mut self.test_in_bar_requested)
    }

    /// Consume the one-frame "run this specific bundler node" pulse.
    pub fn take_run_export_node(&mut self) -> Option<NodeId> {
        self.run_export_node.take()
    }

    pub fn export_status(&self) -> ExportStatus {
        self.export_status
    }

    pub fn set_export_status(&mut self, s: ExportStatus) {
        self.export_status = s;
    }

    /// Consume the one-frame "compile" pulse.
    pub fn take_compile_requested(&mut self) -> bool {
        std::mem::take(&mut self.compile_requested)
    }

    /// Consume the one-frame "load BC1 texture" pulse.
    pub fn take_bc_texture_requested(&mut self) -> bool {
        std::mem::take(&mut self.bc_texture_requested)
    }

    /// Reset to defaults. Called by `BarEditorApp::reset_session_state`
    /// when a project is closed or replaced -- keeps `dialog`, `paint`,
    /// and other concerns from leaking across project boundaries.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

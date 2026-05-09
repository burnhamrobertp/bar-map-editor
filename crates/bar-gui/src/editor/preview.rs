//! Preview-and-export state owned by `BarEditorApp`.
//!
//! Tracks which node drives the 3D viewport, whether the viewport is
//! visible, and the one-frame "run" / "test in BAR" pulses that
//! `bar-app` polls each frame to kick off background export jobs.
//!
//! These all reset to defaults on project switch (cleared by
//! `BarEditorApp::reset_session_state` via `PreviewState::reset`).

use bar_graph::NodeId;

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
    /// Is the preview window open?
    pub open: bool,
    /// Which node feeds the 3D viewport. `None` => the renderer shows
    /// the empty/no-mesh state.
    pub node: Option<NodeId>,
    /// Set to `true` for one frame when the user clicks the toolbar
    /// "Run" button. `bar-app` consumes this via `take_run_requested`.
    pub run_requested: bool,
    /// Set to `true` for one frame when the user clicks "Test in BAR".
    /// `bar-app` consumes via `take_test_in_bar`.
    pub test_in_bar_requested: bool,
    /// Set when the user runs a single bundler node (rather than all).
    /// `bar-app` consumes via `take_run_bundler_node`.
    pub run_bundler_node: Option<NodeId>,
    /// Live export busy-state. Set by `bar-app` to gate the run buttons
    /// in the GUI.
    pub export_status: ExportStatus,
}

impl PreviewState {
    /// Whether the preview window is currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_open(&mut self, v: bool) {
        self.open = v;
    }

    pub fn node(&self) -> Option<NodeId> {
        self.node
    }

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
    pub fn take_run_bundler_node(&mut self) -> Option<NodeId> {
        self.run_bundler_node.take()
    }

    pub fn export_status(&self) -> ExportStatus {
        self.export_status
    }

    pub fn set_export_status(&mut self, s: ExportStatus) {
        self.export_status = s;
    }

    /// Reset to defaults. Called by `BarEditorApp::reset_session_state`
    /// when a project is closed or replaced -- keeps `dialog`, `paint`,
    /// and other concerns from leaking across project boundaries.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

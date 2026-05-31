//! Floating contextual properties popup state.
//!
//! Tracks which graph object (node / group / connection) the popup
//! is bound to and its last-known on-screen rect (used for click-
//! outside dismissal). The hover-gate that decides *when* to open
//! the popup lives in `DialogState::pending_props_open`.

use bar_graph::NodeId;
use std::time::Instant;

/// What the contextual Properties panel is currently editing. Each
/// variant resolves to a screen-space rect at render time so the
/// panel can anchor itself relative to the target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropsTarget {
    Node(NodeId),
    Group(u64),
}

impl PropsTarget {
    /// Stable per-target value used to seed the popup's egui Id so
    /// switching between targets doesn't reuse the same window state.
    pub(crate) fn id_hash(&self) -> u64 {
        match self {
            PropsTarget::Node(n) => n.0,
            PropsTarget::Group(g) => g ^ 0xA5A5_A5A5_A5A5_A5A5,
        }
    }
}

/// In-flight gate for opening the contextual Properties panel.
#[derive(Clone, Debug)]
pub struct PendingPropsOpen {
    pub target: PropsTarget,
    pub armed_at: Instant,
    pub armed_pos: eframe::egui::Pos2,
}

/// Delay between releasing a click on a target and the panel opening.
/// Tuned to feel "intentional, not trigger-happy".
pub(crate) const PROPS_OPEN_DELAY_MS: u64 = 100;
/// Maximum cursor drift, in screen pixels, allowed during the
/// post-click hover before the gate resets.
pub(crate) const PROPS_OPEN_MOVE_TOLERANCE: f32 = 4.0;

/// Grouped properties-popup state. See module docs.
#[derive(Default, Debug, Clone)]
pub struct PropsPanelState {
    /// What the popup is currently bound to (node id, group id, or
    /// connection endpoints). `None` means no popup is open.
    pub active: Option<PropsTarget>,
    /// On-screen rect of the popup as drawn last frame. Used by the
    /// click-outside dismissal logic.
    pub active_rect: Option<eframe::egui::Rect>,
    /// Set by the popup's own close affordance (the top-right ✕ next to
    /// the name field) or by actions that supersede the popup, e.g.
    /// descending into a node's edit view. Consumed in
    /// `tick_props_panel` after the panel is drawn.
    pub close_requested: bool,
}

impl PropsPanelState {
    pub fn close(&mut self) {
        self.active = None;
        self.active_rect = None;
        self.close_requested = false;
    }
}

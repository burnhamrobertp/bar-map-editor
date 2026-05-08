//! Floating contextual properties popup state.
//!
//! Tracks which graph object (node / group / connection) the popup
//! is bound to and its last-known on-screen rect (used for click-
//! outside dismissal). The hover-gate that decides *when* to open
//! the popup lives in `DialogState::pending_props_open`.

use crate::app::PropsTarget;

/// Grouped properties-popup state. See module docs.
#[derive(Default, Debug, Clone)]
pub(crate) struct PropsPanelState {
    /// What the popup is currently bound to (node id, group id, or
    /// connection endpoints). `None` means no popup is open.
    pub active: Option<PropsTarget>,
    /// On-screen rect of the popup as drawn last frame. Used by the
    /// click-outside dismissal logic.
    pub active_rect: Option<eframe::egui::Rect>,
}

impl PropsPanelState {
    pub fn close(&mut self) {
        self.active = None;
        self.active_rect = None;
    }
}

//! Canvas viewport + interaction state.
//!
//! Holds the pan offset, the open tabs (Main + drilldowns into
//! collapsed subgraphs), the active and previous tab indices, the
//! cached canvas rect from the previous frame (used by drop-target
//! tests during palette drag), and the in-progress marquee /
//! drag-connection state.

use eframe::egui;

use crate::app::{CanvasView, DragConnection};

/// Grouped canvas viewport + interaction state. See module docs.
#[derive(Debug, Clone)]
pub(crate) struct CanvasState {
    /// Pan offset of the canvas in screen pixels.
    pub offset: egui::Vec2,
    /// Canvas rect from the previous frame -- used by palette drag
    /// to detect drops onto the canvas.
    pub rect_last: egui::Rect,
    /// Open canvas tabs. Index 0 is always `CanvasView::Main` and
    /// can't be closed. Drilldowns into collapsed subgraphs append
    /// here and close via the small x on each tab.
    pub tabs: Vec<CanvasView>,
    /// Index into `tabs`. Always valid: `tabs.len() > 0` and
    /// `active_tab < tabs.len()`.
    pub active_tab: usize,
    /// Tab the user was on before the current one. Ctrl+Tab swaps
    /// `active_tab` and this -- the conventional "back to where I
    /// was" shortcut.
    pub last_active_tab: usize,
    /// Set when a project-creation path needs an "everything" Auto
    /// Layout AFTER the canvas has rendered at least once. Consumed
    /// in `draw_node_graph` once `rect_last` is fresh.
    pub pending_auto_layout_all: bool,
    /// Anchor point of an in-progress marquee selection. Set when
    /// the user starts a primary-button drag on empty canvas;
    /// cleared on drag-stopped.
    pub marquee_start: Option<egui::Pos2>,
    /// In-progress port drag for wire creation. Output ports always
    /// emit from a Right placement, so the wire's tangent at the
    /// source end is always +X.
    pub drag_connection: Option<DragConnection>,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            offset: egui::Vec2::ZERO,
            rect_last: egui::Rect::NOTHING,
            tabs: vec![CanvasView::Main],
            active_tab: 0,
            last_active_tab: 0,
            pending_auto_layout_all: false,
            marquee_start: None,
            drag_connection: None,
        }
    }
}

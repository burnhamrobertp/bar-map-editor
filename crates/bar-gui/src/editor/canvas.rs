//! Canvas viewport + interaction state.
//!
//! Holds the pan offset, the open tabs (Main + drilldowns into
//! collapsed subgraphs), the active and previous tab indices, the
//! cached canvas rect from the previous frame (used by drop-target
//! tests during palette drag), and the in-progress marquee /
//! drag-connection state.

use bar_graph::NodeId;
use eframe::egui;

/// One tab in the canvas-area tab bar. The user can keep multiple
/// editing contexts open and switch between them. Tabs are
/// session-scoped state; they don't persist through save/load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanvasView {
    /// The whole graph. Always present, can't be closed.
    Main,
    /// Edit-in-isolation view of one sub-graph's contents -- the
    /// previous "confined edit mode" lifted into a tab so the user
    /// can keep the Main tab open alongside.
    SubGraph(u64),
}

/// In-progress port drag for wire creation. Output ports always emit
/// from a Right placement, so the wire's tangent at the source end is
/// always +X.
#[derive(Clone, Debug)]
pub struct DragConnection {
    pub from_node: NodeId,
    pub from_port: String,
    pub from_pos: egui::Pos2,
}

/// Grouped canvas viewport + interaction state. See module docs.
#[derive(Debug, Clone)]
pub struct CanvasState {
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

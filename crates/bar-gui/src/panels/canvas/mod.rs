//! Node graph canvas -- the centerpiece of the editor.
//!
//! Renders nodes, ports, wires, group frames, collapsed-subgraph
//! blocks, the canvas tabs, marquee selection, and palette drop
//! handling. Lives as distributed `impl BarEditorApp` blocks (rather
//! than free `pub(crate) fn draw(app, ...)` functions) because the
//! methods touch the full breadth of `BarEditorApp`'s state -- &mut
//! self grants what's needed without a wall of accessors.
//!
//! Sub-modules:
//! - `groups` -- group-frame rendering, collapsed subgraph blocks,
//!   group create / add / remove / dissolve.
//! - `layout` -- auto-layout of selected nodes / groups,
//!   port-position helpers, automatic wiring of compatible ports.
//! - `tabs` -- tab strip rendering, tab open / close / activate,
//!   "what's the active view" lookup.
//! - `render` -- `draw_node_graph`, the main per-frame render +
//!   interaction pass for the canvas itself.

use std::collections::HashMap;

use bar_graph::NodeId;
use eframe::egui;

use crate::app::DragConnection;
use crate::panels::tokens;

pub(crate) mod groups;
pub(crate) mod layout;
pub(crate) mod passthrough;
pub(crate) mod render;
pub(crate) mod sculpt_layers;
pub(crate) mod style;
pub(crate) mod tabs;

/// Return type of `draw_collapsed_subgraphs`: bounding rects keyed by
/// group id, external-port handle positions, and the in-progress
/// connection start/end if one was initiated this frame.
pub(crate) type CollapsedSubgraphsDraw = (
    HashMap<u64, egui::Rect>,
    HashMap<(NodeId, String), egui::Pos2>,
    Option<DragConnection>,
    Option<(NodeId, String)>,
);

/// Geometry and colour constants for the standard (non-IO) node body.
/// All node drawing passes read from here so magic numbers live in
/// one place.
pub(crate) struct NodeStyle {
    pub bg: egui::Color32,
    pub bg_sel: egui::Color32,
    pub bg_pri: egui::Color32,
    pub border: egui::Color32,
    pub border_sel: egui::Color32,
    pub border_w: f32,
    pub border_w_sel: f32,
    pub rounding: f32,
    pub title_h: f32,
    pub title_rounding: egui::CornerRadius,
}

impl NodeStyle {
    pub(crate) fn default() -> Self {
        Self {
            bg: tokens::NODE_BG,
            bg_sel: tokens::NODE_BG_SEL,
            bg_pri: tokens::NODE_BG_PRI,
            border: tokens::NODE_BORDER,
            border_sel: tokens::NODE_BORDER_SEL,
            border_w: 1.5,
            border_w_sel: 2.0,
            rounding: 4.0,
            title_h: 20.0,
            title_rounding: egui::CornerRadius {
                nw: 0,
                ne: 0,
                sw: 0,
                se: 0,
            },
        }
    }
}

/// Paint a port handle. Filled circle at `pos`; hover halo if the
/// pointer is currently over it.
pub(crate) fn draw_port_circle(
    painter: &egui::Painter,
    pos: egui::Pos2,
    radius: f32,
    color: egui::Color32,
    hovered: bool,
) {
    painter.circle_filled(pos, radius, color);
    if hovered {
        painter.circle_stroke(
            pos,
            radius + 2.5,
            egui::Stroke::new(
                1.5,
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 120),
            ),
        );
    }
}

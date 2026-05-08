//! Canvas tab strip + tab management. Distributed `impl BarEditorApp`
//! block.
//!
//! Tabs are session state (not persisted across project loads). The
//! Main tab is always at index 0 and can't be closed; SubGraph
//! drilldowns and any future Sculpt tabs append after it. Closing
//! the active tab activates the previous-active tab where possible.

use std::collections::HashMap;

use bar_graph::NodeId;
use eframe::egui;

use crate::app::*;
use crate::panels::tokens;
use crate::state::GroupRuntime;

impl BarEditorApp {

    /// Render the canvas tab bar across the top of the canvas area.
    /// Main is always present at index 0 and never moves or closes;
    /// other tabs (SubGraph, Sculpt) carry an `✕` close button and
    /// can be reordered by dragging horizontally.
    pub(crate) fn draw_canvas_tabs(&mut self, ui: &mut egui::Ui) {
        let mut switch_to: Option<usize> = None;
        let mut close: Option<usize> = None;
        let mut tab_rects: Vec<(usize, egui::Rect)> = Vec::new();
        let mut drag_release_index: Option<usize> = None;

        // Visual constants for the tab strip. Tabs are flat rects
        // with rounded top corners, joined to a baseline separator
        // that the active tab visually breaks. Inactive tabs sit a
        // pixel lower than the active one for that "depressed"
        // bottom-tab feel.
        let tab_height = 26.0_f32;
        let min_tab_width = 110.0_f32;
        let max_tab_width = 220.0_f32;
        let strip_h = tab_height + 1.0;
        let (strip_rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), strip_h),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(strip_rect);

        let neutral_active = tokens::TAB_BG_ACTIVE;
        let neutral_inactive = tokens::TAB_BG_INACTIVE;
        let neutral_hover = tokens::TAB_BG_HOVER;
        let baseline = tokens::TAB_BASELINE;

        // Pick the tab's tinted base colour. SubGraph tabs use their
        // group's palette colour so two SubGraphs with different
        // colours look visibly different in the tab strip. Main and
        // any tab whose target is missing fall back to neutral.
        let tab_tint =
            |view: &CanvasView, groups: &HashMap<u64, GroupRuntime>| -> Option<egui::Color32> {
                match view {
                    CanvasView::Main => None,
                    CanvasView::SubGraph(gid) => groups.get(gid).map(|g| group_color(g.color_idx)),
                }
            };

        // Baseline along the bottom of the strip.
        painter.line_segment(
            [
                egui::pos2(strip_rect.left(), strip_rect.bottom() - 0.5),
                egui::pos2(strip_rect.right(), strip_rect.bottom() - 0.5),
            ],
            egui::Stroke::new(1.0, baseline),
        );

        let pointer = ui.ctx().pointer_latest_pos();
        let mut x = strip_rect.left() + 2.0;
        for (i, view) in self.canvas.tabs.iter().enumerate() {
            let label = match view {
                CanvasView::Main => "Main".to_string(),
                CanvasView::SubGraph(gid) => self
                    .visuals.groups
                    .get(gid)
                    .map(|g| {
                        if g.label.is_empty() {
                            format!("SubGraph {gid}")
                        } else {
                            g.label.clone()
                        }
                    })
                    .unwrap_or_else(|| format!("SubGraph {gid}")),
            };
            let is_active = i == self.canvas.active_tab;
            let closable = i != 0;

            // Lay out the tab text + (optional) close button and
            // figure out the tab's width.
            let font = egui::FontId::proportional(13.0);
            let label_galley = painter.layout_no_wrap(
                label.clone(),
                font.clone(),
                if is_active {
                    tokens::TAB_LABEL_ACTIVE
                } else {
                    tokens::TAB_LABEL_DIM
                },
            );
            let close_w = if closable { 18.0 } else { 0.0 };
            let raw_w = label_galley.size().x + 24.0 + close_w;
            let tab_w = raw_w.clamp(min_tab_width, max_tab_width);

            let tab_rect = egui::Rect::from_min_size(
                egui::pos2(x, strip_rect.top()),
                egui::vec2(tab_w, tab_height),
            );
            tab_rects.push((i, tab_rect));

            let resp = ui.interact(
                tab_rect,
                egui::Id::new(("canvas_tab", i)),
                egui::Sense::click_and_drag(),
            );

            // Body fill. Tinted tabs use their group/node colour for
            // identity at a glance; Main keeps the neutral palette.
            // Active variants are brighter than inactive so the
            // currently-selected tab pops without losing its tint.
            let bg = match tab_tint(view, &self.visuals.groups) {
                Some(tint) => {
                    if is_active {
                        // Mix toward active_bg so the active tab still
                        // visually merges with the canvas below it.
                        blend(tint, neutral_active, 0.55)
                    } else if resp.hovered() {
                        blend(tint, neutral_hover, 0.55)
                    } else {
                        blend(tint, neutral_inactive, 0.65)
                    }
                }
                None => {
                    if is_active {
                        neutral_active
                    } else if resp.hovered() {
                        neutral_hover
                    } else {
                        neutral_inactive
                    }
                }
            };
            painter.rect_filled(
                tab_rect,
                egui::CornerRadius {
                    nw: 6,
                    ne: 6,
                    sw: 0,
                    se: 0,
                },
                bg,
            );
            // Side + top stroke. Skip the bottom side for the active
            // tab so it bleeds into the content below.
            let stroke = egui::Stroke::new(1.0, baseline);
            // Top
            painter.line_segment(
                [
                    egui::pos2(tab_rect.left() + 6.0, tab_rect.top()),
                    egui::pos2(tab_rect.right() - 6.0, tab_rect.top()),
                ],
                stroke,
            );
            // Top-left curve approximation
            painter.line_segment(
                [
                    egui::pos2(tab_rect.left(), tab_rect.top() + 6.0),
                    egui::pos2(tab_rect.left(), tab_rect.bottom()),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(tab_rect.right(), tab_rect.top() + 6.0),
                    egui::pos2(tab_rect.right(), tab_rect.bottom()),
                ],
                stroke,
            );
            // Cover the baseline under the active tab so it appears
            // joined to the content below. Use the active tab's own
            // bg colour so the join is invisible.
            if is_active {
                painter.line_segment(
                    [
                        egui::pos2(tab_rect.left() + 1.0, tab_rect.bottom() - 0.5),
                        egui::pos2(tab_rect.right() - 1.0, tab_rect.bottom() - 0.5),
                    ],
                    egui::Stroke::new(1.5, bg),
                );
            }

            // Truncate label if it would overflow the tab.
            let label_x = tab_rect.left() + 12.0;
            let label_y = tab_rect.center().y;
            let max_label_w = tab_rect.width() - 24.0 - close_w;
            if label_galley.size().x <= max_label_w {
                painter.galley(
                    egui::pos2(label_x, label_y - label_galley.size().y * 0.5),
                    label_galley,
                    egui::Color32::WHITE,
                );
            } else {
                // Render an ellipsis-truncated copy.
                let mut truncated = label.clone();
                while !truncated.is_empty() {
                    truncated.pop();
                    let test = format!("{truncated}…");
                    let galley =
                        painter.layout_no_wrap(test.clone(), font.clone(), egui::Color32::WHITE);
                    if galley.size().x <= max_label_w {
                        painter.galley(
                            egui::pos2(label_x, label_y - galley.size().y * 0.5),
                            galley,
                            egui::Color32::WHITE,
                        );
                        break;
                    }
                }
            }

            // Close button.
            if closable {
                let close_rect = egui::Rect::from_center_size(
                    egui::pos2(tab_rect.right() - 12.0, tab_rect.center().y),
                    egui::vec2(14.0, 14.0),
                );
                let close_resp = ui.interact(
                    close_rect,
                    egui::Id::new(("canvas_tab_close", i)),
                    egui::Sense::click(),
                );
                let close_color = if close_resp.hovered() {
                    tokens::SEVERITY_ERROR
                } else {
                    tokens::TAB_LABEL_DIM
                };
                let m = 4.0_f32;
                painter.line_segment(
                    [
                        egui::pos2(close_rect.left() + m, close_rect.top() + m),
                        egui::pos2(close_rect.right() - m, close_rect.bottom() - m),
                    ],
                    egui::Stroke::new(1.5, close_color),
                );
                painter.line_segment(
                    [
                        egui::pos2(close_rect.right() - m, close_rect.top() + m),
                        egui::pos2(close_rect.left() + m, close_rect.bottom() - m),
                    ],
                    egui::Stroke::new(1.5, close_color),
                );
                if close_resp.clicked() {
                    close = Some(i);
                }
            }

            // Tab interactions: click to switch, drag to reorder.
            if resp.clicked() {
                switch_to = Some(i);
            }
            if i != 0 && resp.drag_stopped_by(egui::PointerButton::Primary) {
                drag_release_index = Some(i);
            }

            x += tab_w;
            // Suppress unused warning when pointer is None.
            let _ = pointer;
        }

        if let Some(idx) = switch_to {
            self.set_active_tab(idx);
        }
        if let Some(idx) = close {
            self.close_tab(idx);
        }
        if let Some(from) = drag_release_index {
            if let Some(cursor) = ui.ctx().pointer_latest_pos() {
                let target = tab_rects
                    .iter()
                    .find(|(_, r)| r.contains(cursor))
                    .map(|(i, _)| *i)
                    .unwrap_or(from);
                let to = target.max(1);
                if to != from && to < self.canvas.tabs.len() {
                    let active_view = self.canvas.tabs.get(self.canvas.active_tab).cloned();
                    let item = self.canvas.tabs.remove(from);
                    self.canvas.tabs.insert(to, item);
                    if let Some(av) = active_view {
                        self.canvas.active_tab = self.canvas.tabs.iter().position(|v| v == &av).unwrap_or(0);
                    }
                }
            }
        }
    }

    /// Switch the active tab and remember the previously-active one
    /// for Ctrl+Tab back-and-forth. Use this everywhere the active
    /// tab changes — direct assignment to `active_tab` skips the
    /// last-active tracking.
    pub(crate) fn set_active_tab(&mut self, idx: usize) {
        if idx == self.canvas.active_tab || idx >= self.canvas.tabs.len() {
            return;
        }
        self.canvas.last_active_tab = self.canvas.active_tab;
        self.canvas.active_tab = idx;
    }

    /// Open a tab if not already open, then make it active. Returns
    /// the tab index. If a matching tab already exists, that one is
    /// reused — opening a SubGraph tab twice doesn't make two of
    /// them.
    pub(crate) fn open_or_activate_tab(&mut self, view: CanvasView) -> usize {
        if let Some(idx) = self.canvas.tabs.iter().position(|v| v == &view) {
            self.set_active_tab(idx);
            return idx;
        }
        self.canvas.tabs.push(view);
        let new_idx = self.canvas.tabs.len() - 1;
        self.set_active_tab(new_idx);
        new_idx
    }

    /// Close a tab by index. The Main tab (index 0) is never closed.
    /// If the closed tab was active, focus shifts to the previous
    /// tab (or Main if there's no previous).
    pub(crate) fn close_tab(&mut self, idx: usize) {
        if idx == 0 || idx >= self.canvas.tabs.len() {
            return;
        }
        self.canvas.tabs.remove(idx);
        if self.canvas.active_tab >= self.canvas.tabs.len() {
            self.canvas.active_tab = self.canvas.tabs.len() - 1;
        } else if self.canvas.active_tab > idx {
            self.canvas.active_tab -= 1;
        } else if self.canvas.active_tab == idx {
            // Closed the active one; pick the tab that was before it.
            self.canvas.active_tab = idx.saturating_sub(1);
        }
    }

    /// Drop tabs whose target no longer exists. Called whenever the
    /// graph or groups change so the tab bar can never display a
    /// reference to a deleted thing. The Main tab is preserved.
    pub(crate) fn prune_dangling_tabs(&mut self) {
        let valid_groups: std::collections::HashSet<u64> = self.visuals.groups.keys().copied().collect();
        let mut new_tabs: Vec<CanvasView> = Vec::with_capacity(self.canvas.tabs.len());
        let prev_active = self.canvas.tabs.get(self.canvas.active_tab).cloned();
        for tab in &self.canvas.tabs {
            let keep = match tab {
                CanvasView::Main => true,
                CanvasView::SubGraph(gid) => valid_groups.contains(gid),
            };
            if keep {
                new_tabs.push(tab.clone());
            }
        }
        if new_tabs.is_empty() {
            new_tabs.push(CanvasView::Main);
        }
        self.canvas.tabs = new_tabs;
        self.canvas.active_tab = match prev_active {
            Some(prev) => self.canvas.tabs.iter().position(|v| v == &prev).unwrap_or(0),
            None => 0,
        };
    }

    /// Returns the active tab's view.
    pub(crate) fn current_view(&self) -> CanvasView {
        self.canvas.tabs
            .get(self.canvas.active_tab)
            .cloned()
            .unwrap_or(CanvasView::Main)
    }

    /// Set of node ids that should NOT render this frame. Two cases:
    /// (1) members of a *collapsed* subgraph (replaced visually by
    /// the subgraph's compact block), (2) anything outside the
    /// current confined-edit scope.
    pub(crate) fn hidden_nodes_this_frame(&self) -> std::collections::HashSet<NodeId> {
        let mut hidden = std::collections::HashSet::new();
        if let CanvasView::SubGraph(scope) = self.current_view() {
            // Subgraph tab: hide every node that isn't a member of
            // the active scope.
            let visible: std::collections::HashSet<NodeId> = self
                .visuals.groups
                .get(&scope)
                .map(|g| g.member_ids.iter().copied().collect())
                .unwrap_or_default();
            for id in self.visuals.node_visuals.keys() {
                if !visible.contains(id) {
                    hidden.insert(*id);
                }
            }
        } else {
            // Whole-graph view: hide members of collapsed subgraphs.
            for g in self.visuals.groups.values() {
                if g.is_subgraph && g.collapsed {
                    for id in &g.member_ids {
                        hidden.insert(*id);
                    }
                }
            }
        }
        hidden
    }

}

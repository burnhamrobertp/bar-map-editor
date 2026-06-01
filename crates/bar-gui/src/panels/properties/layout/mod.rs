//! Layout node properties -- the coordinator / interface layer.
//!
//! The `Layout` node is one node holding a list of items, each of an
//! independent kind: a [`primitive::Primitive`] (ellipse / rectangle /
//! line) or a [`spline::Spline`] (Catmull-Rom control points; the
//! "draw" tool produces these). Those
//! kind modules each own their data, handles, drag response, and
//! drawing in full isolation -- they don't reference one another.
//!
//! This module is the manager: it reads the indexed per-item params
//! into a list of [`Item`]s, drives the shared 2D canvas, routes each
//! canvas gesture to the relevant item's kind, renders the
//! selected-item sidebar, owns the node-level controls (mode,
//! symmetry, item count), handles undo, and writes everything back to
//! the node's params.

mod primitive;
mod spline;

use std::collections::HashMap;

use bar_graph::{NodeId, ParamValue};
use eframe::egui;

use crate::app::{BarEditorApp, CanvasView};
use crate::panels::properties::properties_canvas::{
    self, CanvasGesture, CanvasState, CanvasTransform, HandleSpec,
};
use crate::panels::widgets::ParamSlider;

use primitive::Primitive;
use spline::Spline;

const MAX_ITEMS: usize = 8;

/// One layout item, dispatched to its independent kind implementation.
#[derive(Clone)]
enum Item {
    Primitive(Primitive),
    Spline(Spline),
}

impl Item {
    fn read(params: &HashMap<String, ParamValue>, i: usize) -> Self {
        let is_spline = matches!(
            params.get(&format!("type_{i}")),
            Some(ParamValue::String(s)) if s == "spline"
        );
        if is_spline {
            Item::Spline(Spline::read(params, i))
        } else {
            Item::Primitive(Primitive::read(params, i))
        }
    }

    fn write(&self, params: &mut HashMap<String, ParamValue>, i: usize) {
        match self {
            Item::Primitive(p) => p.write(params, i),
            Item::Spline(s) => s.write(params, i),
        }
    }

    fn handles(&self, item: usize) -> Vec<HandleSpec> {
        match self {
            Item::Primitive(p) => p.handles(item),
            Item::Spline(s) => s.handles(item),
        }
    }

    fn contains(&self, pos: [f32; 2]) -> bool {
        match self {
            Item::Primitive(p) => p.contains(pos),
            Item::Spline(s) => s.contains(pos),
        }
    }

    /// User-facing identifier of the specific kind: the primitive's
    /// `shape_type` for primitives (ellipse / rectangle / line) and
    /// `spline` for splines. Used in the sidebar title and the
    /// read-only type row.
    fn type_label(&self) -> String {
        match self {
            Item::Primitive(p) => p.shape_type.clone(),
            Item::Spline(_) => "spline".to_string(),
        }
    }
}

/// Live state for one update pass of the Layout editor. Chrome (left
/// side panel) and canvas (central panel) each build one of these
/// from the node's params, render their portion of the UI, and call
/// `commit_layout_frame` to write back + handle undo. Two independent
/// passes per frame is cheap and keeps the side-panel rendering free
/// of canvas-only state.
struct LayoutFrame {
    items: Vec<Item>,
    mode: String,
    symmetry: String,
    item_count: usize,
    orig_mode: String,
    orig_symmetry: String,
    state: CanvasState,
    state_id: egui::Id,
    mutated: bool,
    commit_undo_now: bool,
}

impl BarEditorApp {
    /// Active creation-tool kind from session state, defaulting to the
    /// ellipse primitive. Drives drag-to-create + the "+ Add at centre"
    /// button. One of `ellipse` / `rectangle` / `line` / `draw` (the
    /// last creates a Spline item from a freehand cursor path).
    fn layout_creation_tool(&self) -> String {
        self.dialog
            .layout_creation_tool
            .clone()
            .unwrap_or_else(|| "ellipse".to_string())
    }

    fn load_layout_frame(
        &self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        params: &HashMap<String, ParamValue>,
    ) -> LayoutFrame {
        let item_count = match params.get("item_count") {
            Some(ParamValue::UInt(n)) => (*n).clamp(1, MAX_ITEMS as u32) as usize,
            _ => 1,
        };
        let mode = string_param(params, "mode", "ridge");
        let symmetry = string_param(params, "symmetry", "none");
        let items: Vec<Item> = (0..item_count).map(|i| Item::read(params, i)).collect();
        let state_id = egui::Id::new(("lay_canvas_state", node_id.0));
        let mut state: CanvasState = ui
            .data(|d| d.get_temp::<CanvasState>(state_id))
            .unwrap_or_default();
        if let Some((hint_node, hint_sel)) = self.dialog.layout_selection_hint {
            if hint_node == node_id {
                state.selected = hint_sel;
            }
        }
        LayoutFrame {
            orig_mode: mode.clone(),
            orig_symmetry: symmetry.clone(),
            items,
            mode,
            symmetry,
            item_count,
            state,
            state_id,
            mutated: false,
            commit_undo_now: false,
        }
    }

    fn commit_layout_frame(&mut self, ui: &mut egui::Ui, node_id: NodeId, mut frame: LayoutFrame) {
        if frame.mode != frame.orig_mode || frame.symmetry != frame.orig_symmetry {
            frame.mutated = true;
            frame.commit_undo_now = true;
        }
        // Mirror the live selection into the dialog hint so the next
        // snapshot carries it; undo / redo restores will re-select.
        self.dialog.layout_selection_hint = Some((node_id, frame.state.selected));
        ui.data_mut(|d| d.insert_temp::<CanvasState>(frame.state_id, frame.state));

        let want_atomic_undo =
            frame.commit_undo_now && self.dialog.field_edit_in_progress.is_none();
        if want_atomic_undo {
            self.push_undo("Layout edit");
        }
        if frame.mutated {
            if frame.items.is_empty() {
                frame.items.push(Item::Primitive(Primitive::new(0.5, 0.5)));
                frame.item_count = 1;
            }
            if let Some(node) = self.graph.get_node_mut(node_id) {
                node.params.insert(
                    "item_count".to_string(),
                    ParamValue::UInt(frame.item_count as u32),
                );
                node.params
                    .insert("mode".to_string(), ParamValue::String(frame.mode));
                node.params
                    .insert("symmetry".to_string(), ParamValue::String(frame.symmetry));
                for (i, it) in frame.items.iter().enumerate() {
                    it.write(&mut node.params, i);
                }
                // Zero-height unused slots so a future item_count bump
                // doesn't surface stale geometry.
                for i in frame.items.len()..MAX_ITEMS {
                    node.params
                        .insert(format!("height_{i}"), ParamValue::Float(0.0));
                }
                node.mark_dirty();
            }
        }
        // Preview re-render is expensive (single-node eval + offscreen
        // 3D render). Trigger it only on commit-points (drag stop, atomic
        // op, button click) -- NOT on every-frame mutations during a
        // drag, which would re-render dozens of times per second and
        // make handle drags feel laggy.
        if frame.commit_undo_now {
            self.layout_preview.dirty = true;
        }
        if frame.commit_undo_now && !want_atomic_undo {
            if let Some(snap) = self.dialog.field_edit_in_progress.take() {
                self.history.push(snap);
                self.mark_dirty();
            }
        }
    }

    /// Side-panel chrome for an active Layout edit view: node-level
    /// controls, item count + add buttons, and the selected-item
    /// sidebar. Rendered in the left SidePanel (which otherwise hosts
    /// the node-graph palette).
    pub(crate) fn draw_layout_editor_chrome(&mut self, ui: &mut egui::Ui, node_id: NodeId) {
        let Some(params) = self.graph.get_node(node_id).map(|n| n.params.clone()) else {
            return;
        };
        let mut frame = self.load_layout_frame(ui, node_id, &params);

        // ── Tool selector ───────────────────────────────────────────
        // The active creation tool. Drives drag-to-create on the canvas
        // (which primitive kind, or freehand spline). Lives at the top
        // of the panel because it changes most often during authoring.
        let mut tool = self.layout_creation_tool();
        ui.horizontal(|ui| {
            ui.label("tool");
            combo(
                ui,
                ("lay_tool", node_id.0),
                &mut tool,
                // "draw" is the user-facing label for the spline /
                // freehand-curve tool. The underlying data type is
                // still `Spline` (Catmull-Rom control points).
                &["ellipse", "rectangle", "line", "draw"],
            );
        });
        self.dialog.layout_creation_tool = Some(tool.clone());
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label("mode");
            combo(
                ui,
                ("lay_mode", node_id.0),
                &mut frame.mode,
                &["ridge", "valley", "mask"],
            );
        });
        ui.horizontal(|ui| {
            ui.label("symmetry");
            combo(
                ui,
                ("lay_sym", node_id.0),
                &mut frame.symmetry,
                &[
                    "none",
                    "mirror_x",
                    "mirror_y",
                    "mirror_xy",
                    "rotate_180",
                    "rotate_90",
                ],
            );
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(format!("{} / {} items", frame.item_count, MAX_ITEMS));
            ui.add_space(8.0);
            let can_add = frame.items.len() < MAX_ITEMS;
            // One "add at centre" button that follows the active tool.
            // Drag-to-create on the canvas handles sized creation; this
            // is the click-once alternative.
            if ui
                .add_enabled(can_add, egui::Button::new("+ Add at centre"))
                .clicked()
            {
                if tool == "draw" {
                    frame.items.push(Item::Spline(Spline::new()));
                } else {
                    let mut p = Primitive::new(0.5, 0.5);
                    p.shape_type = tool.clone();
                    // Line default starts thin -- a thick default
                    // wouldn't read as a line. Other primitives keep
                    // their `Primitive::new` defaults.
                    if tool == "line" {
                        p.ry = 0.01;
                    }
                    frame.items.push(Item::Primitive(p));
                }
                frame.item_count = frame.items.len();
                frame.state.selected = Some(frame.item_count - 1);
                frame.mutated = true;
                frame.commit_undo_now = true;
            }
        });
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        if let Some(sel) = frame.state.selected {
            if sel < frame.items.len() {
                // Title row: "Shape N (type)" with a right-aligned
                // red trash icon to delete this shape. 1-indexed so
                // it reads naturally to users ("Shape 1" not "0").
                let title = format!("Shape {} ({})", sel + 1, frame.items[sel].type_label());
                let mut delete = false;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(title).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if trash_icon_button(ui) {
                            delete = true;
                        }
                    });
                });
                if delete {
                    remove_item(&mut frame.items, &mut frame.state, sel);
                    frame.item_count = frame.items.len();
                    frame.mutated = true;
                    frame.commit_undo_now = true;
                } else {
                    self.draw_item_sidebar(
                        ui,
                        node_id,
                        sel,
                        &mut frame.items,
                        &mut frame.mutated,
                        &mut frame.commit_undo_now,
                    );
                }
            }
        } else if frame.items.iter().any(|it| matches!(it, Item::Spline(_))) {
            ui.label("Select a spline, then click the canvas to add points.");
        } else {
            ui.label(
                "Drag a rectangle on the canvas to create a shape, or click a shape to select.",
            );
        }

        self.commit_layout_frame(ui, node_id, frame);
    }

    /// Central-panel authoring canvas. Renders only the canvas + its
    /// gestures (no chrome -- the side panel owns that). Returns
    /// whether the node mutated this frame so the caller can mark the
    /// live preview dirty.
    fn layout_editor_canvas(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        canvas_size: f32,
    ) -> bool {
        let Some(params) = self.graph.get_node(node_id).map(|n| n.params.clone()) else {
            return false;
        };
        let mut frame = self.load_layout_frame(ui, node_id, &params);
        let mut needs_undo_snapshot = false;

        // Delete / Backspace removes the currently-selected shape when
        // an edit view is the active tab. The shell-level Delete
        // handler is gated off in this view so the Layout node itself
        // doesn't get deleted. Suppressed while a widget has keyboard
        // focus (e.g. a numeric input mid-edit).
        let typing = ui.ctx().wants_keyboard_input();
        let delete_pressed = !typing
            && ui
                .ctx()
                .input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
        if delete_pressed {
            if let Some(sel) = frame.state.selected {
                if sel < frame.items.len() {
                    remove_item(&mut frame.items, &mut frame.state, sel);
                    frame.item_count = frame.items.len();
                    frame.mutated = true;
                    frame.commit_undo_now = true;
                }
            }
        }

        // Transformer handles render ONLY for the selected item.
        // Unselected shapes get just their outline; clicking the
        // outline selects them (the body hit-test closure below).
        let handles: Vec<HandleSpec> = frame
            .state
            .selected
            .and_then(|sel| frame.items.get(sel).map(|it| it.handles(sel)))
            .unwrap_or_default();

        let draw_data: Vec<(bool, DrawSnapshot)> = frame
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| (frame.state.selected == Some(i), DrawSnapshot::from(item)))
            .collect();
        // Drag-to-create preview. For primitive tools it's a Primitive
        // of the chosen kind sized to the drag rect. For the spline
        // tool it's the freehand polyline being traced. The widget
        // owns the creation lifecycle but is shape-agnostic, so the
        // silhouette is painted here in the caller's draw_items
        // closure.
        let tool = self.layout_creation_tool();
        let shift_held = ui.ctx().input(|i| i.modifiers.shift);
        let (creation_preview, preview_polyline): (Option<Primitive>, Option<Vec<[f32; 2]>>) =
            match frame.state.creation.as_ref() {
                Some(c) if c.moved => {
                    if tool == "draw" {
                        (None, Some(c.path.clone()))
                    } else {
                        let p = primitive_from_drag(c.press_pos, c.current_pos, &tool, shift_held);
                        (Some(p), None)
                    }
                }
                _ => (None, None),
            };
        // Body hit-test closure: walks items top-down so the most
        // recently-added item wins ties. Cloned so the closure owns
        // its inputs (the items Vec is mutated by the gesture loop).
        let hit_items = frame.items.clone();
        let gestures = properties_canvas::draw(
            ui,
            egui::vec2(canvas_size, canvas_size),
            &mut frame.state,
            &handles,
            move |painter, xform| {
                for (sel, snap) in &draw_data {
                    snap.draw(painter, xform, *sel);
                }
                if let Some(p) = &creation_preview {
                    p.draw_preview(painter, xform);
                }
                if let Some(path) = &preview_polyline {
                    if path.len() >= 2 {
                        let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 200, 60));
                        let mut prev = xform.to_pixel(path[0]);
                        for next in path.iter().skip(1) {
                            let np = xform.to_pixel(*next);
                            painter.line_segment([prev, np], stroke);
                            prev = np;
                        }
                    }
                }
            },
            move |pos| hit_items.iter().rposition(|it| it.contains(pos)),
        );

        for g in gestures {
            match g {
                CanvasGesture::AddAt { pos: _ } => {
                    // A click in empty canvas (no drag, no item
                    // body, no handle) deselects the current shape.
                    // Standard editor UX: empty-space click clears
                    // the selection. Splines are authored end-to-end
                    // by the "draw" tool's drag flow, so the old
                    // "click adds a control point" behavior is gone.
                    frame.state.selected = None;
                }
                CanvasGesture::CreateAt { from, to, path } => {
                    if frame.items.len() < MAX_ITEMS {
                        if tool == "draw" {
                            // Freehand draw: subsample the captured
                            // cursor path to a manageable number of
                            // control points. The Catmull-Rom curve
                            // through them is what the executor will
                            // rasterise.
                            let pts = simplify_path(&path);
                            if pts.len() >= 2 {
                                let mut s = Spline::new();
                                s.points = pts;
                                frame.items.push(Item::Spline(s));
                                frame.item_count = frame.items.len();
                                frame.state.selected = Some(frame.item_count - 1);
                                frame.mutated = true;
                                frame.commit_undo_now = true;
                            }
                        } else {
                            // Use the same helper the preview drew so
                            // the released shape matches the silhouette
                            // exactly (drag direction -> line angle,
                            // shift -> snapped angle / equal radii).
                            let p = primitive_from_drag(from, to, &tool, shift_held);
                            frame.items.push(Item::Primitive(p));
                            frame.item_count = frame.items.len();
                            frame.state.selected = Some(frame.item_count - 1);
                            frame.mutated = true;
                            frame.commit_undo_now = true;
                        }
                    }
                }
                CanvasGesture::HandlePressed { item, handle, .. } => {
                    frame.state.selected = Some(item);
                    needs_undo_snapshot = true;
                    // Capture the opposite-corner anchor for
                    // corner-resize handles. The position is in world
                    // (normalised [0,1]) space so it stays fixed as
                    // the primitive's centre and rx/ry change during
                    // the drag. Cleared on release.
                    frame.state.corner_anchor = opposite_corner_anchor(&frame.items, item, handle);
                }
                CanvasGesture::HandleDragged { item, handle, pos } => {
                    if let Some(it) = frame.items.get_mut(item) {
                        match it {
                            Item::Primitive(p) => {
                                // Body-press centre drags translate by
                                // the cursor delta from the press
                                // point so the click position stays
                                // under the cursor; direct centre-
                                // handle grabs fall through to the
                                // existing cursor-snap behaviour.
                                let effective_pos = if handle == primitive::H_CENTRE {
                                    match (
                                        frame.state.body_drag_origin,
                                        frame.state.drag.as_ref().map(|d| d.press_pos),
                                    ) {
                                        (Some(origin), Some(press)) => [
                                            origin[0] + (pos[0] - press[0]),
                                            origin[1] + (pos[1] - press[1]),
                                        ],
                                        _ => pos,
                                    }
                                } else {
                                    pos
                                };
                                p.apply_drag(
                                    handle,
                                    effective_pos,
                                    frame.state.corner_anchor,
                                    shift_held,
                                );
                            }
                            Item::Spline(s) => s.move_point(handle.0 as usize, pos),
                        }
                        frame.mutated = true;
                    }
                }
                CanvasGesture::HandleReleased { moved, .. } => {
                    frame.state.corner_anchor = None;
                    frame.state.body_drag_origin = None;
                    if moved {
                        if self.dialog.field_edit_in_progress.is_some() {
                            frame.commit_undo_now = true;
                        }
                    } else {
                        self.dialog.field_edit_in_progress = None;
                    }
                }
                CanvasGesture::ItemPressed { item, pos } => {
                    // Press on an item's body: select it, and for
                    // primitives also set up a centre-handle drag
                    // anchored to the primitive's centre at press
                    // time. Subsequent HandleDragged frames translate
                    // by the cursor's delta from `pos`, so the click
                    // point stays under the cursor. Splines have no
                    // whole-shape translate; for them this is a pure
                    // select. The drag the widget actually tracks
                    // starts firing HandleDragged on the next frame
                    // because the widget already ran its drag block
                    // before we mutated state.drag here.
                    frame.state.selected = Some(item);
                    needs_undo_snapshot = true;
                    if let Some(Item::Primitive(p)) = frame.items.get(item) {
                        frame.state.body_drag_origin = Some([p.x, p.y]);
                        frame.state.drag = Some(properties_canvas::DragInProgress {
                            item,
                            handle: primitive::H_CENTRE,
                            press_pos: pos,
                            moved: false,
                        });
                    }
                }
                CanvasGesture::HandleDeleted { item, handle } => {
                    let removed = match frame.items.get_mut(item) {
                        Some(Item::Spline(s)) => s.remove_point(handle.0 as usize),
                        Some(Item::Primitive(_)) => {
                            remove_item(&mut frame.items, &mut frame.state, item);
                            frame.item_count = frame.items.len();
                            true
                        }
                        None => false,
                    };
                    if removed {
                        frame.mutated = true;
                        frame.commit_undo_now = true;
                    }
                }
            }
        }

        if needs_undo_snapshot && self.dialog.field_edit_in_progress.is_none() {
            let snap = self.snapshot("Layout edit");
            self.dialog.field_edit_in_progress = Some(snap);
        }

        let mutated = frame.mutated;
        self.commit_layout_frame(ui, node_id, frame);
        mutated
    }

    /// Full-area node-edit view: the authoring canvas on the left
    /// and a live preview on the right, splitting the available width
    /// 50/50. Chrome (mode/symmetry/items/sidebar) lives in the left
    /// SidePanel (see `draw_layout_editor_chrome`). Entered by
    /// double-clicking the node; backed out via the tab close button.
    pub(crate) fn draw_layout_editor(&mut self, ui: &mut egui::Ui, node_id: NodeId) {
        if self.graph.get_node(node_id).is_none() {
            return;
        }

        let avail = ui.available_size();
        let gap = 12.0;
        // Equal halves; each pane is a square that fits its half (and
        // the vertical room).
        let half_w = ((avail.x - gap) * 0.5).max(160.0);
        let pane_size = half_w.min(avail.y - 24.0).max(160.0);

        let mut mutated = false;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_min_width(pane_size);
                ui.set_max_width(pane_size);
                mutated = self.layout_editor_canvas(ui, node_id, pane_size);
            });
            ui.add_space(gap);
            ui.vertical(|ui| {
                ui.set_min_width(pane_size);
                ui.set_max_width(pane_size);
                ui.label("Preview (this node only)");
                self.draw_layout_preview_pane(ui, node_id, pane_size, true);
            });
        });

        // Note: `mutated` returned by `layout_editor_canvas` fires on
        // every drag frame (so the on-canvas silhouette tracks live);
        // we deliberately do NOT mark the preview dirty here. The
        // preview is marked dirty inside `commit_layout_frame` only on
        // commit-points (drag stop, atomic op) so the heavy 3D
        // re-render fires once per gesture, not per pixel.
        let _ = mutated;
        if self.layout_preview.dirty {
            ui.ctx().request_repaint();
        }
    }

    /// Draw the live-preview image for `node_id` if the runner has
    /// produced one, else a placeholder. The texture is uploaded by
    /// bar-app's runner (see `BarEditorApp::layout_preview`).
    fn draw_layout_preview_pane(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        size: f32,
        interactive: bool,
    ) {
        // Ensure the runner is rendering THIS node's preview. Drawn
        // from both the side-panel popover (single-click) and the
        // edit view, so requesting here means the popover preview
        // renders without having to enter the node first.
        if self.layout_preview.node != Some(node_id) {
            self.layout_preview.node = Some(node_id);
            self.layout_preview.dirty = true;
            ui.ctx().request_repaint();
        }

        let sense = if interactive {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::hover()
        };
        let size_v = egui::vec2(size, size);

        // The pane is None until the runner has GPU context + the
        // first preview frame to render. Show a placeholder until
        // then; otherwise paint whatever the pane has bound.
        let Some(pane) = self.layout_preview.pane.as_mut() else {
            let (rect, _) = ui.allocate_exact_size(size_v, sense);
            ui.painter()
                .rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "rendering preview…",
                egui::FontId::proportional(13.0),
                ui.visuals().weak_text_color(),
            );
            return;
        };

        if !pane.has_texture() {
            // Pane exists but hasn't bound its texture yet -- still
            // request a repaint so the runner catches up.
            ui.ctx().request_repaint();
        }
        let response = pane.paint(ui, size_v, sense);
        if !interactive {
            return;
        }
        if pane.apply_default_camera_input(&response, ui.ctx()) {
            self.layout_preview.dirty = true;
            ui.ctx().request_repaint();
        }
    }

    /// Compact side-panel surface for a single-clicked Layout node: a
    /// read-only preview thumbnail + an Edit button that descends into
    /// the full editor, plus a terse summary. The authoring canvas
    /// itself lives only in the edit view now.
    pub(crate) fn draw_layout_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        params: &HashMap<String, ParamValue>,
    ) {
        let item_count = match params.get("item_count") {
            Some(ParamValue::UInt(n)) => (*n).clamp(1, MAX_ITEMS as u32),
            _ => 1,
        };
        let mode = string_param(params, "mode", "ridge");
        let symmetry = string_param(params, "symmetry", "none");

        let thumb = ui.available_width().clamp(120.0, 240.0);
        self.draw_layout_preview_pane(ui, node_id, thumb, false);

        ui.add_space(6.0);
        if ui
            .add_sized([thumb, 26.0], egui::Button::new("Edit layout"))
            .clicked()
        {
            self.open_or_activate_tab(CanvasView::NodeEdit(node_id));
            // Descending into the edit view supersedes the popover;
            // close it (tick_props_panel consumes this).
            self.props.close_requested = true;
        }
        ui.add_space(4.0);
        ui.label(format!("{item_count} item(s)  ·  mode: {mode}"));
        if symmetry != "none" {
            ui.label(format!("symmetry: {symmetry}"));
        }
        ui.label("Double-click the node, or Edit, to author shapes.");
    }

    /// Selected-item sidebar. Type dropdown is common; the rest is the
    /// item kind's own controls. Kept on the coordinator because it
    /// needs `self` for the undo snapshot during slider drags.
    fn draw_item_sidebar(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        sel: usize,
        items: &mut [Item],
        mutated: &mut bool,
        commit_undo_now: &mut bool,
    ) {
        // The type identifier is shown in the sidebar header ("Shape
        // N (type)") right above this code, so no separate type row
        // is needed here. The supported path to change a shape's
        // type is "delete the shape, switch the tool, draw a new
        // one" -- in-place conversion has too many edge cases around
        // radii / control points.

        // Shared + kind-specific sliders / toggles.
        egui::Grid::new(("lay_side2", node_id.0))
            .num_columns(2)
            .spacing([8.0, 3.0])
            .show(ui, |ui| match &mut items[sel] {
                Item::Primitive(p) => {
                    self.slider_row(
                        ui,
                        "height",
                        &mut p.height,
                        0.0,
                        1.0,
                        mutated,
                        commit_undo_now,
                    );
                    self.slider_row(
                        ui,
                        "falloff",
                        &mut p.falloff,
                        0.0,
                        1.0,
                        mutated,
                        commit_undo_now,
                    );
                }
                Item::Spline(s) => {
                    self.slider_row(
                        ui,
                        "height",
                        &mut s.height,
                        0.0,
                        1.0,
                        mutated,
                        commit_undo_now,
                    );
                    self.slider_row(
                        ui,
                        "falloff",
                        &mut s.falloff,
                        0.0,
                        1.0,
                        mutated,
                        commit_undo_now,
                    );
                    self.slider_row(
                        ui,
                        "width",
                        &mut s.width,
                        0.001,
                        0.5,
                        mutated,
                        commit_undo_now,
                    );
                    ui.label("closed");
                    if ui.checkbox(&mut s.closed, "").changed() {
                        *mutated = true;
                        *commit_undo_now = true;
                    }
                    ui.end_row();
                    ui.label("fill");
                    if ui.checkbox(&mut s.fill, "").changed() {
                        *mutated = true;
                        *commit_undo_now = true;
                    }
                    ui.end_row();
                }
            });
    }

    fn slider_row(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        value: &mut f32,
        lo: f32,
        hi: f32,
        mutated: &mut bool,
        commit_undo_now: &mut bool,
    ) {
        ui.label(label);
        let resp = ui.add(ParamSlider::new(value, lo, hi));
        if (resp.drag_started() || resp.gained_focus())
            && self.dialog.field_edit_in_progress.is_none()
        {
            let snap = self.snapshot("Layout edit");
            self.dialog.field_edit_in_progress = Some(snap);
        }
        // ParamSlider intentionally suppresses `changed()` during drag
        // (so heavy callers don't fire per pixel) and only fires it on
        // drag-stop. The layout editor reloads its items from graph
        // params each frame, so a "mutated only on drag-stop" pattern
        // would lose the value mid-drag -- the slider would visually
        // snap back to the stored value next frame. Mark `mutated` on
        // both `changed()` and `dragged()` so the value persists
        // through the drag; `commit_undo_now` stays gated on drag-stop
        // so the heavy preview re-render only fires once at the end.
        if resp.changed() || resp.dragged() {
            *mutated = true;
        }
        if resp.drag_stopped() || resp.lost_focus() {
            *commit_undo_now = true;
        }
        ui.end_row();
    }
}

/// Reduce a freehand cursor path to control points using the
/// Ramer-Douglas-Peucker algorithm. Straight stretches collapse to
/// their endpoints; tight curves keep enough samples to stay within
/// `TOL` of the original path. The point count grows with the shape's
/// complexity rather than its mere length, which matches the
/// authoring intent: a long straight drag should not be more
/// expensive than a short one.
///
/// `MAX_POINTS` is a safety cap (a pathologically jittery input could
/// otherwise emit thousands of control points and slow rasterisation).
/// `TOL` is the perpendicular-distance threshold in normalised
/// [0, 1] canvas units -- 0.004 is ~0.4 % of the canvas, comfortably
/// below what the eye notices at preview resolution.
fn simplify_path(path: &[[f32; 2]]) -> Vec<[f32; 2]> {
    const TOL: f32 = 0.004;
    const MAX_POINTS: usize = 96;
    let n = path.len();
    if n <= 2 {
        return path.to_vec();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    rdp_recurse(path, 0, n - 1, TOL, &mut keep);
    let kept: Vec<[f32; 2]> = path
        .iter()
        .enumerate()
        .filter_map(|(i, p)| if keep[i] { Some(*p) } else { None })
        .collect();
    if kept.len() <= MAX_POINTS {
        return kept;
    }
    // RDP couldn't simplify enough (very curvy long drawing). Evenly
    // decimate the kept points so eval cost stays bounded.
    let step = (kept.len() - 1) as f32 / (MAX_POINTS - 1) as f32;
    (0..MAX_POINTS)
        .map(|i| kept[((i as f32 * step).round() as usize).min(kept.len() - 1)])
        .collect()
}

fn rdp_recurse(path: &[[f32; 2]], start: usize, end: usize, tol: f32, keep: &mut [bool]) {
    if end <= start + 1 {
        return;
    }
    let a = path[start];
    let b = path[end];
    let mut max_d = 0.0_f32;
    let mut max_i = start;
    for (i, p) in path.iter().enumerate().take(end).skip(start + 1) {
        let d = perpendicular_distance(*p, a, b);
        if d > max_d {
            max_d = d;
            max_i = i;
        }
    }
    if max_d > tol {
        keep[max_i] = true;
        rdp_recurse(path, start, max_i, tol, keep);
        rdp_recurse(path, max_i, end, tol, keep);
    }
}

/// Perpendicular distance from `p` to the infinite line through `a` and
/// `b`. Falls back to the point-to-point distance when `a == b`.
fn perpendicular_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let abx = b[0] - a[0];
    let aby = b[1] - a[1];
    let len2 = abx * abx + aby * aby;
    if len2 < 1e-10 {
        let dx = p[0] - a[0];
        let dy = p[1] - a[1];
        return (dx * dx + dy * dy).sqrt();
    }
    let cross = abx * (p[1] - a[1]) - aby * (p[0] - a[0]);
    cross.abs() / len2.sqrt()
}

/// Build the primitive a drag-to-create gesture should produce. Used by
/// both the live preview silhouette (so what the user sees is what
/// they get on release) and the `CreateAt` handler that actually adds
/// the item. Tool-specific:
///
/// * `line` -- drag direction sets the angle, drag midpoint the centre,
///   drag length the `rx` (handle extent; the executor renders a
///   line of map-wide visible length regardless). Shift snaps the
///   angle to the nearest 45-degree increment.
/// * `ellipse` / `rectangle` -- drag corners bound the shape. Shift
///   constrains to equal radii (circle / square).
///
/// `spline` is handled separately in the caller because it uses the
/// freehand path rather than just `from`/`to`.
fn primitive_from_drag(from: [f32; 2], to: [f32; 2], tool: &str, shift: bool) -> Primitive {
    let cx = ((from[0] + to[0]) * 0.5).clamp(0.0, 1.0);
    let cy = ((from[1] + to[1]) * 0.5).clamp(0.0, 1.0);
    let mut p = Primitive::new(cx, cy);
    p.shape_type = tool.to_string();
    if tool == "line" {
        let dx = to[0] - from[0];
        let dy = to[1] - from[1];
        let mut angle = dy.atan2(dx);
        if shift {
            let step = std::f32::consts::FRAC_PI_4;
            angle = (angle / step).round() * step;
        }
        p.angle = angle.to_degrees().rem_euclid(360.0);
        p.rx = ((dx * dx + dy * dy).sqrt() * 0.5).max(0.01);
        // Thin default width; users widen via the falloff slider or
        // by dragging the corner handles after creation.
        p.ry = 0.01;
    } else {
        let mut rx = ((to[0] - from[0]).abs() * 0.5).max(0.005);
        let mut ry = ((to[1] - from[1]).abs() * 0.5).max(0.005);
        if shift {
            let r = rx.max(ry);
            rx = r;
            ry = r;
        }
        p.rx = rx;
        p.ry = ry;
    }
    p
}

/// World-space position of the OPPOSITE corner of the given handle,
/// captured at press time. The corner-resize math anchors the
/// opposite corner here so the dragged corner can move to the cursor
/// without the rest of the shape drifting. Returns `None` for any
/// handle that isn't one of the four corner handles or any item that
/// isn't a primitive (splines don't have a corner-anchor concept).
fn opposite_corner_anchor(
    items: &[Item],
    item_idx: usize,
    handle: properties_canvas::HandleId,
) -> Option<[f32; 2]> {
    use std::f32::consts::PI;
    let Some(Item::Primitive(p)) = items.get(item_idx) else {
        return None;
    };
    // Primitive corner handle ids: TL=1, TR=2, BL=3, BR=4. Opposite
    // pairs are TL<->BR and TR<->BL.
    let (opp_lx, opp_ly) = match handle.0 {
        1 => (p.rx, p.ry),   // TL -> BR
        2 => (-p.rx, p.ry),  // TR -> BL
        3 => (p.rx, -p.ry),  // BL -> TR
        4 => (-p.rx, -p.ry), // BR -> TL
        _ => return None,
    };
    let (sina, cosa) = (p.angle * PI / 180.0).sin_cos();
    Some([
        p.x + opp_lx * cosa - opp_ly * sina,
        p.y + opp_lx * sina + opp_ly * cosa,
    ])
}

/// Compact red trash-can button used in the layout sidebar to delete
/// the selected shape. Drawn from line segments so the source stays
/// font-independent and ASCII-only.
fn trash_icon_button(ui: &mut egui::Ui) -> bool {
    let size = egui::vec2(20.0, 20.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let color = if resp.hovered() {
        egui::Color32::from_rgb(255, 80, 80)
    } else {
        egui::Color32::from_rgb(210, 70, 70)
    };
    let stroke = egui::Stroke::new(1.4, color);
    let painter = ui.painter();
    let cx = rect.center().x;
    let lid_y = rect.top() + 5.0;
    let body_top = lid_y + 1.5;
    let bot = rect.bottom() - 3.0;
    let half_w = 5.0;
    // Lid handle (small notch on top of the lid).
    painter.line_segment(
        [
            egui::pos2(cx - 2.5, lid_y - 1.8),
            egui::pos2(cx + 2.5, lid_y - 1.8),
        ],
        stroke,
    );
    // Lid (slightly wider than the body).
    painter.line_segment(
        [
            egui::pos2(cx - half_w - 1.0, lid_y),
            egui::pos2(cx + half_w + 1.0, lid_y),
        ],
        stroke,
    );
    // Body sides (taper inward slightly toward the base).
    painter.line_segment(
        [
            egui::pos2(cx - half_w + 0.4, body_top),
            egui::pos2(cx - half_w + 1.4, bot),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(cx + half_w - 0.4, body_top),
            egui::pos2(cx + half_w - 1.4, bot),
        ],
        stroke,
    );
    // Body bottom.
    painter.line_segment(
        [
            egui::pos2(cx - half_w + 1.4, bot),
            egui::pos2(cx + half_w - 1.4, bot),
        ],
        stroke,
    );
    resp.on_hover_text("Delete shape").clicked()
}

fn remove_item(items: &mut Vec<Item>, state: &mut CanvasState, idx: usize) {
    if idx >= items.len() {
        return;
    }
    items.remove(idx);
    match state.selected {
        Some(s) if s == idx => state.selected = None,
        Some(s) if s > idx => state.selected = Some(s - 1),
        _ => {}
    }
}

fn string_param(params: &HashMap<String, ParamValue>, key: &str, default: &str) -> String {
    match params.get(key) {
        Some(ParamValue::String(s)) => s.clone(),
        _ => default.to_string(),
    }
}

fn combo(ui: &mut egui::Ui, id: impl std::hash::Hash, current: &mut String, choices: &[&str]) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(current.as_str())
        .show_ui(ui, |ui| {
            for choice in choices {
                if ui.selectable_label(current == choice, *choice).clicked() {
                    *current = choice.to_string();
                }
            }
        });
}

/// Flattened drawing data so the FnOnce paint closure doesn't borrow
/// the `items` the gesture loop needs to mutate afterwards.
enum DrawSnapshot {
    Primitive(Primitive),
    Spline(Spline),
}

impl DrawSnapshot {
    fn from(item: &Item) -> Self {
        match item {
            Item::Primitive(p) => DrawSnapshot::Primitive(p.clone()),
            Item::Spline(s) => DrawSnapshot::Spline(s.clone()),
        }
    }

    fn draw(&self, painter: &egui::Painter, xform: &CanvasTransform, selected: bool) {
        match self {
            DrawSnapshot::Primitive(p) => p.draw(painter, xform, selected),
            DrawSnapshot::Spline(s) => s.draw(painter, xform, selected),
        }
    }
}

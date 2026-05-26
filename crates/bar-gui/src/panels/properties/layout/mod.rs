//! Layout node properties -- the coordinator / interface layer.
//!
//! The `Layout` node is one node holding a list of items, each of an
//! independent kind: a [`primitive::Primitive`] (ellipse / rectangle /
//! ridge) or a [`spline::Spline`] (Catmull-Rom control points). Those
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

use crate::app::BarEditorApp;
use crate::panels::properties::properties_canvas::{
    self, CanvasGesture, CanvasState, CanvasTransform, HandleSpec,
};
use crate::panels::widgets::ParamSlider;

use primitive::Primitive;
use spline::Spline;

const MAX_ITEMS: usize = 8;

/// One layout item, dispatched to its independent kind implementation.
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

    fn kind_label(&self) -> &'static str {
        match self {
            Item::Primitive(_) => "shape",
            Item::Spline(_) => "spline",
        }
    }
}

impl BarEditorApp {
    pub(crate) fn draw_layout_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        params: &HashMap<String, ParamValue>,
    ) {
        let mut item_count = match params.get("item_count") {
            Some(ParamValue::UInt(n)) => (*n).clamp(1, MAX_ITEMS as u32) as usize,
            _ => 1,
        };
        let mut mode = string_param(params, "mode", "ridge");
        let mut symmetry = string_param(params, "symmetry", "none");
        let mut items: Vec<Item> = (0..item_count).map(|i| Item::read(params, i)).collect();

        let mut mutated = false;
        let mut commit_undo_now = false;
        let mut needs_undo_snapshot = false;

        // ── Node-level controls ─────────────────────────────────────
        ui.horizontal(|ui| {
            ui.label("mode");
            combo(
                ui,
                ("lay_mode", node_id.0),
                &mut mode,
                &["ridge", "valley", "mask"],
            );
            ui.add_space(12.0);
            ui.label("symmetry");
            combo(
                ui,
                ("lay_sym", node_id.0),
                &mut symmetry,
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

        let state_id = egui::Id::new(("lay_canvas_state", node_id.0));
        let mut state: CanvasState = ui
            .data(|d| d.get_temp::<CanvasState>(state_id))
            .unwrap_or_default();

        ui.horizontal(|ui| {
            ui.label(format!("{item_count} / {MAX_ITEMS} items"));
            ui.add_space(12.0);
            let can_add = items.len() < MAX_ITEMS;
            if ui
                .add_enabled(can_add, egui::Button::new("+ Shape"))
                .clicked()
            {
                items.push(Item::Primitive(Primitive::new(0.5, 0.5)));
                item_count = items.len();
                state.selected = Some(item_count - 1);
                mutated = true;
                commit_undo_now = true;
            }
            if ui
                .add_enabled(can_add, egui::Button::new("+ Spline"))
                .clicked()
            {
                items.push(Item::Spline(Spline::new()));
                item_count = items.len();
                state.selected = Some(item_count - 1);
                mutated = true;
                commit_undo_now = true;
            }
        });

        // ── Canvas ──────────────────────────────────────────────────
        let canvas_size = ui.available_width().clamp(280.0, 500.0);
        let handles: Vec<HandleSpec> = items
            .iter()
            .enumerate()
            .flat_map(|(i, item)| item.handles(i))
            .collect();

        // Snapshot drawing data (the closure is FnOnce).
        let draw_data: Vec<(bool, DrawSnapshot)> = items
            .iter()
            .enumerate()
            .map(|(i, item)| (state.selected == Some(i), DrawSnapshot::from(item)))
            .collect();
        let gestures = properties_canvas::draw(
            ui,
            egui::vec2(canvas_size, canvas_size),
            &mut state,
            &handles,
            move |painter, xform| {
                for (sel, snap) in &draw_data {
                    snap.draw(painter, xform, *sel);
                }
            },
        );

        for g in gestures {
            match g {
                CanvasGesture::AddAt { pos } => {
                    // Append a control point to a selected spline;
                    // otherwise add a new primitive (preserves the old
                    // click-to-add-shape habit).
                    let add_point_to = match state.selected.and_then(|s| items.get_mut(s)) {
                        Some(Item::Spline(s)) => Some(s),
                        _ => None,
                    };
                    if let Some(s) = add_point_to {
                        s.add_point(pos);
                        mutated = true;
                        commit_undo_now = true;
                    } else if items.len() < MAX_ITEMS {
                        items.push(Item::Primitive(Primitive::new(pos[0], pos[1])));
                        item_count = items.len();
                        state.selected = Some(item_count - 1);
                        mutated = true;
                        commit_undo_now = true;
                    }
                }
                CanvasGesture::HandlePressed { item, .. } => {
                    state.selected = Some(item);
                    needs_undo_snapshot = true;
                }
                CanvasGesture::HandleDragged { item, handle, pos } => {
                    if let Some(it) = items.get_mut(item) {
                        match it {
                            Item::Primitive(p) => p.apply_drag(handle, pos),
                            Item::Spline(s) => s.move_point(handle.0 as usize, pos),
                        }
                        mutated = true;
                    }
                }
                CanvasGesture::HandleReleased { moved, .. } => {
                    if moved {
                        if self.dialog.field_edit_in_progress.is_some() {
                            commit_undo_now = true;
                        }
                    } else {
                        self.dialog.field_edit_in_progress = None;
                    }
                }
                CanvasGesture::HandleDeleted { item, handle } => {
                    let removed = match items.get_mut(item) {
                        // Spline: drop the clicked point; the item stays
                        // (remove it via the sidebar).
                        Some(Item::Spline(s)) => s.remove_point(handle.0 as usize),
                        // Primitive: remove the whole item.
                        Some(Item::Primitive(_)) => {
                            remove_item(&mut items, &mut state, item);
                            item_count = items.len();
                            true
                        }
                        None => false,
                    };
                    if removed {
                        mutated = true;
                        commit_undo_now = true;
                    }
                }
            }
        }

        if needs_undo_snapshot && self.dialog.field_edit_in_progress.is_none() {
            let snap = self.snapshot("Layout edit");
            self.dialog.field_edit_in_progress = Some(snap);
        }

        // ── Selected-item sidebar ───────────────────────────────────
        ui.add_space(6.0);
        if let Some(sel) = state.selected {
            if sel < items.len() {
                let label = format!("Item {sel} ({})", items[sel].kind_label());
                ui.label(label);
                self.draw_item_sidebar(
                    ui,
                    node_id,
                    sel,
                    &mut items,
                    &mut mutated,
                    &mut commit_undo_now,
                );
                if ui.button("Delete item").clicked() {
                    remove_item(&mut items, &mut state, sel);
                    item_count = items.len();
                    mutated = true;
                    commit_undo_now = true;
                }
            }
        } else if items.iter().any(|it| matches!(it, Item::Spline(_))) {
            ui.label("Select a spline, then click the canvas to add points.");
        } else {
            ui.label("Click a shape to select, or click empty canvas to add one.");
        }

        ui.data_mut(|d| d.insert_temp::<CanvasState>(state_id, state));

        // Node-level dropdowns don't emit canvas gestures; diff them.
        if mode != string_param(params, "mode", "ridge")
            || symmetry != string_param(params, "symmetry", "none")
        {
            mutated = true;
            commit_undo_now = true;
        }

        // Atomic ops snapshot pre-mutation; drags use the press snapshot.
        let want_atomic_undo = commit_undo_now && self.dialog.field_edit_in_progress.is_none();
        if want_atomic_undo {
            self.push_undo("Layout edit");
        }

        if mutated {
            if items.is_empty() {
                items.push(Item::Primitive(Primitive::new(0.5, 0.5)));
                item_count = 1;
            }
            if let Some(node) = self.graph.get_node_mut(node_id) {
                node.params.insert(
                    "item_count".to_string(),
                    ParamValue::UInt(item_count as u32),
                );
                node.params
                    .insert("mode".to_string(), ParamValue::String(mode));
                node.params
                    .insert("symmetry".to_string(), ParamValue::String(symmetry));
                for (i, it) in items.iter().enumerate() {
                    it.write(&mut node.params, i);
                }
                // Zero-height the unused slots so a future item_count
                // bump doesn't surface stale geometry.
                for i in items.len()..MAX_ITEMS {
                    node.params
                        .insert(format!("height_{i}"), ParamValue::Float(0.0));
                }
                node.mark_dirty();
            }
        }

        if commit_undo_now && !want_atomic_undo {
            if let Some(snap) = self.dialog.field_edit_in_progress.take() {
                self.history.push(snap);
                self.mark_dirty();
            }
        }
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
        // Type dropdown can convert the item between kinds.
        let current_kind = match &items[sel] {
            Item::Primitive(p) => p.shape_type.clone(),
            Item::Spline(_) => "spline".to_string(),
        };
        let mut new_kind = current_kind.clone();
        egui::Grid::new(("lay_side", node_id.0))
            .num_columns(2)
            .spacing([8.0, 3.0])
            .show(ui, |ui| {
                ui.label("type");
                combo(
                    ui,
                    ("lay_type", node_id.0, sel as u64),
                    &mut new_kind,
                    &["ellipse", "rectangle", "ridge", "spline"],
                );
                ui.end_row();
            });
        if new_kind != current_kind {
            convert_item(&mut items[sel], &new_kind);
            *mutated = true;
            *commit_undo_now = true;
        }

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
        if resp.changed() {
            *mutated = true;
        }
        if resp.drag_stopped() || resp.lost_focus() {
            *commit_undo_now = true;
        }
        ui.end_row();
    }
}

/// Convert an item to a new kind, preserving the shared height/falloff.
fn convert_item(item: &mut Item, new_kind: &str) {
    let (height, falloff) = match item {
        Item::Primitive(p) => (p.height, p.falloff),
        Item::Spline(s) => (s.height, s.falloff),
    };
    if new_kind == "spline" {
        if !matches!(item, Item::Spline(_)) {
            let mut s = Spline::new();
            s.height = height;
            s.falloff = falloff;
            *item = Item::Spline(s);
        }
    } else {
        match item {
            // Already a primitive: just change its shape sub-type.
            Item::Primitive(p) => p.shape_type = new_kind.to_string(),
            // Was a spline: become a primitive of the chosen shape.
            Item::Spline(_) => {
                let mut p = Primitive::new(0.5, 0.5);
                p.shape_type = new_kind.to_string();
                p.height = height;
                p.falloff = falloff;
                *item = Item::Primitive(p);
            }
        }
    }
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

//! Contextual properties for the LayoutGenerator node.
//!
//! 2D canvas editor: each shape is drawn as its actual silhouette and
//! manipulated through handles (centre = move, four corners = resize,
//! one rotation arm = angle). Click empty canvas to add an ellipse;
//! right-click a centre handle to delete its shape. Sidebar below the
//! canvas exposes the selected shape's non-spatial params (type,
//! height, falloff). A `symmetry` dropdown sits above the canvas so
//! authors can mirror / rotate the placed shapes into the symmetric
//! orbit the executor uses.
//!
//! Undo: each gesture (add / drag / delete) produces exactly one undo
//! entry via the `field_edit_in_progress` snapshot pattern. The old
//! per-frame `push_undo` during slider drags is gone.

use std::collections::HashMap;
use std::f32::consts::PI;

use bar_graph::{NodeId, ParamValue};
use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::properties::properties_canvas::{
    self, CanvasGesture, CanvasState, CanvasTransform, HandleId, HandleSpec,
};
use crate::panels::widgets::ParamSlider;

// Handle id slots used by this panel. Centre = 0, four corners
// 1..=4, rotation = 5. Kept private here -- the canvas widget only
// cares about identity equality.
const HANDLE_CENTRE: HandleId = HandleId(0);
const HANDLE_TL: HandleId = HandleId(1);
const HANDLE_TR: HandleId = HandleId(2);
const HANDLE_BL: HandleId = HandleId(3);
const HANDLE_BR: HandleId = HandleId(4);
const HANDLE_ROT: HandleId = HandleId(5);

const MAX_SHAPES: usize = 8;

/// Snapshot of one shape's full param set as a struct so the canvas
/// logic doesn't have to thread eight individual params through
/// every event.
#[derive(Clone, Debug)]
struct ShapeRow {
    shape_type: String,
    x: f32,
    y: f32,
    rx: f32,
    ry: f32,
    angle: f32,
    height: f32,
    falloff: f32,
}

impl ShapeRow {
    fn read(params: &HashMap<String, ParamValue>, i: usize) -> Self {
        let shape_type = match params.get(&format!("type_{i}")) {
            Some(ParamValue::String(s)) => s.clone(),
            _ => "ellipse".to_string(),
        };
        Self {
            shape_type,
            x: get_f(params, &format!("x_{i}"), 0.5),
            y: get_f(params, &format!("y_{i}"), 0.5),
            rx: get_f(params, &format!("rx_{i}"), 0.2),
            ry: get_f(params, &format!("ry_{i}"), 0.2),
            angle: get_f(params, &format!("angle_{i}"), 0.0),
            height: get_f(params, &format!("height_{i}"), 0.5),
            falloff: get_f(params, &format!("falloff_{i}"), 0.5),
        }
    }
}

impl BarEditorApp {
    pub(crate) fn draw_layout_generator_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        params: &HashMap<String, ParamValue>,
    ) {
        let mut shape_count = match params.get("shape_count") {
            Some(ParamValue::UInt(n)) => (*n).clamp(1, MAX_SHAPES as u32) as usize,
            _ => 1,
        };
        let mut symmetry = match params.get("symmetry") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => "none".to_string(),
        };
        // Read every shape's params into a working buffer so the
        // canvas code mutates a local struct instead of weaving
        // through HashMap keys per gesture.
        let mut shapes: Vec<ShapeRow> = (0..shape_count)
            .map(|i| ShapeRow::read(params, i))
            .collect();

        // Top row: symmetry + shape-count indicator.
        ui.horizontal(|ui| {
            ui.label("symmetry");
            egui::ComboBox::from_id_salt(("lg_symmetry", node_id.0))
                .selected_text(&symmetry)
                .show_ui(ui, |ui| {
                    for choice in [
                        "none",
                        "mirror_x",
                        "mirror_y",
                        "mirror_xy",
                        "rotate_180",
                        "rotate_90",
                    ] {
                        if ui.selectable_label(symmetry == choice, choice).clicked() {
                            symmetry = choice.to_string();
                        }
                    }
                });
            ui.add_space(20.0);
            ui.label(format!("{shape_count} / {MAX_SHAPES} shapes"));
        });

        // Square canvas size. Lower floor (280px) keeps the panel
        // usable in narrow side-panel docks; the previous 360 floor
        // pushed the sidebar below the visible viewport on standard
        // panel widths and forced a vertical scrollbar even at
        // baseline.
        let canvas_size = ui.available_width().clamp(280.0, 500.0);

        let state_id = egui::Id::new(("lg_canvas_state", node_id.0));
        let mut state: CanvasState = ui
            .data(|d| d.get_temp::<CanvasState>(state_id))
            .unwrap_or_default();

        // Build the handle list. Five handles per shape: centre, 4
        // corners, rotation arm. Corners and rotation are at radius
        // offsets so they don't pile on top of the centre handle for
        // small shapes -- enforce a minimum pixel offset on the
        // rotation arm to keep it visible. Done in normalised coords
        // here; pixel offsets get applied via the rotation_arm
        // helper below.
        let mut handles: Vec<HandleSpec> = Vec::new();
        for (i, s) in shapes.iter().enumerate() {
            let (cosa, sina) = (s.angle * PI / 180.0).sin_cos();
            // Local-to-world frame: x' = cos * rx, y' = sin * rx (etc.)
            // The four corner handles sit at (+/-rx, +/-ry) in shape-local
            // coords, rotated by `angle`.
            let corner = |lx: f32, ly: f32| -> [f32; 2] {
                let wx = s.x + lx * cosa - ly * sina;
                let wy = s.y + lx * sina + ly * cosa;
                [wx.clamp(-0.5, 1.5), wy.clamp(-0.5, 1.5)]
            };
            handles.push(HandleSpec {
                item: i,
                id: HANDLE_CENTRE,
                pos: [s.x, s.y],
                px_radius: 10.0,
            });
            handles.push(HandleSpec {
                item: i,
                id: HANDLE_TL,
                pos: corner(-s.rx, -s.ry),
                px_radius: 8.0,
            });
            handles.push(HandleSpec {
                item: i,
                id: HANDLE_TR,
                pos: corner(s.rx, -s.ry),
                px_radius: 8.0,
            });
            handles.push(HandleSpec {
                item: i,
                id: HANDLE_BL,
                pos: corner(-s.rx, s.ry),
                px_radius: 8.0,
            });
            handles.push(HandleSpec {
                item: i,
                id: HANDLE_BR,
                pos: corner(s.rx, s.ry),
                px_radius: 8.0,
            });
            // Rotation arm sits offset along local +x axis at
            // 1.3 * rx so it's outside the silhouette.
            handles.push(HandleSpec {
                item: i,
                id: HANDLE_ROT,
                pos: corner(s.rx * 1.3 + 0.02, 0.0),
                px_radius: 8.0,
            });
        }

        // Snapshot of shapes captured before any draw_callback paints
        // them. The draw callback is FnOnce so we can't read shapes
        // from inside; clone the data the callback needs upfront.
        let shapes_for_draw = shapes.clone();
        let selected_for_draw = state.selected;

        let gestures = properties_canvas::draw(
            ui,
            egui::vec2(canvas_size, canvas_size),
            &mut state,
            &handles,
            move |painter, xform| {
                draw_shapes(painter, xform, &shapes_for_draw, selected_for_draw);
            },
        );

        // Apply gestures to the shapes buffer + symmetry buffer.
        let mut mutated = false;
        let mut needs_undo_snapshot = false;
        let mut commit_undo_now = false;
        for g in gestures {
            match g {
                CanvasGesture::AddAt { pos } => {
                    if shapes.len() < MAX_SHAPES {
                        shapes.push(ShapeRow {
                            shape_type: "ellipse".to_string(),
                            x: pos[0],
                            y: pos[1],
                            rx: 0.1,
                            ry: 0.1,
                            angle: 0.0,
                            height: 0.5,
                            falloff: 0.5,
                        });
                        shape_count = shapes.len();
                        state.selected = Some(shape_count - 1);
                        mutated = true;
                        commit_undo_now = true;
                    }
                }
                CanvasGesture::HandlePressed { item, .. } => {
                    state.selected = Some(item);
                    needs_undo_snapshot = true;
                }
                CanvasGesture::HandleDragged { item, handle, pos } => {
                    if let Some(s) = shapes.get_mut(item) {
                        apply_drag(s, handle, pos);
                        mutated = true;
                    }
                }
                CanvasGesture::HandleReleased { .. } => {
                    if self.dialog.field_edit_in_progress.is_some() {
                        commit_undo_now = true;
                    }
                }
                CanvasGesture::HandleDeleted { item } => {
                    if item < shapes.len() {
                        shapes.remove(item);
                        shape_count = shapes.len();
                        if state.selected == Some(item) {
                            state.selected = None;
                        } else if let Some(sel) = state.selected {
                            if sel > item {
                                state.selected = Some(sel - 1);
                            }
                        }
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

        // Sidebar: selected shape's non-spatial params.
        ui.add_space(6.0);
        if let Some(sel) = state.selected {
            if let Some(s) = shapes.get_mut(sel) {
                ui.label(format!("Shape {sel} ({})", s.shape_type));
                egui::Grid::new(("lg_sidebar", node_id.0))
                    .num_columns(2)
                    .spacing([8.0, 3.0])
                    .show(ui, |ui| {
                        ui.label("type");
                        egui::ComboBox::from_id_salt(("lg_type", node_id.0, sel as u64))
                            .selected_text(&s.shape_type)
                            .show_ui(ui, |ui| {
                                for choice in ["ellipse", "rectangle", "ridge"] {
                                    if ui
                                        .selectable_label(s.shape_type == choice, choice)
                                        .clicked()
                                    {
                                        s.shape_type = choice.to_string();
                                        mutated = true;
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label("height");
                        let resp = ui.add(ParamSlider::new(&mut s.height, 0.0, 1.0));
                        if (resp.drag_started() || resp.gained_focus())
                            && self.dialog.field_edit_in_progress.is_none()
                        {
                            let snap = self.snapshot("Layout edit");
                            self.dialog.field_edit_in_progress = Some(snap);
                        }
                        if resp.changed() {
                            mutated = true;
                        }
                        if resp.drag_stopped() || resp.lost_focus() {
                            commit_undo_now = true;
                        }
                        ui.end_row();

                        ui.label("falloff");
                        let resp = ui.add(ParamSlider::new(&mut s.falloff, 0.0, 1.0));
                        if (resp.drag_started() || resp.gained_focus())
                            && self.dialog.field_edit_in_progress.is_none()
                        {
                            let snap = self.snapshot("Layout edit");
                            self.dialog.field_edit_in_progress = Some(snap);
                        }
                        if resp.changed() {
                            mutated = true;
                        }
                        if resp.drag_stopped() || resp.lost_focus() {
                            commit_undo_now = true;
                        }
                        ui.end_row();
                    });
            }
        } else {
            ui.label("Click a shape to edit, or click empty canvas to add.");
        }

        // Persist canvas state.
        ui.data_mut(|d| d.insert_temp::<CanvasState>(state_id, state));

        // Non-canvas changes (the symmetry dropdown above the canvas)
        // don't emit gestures, so diff against the original params and
        // promote any change to `mutated` + `commit_undo_now`.
        let original_symmetry = match params.get("symmetry") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => "none".to_string(),
        };
        if symmetry != original_symmetry {
            mutated = true;
            commit_undo_now = true;
        }

        // Undo for atomic ops (add / delete / symmetry change) must
        // capture the PRE-mutation graph state. Take the snapshot
        // BEFORE writing new params; drag ends use the snapshot
        // stashed at HandlePressed instead.
        let want_atomic_undo = commit_undo_now && self.dialog.field_edit_in_progress.is_none();
        if want_atomic_undo {
            self.push_undo("Layout edit");
        }

        if mutated {
            if let Some(node) = self.graph.get_node_mut(node_id) {
                node.params.insert(
                    "shape_count".to_string(),
                    ParamValue::UInt(shape_count as u32),
                );
                node.params
                    .insert("symmetry".to_string(), ParamValue::String(symmetry.clone()));
                for (i, s) in shapes.iter().enumerate() {
                    node.params.insert(
                        format!("type_{i}"),
                        ParamValue::String(s.shape_type.clone()),
                    );
                    node.params.insert(format!("x_{i}"), ParamValue::Float(s.x));
                    node.params.insert(format!("y_{i}"), ParamValue::Float(s.y));
                    node.params
                        .insert(format!("rx_{i}"), ParamValue::Float(s.rx));
                    node.params
                        .insert(format!("ry_{i}"), ParamValue::Float(s.ry));
                    node.params
                        .insert(format!("angle_{i}"), ParamValue::Float(s.angle));
                    node.params
                        .insert(format!("height_{i}"), ParamValue::Float(s.height));
                    node.params
                        .insert(format!("falloff_{i}"), ParamValue::Float(s.falloff));
                }
                // Zero-height the slots beyond the active count so a
                // future shape_count bump doesn't surface stale data.
                for i in shapes.len()..MAX_SHAPES {
                    node.params
                        .insert(format!("height_{i}"), ParamValue::Float(0.0));
                }
                node.mark_dirty();
            }
        }

        // Drag-end: push the snapshot captured at drag-start. Skipped
        // when atomic-undo already pushed an entry above.
        if commit_undo_now && !want_atomic_undo {
            if let Some(snap) = self.dialog.field_edit_in_progress.take() {
                self.history.push(snap);
                self.project.is_dirty = true;
            }
        }
    }
}

fn apply_drag(s: &mut ShapeRow, handle: HandleId, pos: [f32; 2]) {
    match handle {
        HANDLE_CENTRE => {
            s.x = pos[0].clamp(0.0, 1.0);
            s.y = pos[1].clamp(0.0, 1.0);
        }
        HANDLE_TL | HANDLE_TR | HANDLE_BL | HANDLE_BR => {
            // Inverse-rotate the drag position back into shape-local
            // space; the corner's local coords are (+/- rx, +/- ry).
            let (cosa, sina) = (s.angle * PI / 180.0).sin_cos();
            let dx = pos[0] - s.x;
            let dy = pos[1] - s.y;
            let local_x = dx * cosa + dy * sina;
            let local_y = -dx * sina + dy * cosa;
            // Each corner controls a sign for rx / ry. Magnitudes
            // are abs of local coords, clamped so the shape doesn't
            // collapse to zero size (the falloff math degenerates).
            s.rx = local_x.abs().clamp(0.01, 1.0);
            s.ry = local_y.abs().clamp(0.01, 1.0);
        }
        HANDLE_ROT => {
            // Rotation arm: the cursor angle relative to the shape's
            // centre is the new rotation.
            let dx = pos[0] - s.x;
            let dy = pos[1] - s.y;
            s.angle = dy.atan2(dx).to_degrees().rem_euclid(360.0);
        }
        _ => {}
    }
}

fn draw_shapes(
    painter: &egui::Painter,
    xform: &CanvasTransform,
    shapes: &[ShapeRow],
    selected: Option<usize>,
) {
    for (i, s) in shapes.iter().enumerate() {
        let centre = xform.to_pixel([s.x, s.y]);
        let is_sel = selected == Some(i);
        let stroke_col = if is_sel {
            egui::Color32::from_rgb(255, 200, 60)
        } else {
            egui::Color32::from_rgb(180, 180, 200)
        };
        let stroke = egui::Stroke::new(if is_sel { 2.0 } else { 1.0 }, stroke_col);

        // Common: outline plus a line marking the shape's local +x
        // axis (so rotation is visible).
        let (cosa, sina) = (s.angle * PI / 180.0).sin_cos();
        let to_world = |lx: f32, ly: f32| -> egui::Pos2 {
            xform.to_pixel([s.x + lx * cosa - ly * sina, s.y + lx * sina + ly * cosa])
        };

        match s.shape_type.as_str() {
            "rectangle" => {
                let p0 = to_world(-s.rx, -s.ry);
                let p1 = to_world(s.rx, -s.ry);
                let p2 = to_world(s.rx, s.ry);
                let p3 = to_world(-s.rx, s.ry);
                painter.line_segment([p0, p1], stroke);
                painter.line_segment([p1, p2], stroke);
                painter.line_segment([p2, p3], stroke);
                painter.line_segment([p3, p0], stroke);
            }
            "ridge" => {
                // The "ridge" type's falloff math is `ly.abs()` -- an
                // infinite line along the shape's local x axis. Draw
                // a polyline of finite length to suggest direction.
                let p0 = to_world(-2.0, 0.0);
                let p1 = to_world(2.0, 0.0);
                painter.line_segment([p0, p1], stroke);
                // Width markers (top + bottom of the falloff envelope).
                let pa = to_world(-s.rx * 4.0, -s.ry);
                let pb = to_world(s.rx * 4.0, -s.ry);
                let pc = to_world(-s.rx * 4.0, s.ry);
                let pd = to_world(s.rx * 4.0, s.ry);
                let thin = egui::Stroke::new(0.5, stroke_col.gamma_multiply(0.5));
                painter.line_segment([pa, pb], thin);
                painter.line_segment([pc, pd], thin);
            }
            _ => {
                // Ellipse: approximate by a polyline of 24 samples.
                let mut prev = to_world(s.rx, 0.0);
                for k in 1..=24 {
                    let t = k as f32 / 24.0 * std::f32::consts::TAU;
                    let next = to_world(s.rx * t.cos(), s.ry * t.sin());
                    painter.line_segment([prev, next], stroke);
                    prev = next;
                }
            }
        }

        // Local +x arm to indicate rotation.
        let arm = egui::Stroke::new(1.0, stroke_col.gamma_multiply(0.7));
        painter.line_segment([centre, to_world(s.rx, 0.0)], arm);
    }
}

fn get_f(params: &HashMap<String, ParamValue>, key: &str, default: f32) -> f32 {
    match params.get(key) {
        Some(ParamValue::Float(v)) => *v,
        _ => default,
    }
}

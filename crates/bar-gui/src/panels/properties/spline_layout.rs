//! Contextual properties for the SplineLayout node.
//!
//! 2D canvas editor: control points appear as discs, the Catmull-Rom
//! curve through them as a polyline. Click empty canvas to append a
//! point; drag a point to move it; right-click to delete. Sidebar
//! exposes the node's top-level params (mode, amplitude, width,
//! falloff, closed, symmetry) -- there's no per-point sidebar
//! because each point is just a position.
//!
//! Undo follows the same `field_edit_in_progress` pattern as the
//! LayoutGenerator panel: one entry per gesture.

use std::collections::HashMap;

use bar_graph::{NodeId, ParamValue};
use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::properties::properties_canvas::{
    self, CanvasGesture, CanvasState, CanvasTransform, HandleId, HandleSpec,
};
use crate::panels::widgets::ParamSlider;

const HANDLE_POINT: HandleId = HandleId(0);

impl BarEditorApp {
    pub(crate) fn draw_spline_layout_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        params: &HashMap<String, ParamValue>,
    ) {
        // Pull current values into a working buffer.
        let mut points: Vec<[f32; 2]> = match params.get("points") {
            Some(ParamValue::Spline(p)) => p.clone(),
            _ => Vec::new(),
        };
        let mut mode = match params.get("mode") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => "ridge".to_string(),
        };
        let mut symmetry = match params.get("symmetry") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => "none".to_string(),
        };
        let mut closed = matches!(params.get("closed"), Some(ParamValue::Bool(true)));
        let mut amplitude = get_f(params, "amplitude", 0.5);
        let mut width_n = get_f(params, "width", 0.05);
        let mut falloff = get_f(params, "falloff", 0.5);

        // Top row: mode + closed + symmetry.
        ui.horizontal(|ui| {
            ui.label("mode");
            egui::ComboBox::from_id_salt(("sl_mode", node_id.0))
                .selected_text(&mode)
                .show_ui(ui, |ui| {
                    for choice in ["ridge", "valley", "mask"] {
                        if ui.selectable_label(mode == choice, choice).clicked() {
                            mode = choice.to_string();
                        }
                    }
                });
            ui.add_space(12.0);
            ui.checkbox(&mut closed, "closed");
            ui.add_space(12.0);
            ui.label("symmetry");
            egui::ComboBox::from_id_salt(("sl_symmetry", node_id.0))
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
        });

        // Square canvas.
        let canvas_size = ui.available_width().min(400.0);
        let (canvas_rect, _) =
            ui.allocate_exact_size(egui::vec2(canvas_size, canvas_size), egui::Sense::hover());

        let state_id = egui::Id::new(("sl_canvas_state", node_id.0));
        let mut state: CanvasState = ui
            .data(|d| d.get_temp::<CanvasState>(state_id))
            .unwrap_or_default();

        // One handle per control point.
        let handles: Vec<HandleSpec> = points
            .iter()
            .enumerate()
            .map(|(i, p)| HandleSpec {
                item: i,
                id: HANDLE_POINT,
                pos: *p,
                px_radius: 6.0,
            })
            .collect();

        let points_for_draw = points.clone();
        let closed_for_draw = closed;
        let selected_for_draw = state.selected;

        let gestures = properties_canvas::draw(
            ui,
            canvas_rect,
            &mut state,
            &handles,
            move |painter, xform| {
                draw_spline(
                    painter,
                    xform,
                    &points_for_draw,
                    closed_for_draw,
                    selected_for_draw,
                );
            },
        );

        let mut mutated = false;
        let mut needs_undo_snapshot = false;
        let mut commit_undo_now = false;
        for g in gestures {
            match g {
                CanvasGesture::AddAt { pos } => {
                    points.push(pos);
                    state.selected = Some(points.len() - 1);
                    mutated = true;
                    commit_undo_now = true;
                }
                CanvasGesture::HandlePressed { item, .. } => {
                    state.selected = Some(item);
                    needs_undo_snapshot = true;
                }
                CanvasGesture::HandleDragged { item, pos, .. } => {
                    if let Some(p) = points.get_mut(item) {
                        p[0] = pos[0].clamp(0.0, 1.0);
                        p[1] = pos[1].clamp(0.0, 1.0);
                        mutated = true;
                    }
                }
                CanvasGesture::HandleReleased { .. } => {
                    if self.dialog.field_edit_in_progress.is_some() {
                        commit_undo_now = true;
                    }
                }
                CanvasGesture::HandleDeleted { item } => {
                    if item < points.len() {
                        points.remove(item);
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
            let snap = self.snapshot("Spline edit");
            self.dialog.field_edit_in_progress = Some(snap);
        }

        // Sidebar sliders.
        ui.add_space(6.0);
        ui.label(format!("{} control points", points.len()));
        egui::Grid::new(("sl_sidebar", node_id.0))
            .num_columns(2)
            .spacing([8.0, 3.0])
            .show(ui, |ui| {
                ui.label("amplitude");
                let resp = ui.add(ParamSlider::new(&mut amplitude, 0.0, 1.0));
                if (resp.drag_started() || resp.gained_focus())
                    && self.dialog.field_edit_in_progress.is_none()
                {
                    let snap = self.snapshot("Spline edit");
                    self.dialog.field_edit_in_progress = Some(snap);
                }
                if resp.changed() {
                    mutated = true;
                }
                if resp.drag_stopped() || resp.lost_focus() {
                    commit_undo_now = true;
                }
                ui.end_row();

                ui.label("width");
                let resp = ui.add(ParamSlider::new(&mut width_n, 0.001, 0.5));
                if (resp.drag_started() || resp.gained_focus())
                    && self.dialog.field_edit_in_progress.is_none()
                {
                    let snap = self.snapshot("Spline edit");
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
                let resp = ui.add(ParamSlider::new(&mut falloff, 0.0, 1.0));
                if (resp.drag_started() || resp.gained_focus())
                    && self.dialog.field_edit_in_progress.is_none()
                {
                    let snap = self.snapshot("Spline edit");
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

        // Detect dropdown / checkbox changes -- they don't emit
        // canvas gestures, so we compare against the original values.
        let original_mode = match params.get("mode") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => "ridge".to_string(),
        };
        let original_sym = match params.get("symmetry") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => "none".to_string(),
        };
        let original_closed = matches!(params.get("closed"), Some(ParamValue::Bool(true)));
        if mode != original_mode || symmetry != original_sym || closed != original_closed {
            mutated = true;
            if !commit_undo_now && self.dialog.field_edit_in_progress.is_none() {
                self.push_undo("Spline edit");
            }
        }

        ui.data_mut(|d| d.insert_temp::<CanvasState>(state_id, state));

        if mutated {
            if let Some(node) = self.graph.get_node_mut(node_id) {
                node.params
                    .insert("points".to_string(), ParamValue::Spline(points));
                node.params
                    .insert("mode".to_string(), ParamValue::String(mode));
                node.params
                    .insert("symmetry".to_string(), ParamValue::String(symmetry));
                node.params
                    .insert("closed".to_string(), ParamValue::Bool(closed));
                node.params
                    .insert("amplitude".to_string(), ParamValue::Float(amplitude));
                node.params
                    .insert("width".to_string(), ParamValue::Float(width_n));
                node.params
                    .insert("falloff".to_string(), ParamValue::Float(falloff));
                node.mark_dirty();
            }
        }

        if commit_undo_now {
            if let Some(snap) = self.dialog.field_edit_in_progress.take() {
                self.history.push(snap);
                self.project.is_dirty = true;
            } else {
                self.push_undo("Spline edit");
            }
        }
    }
}

/// Draw the Catmull-Rom spline through the control points plus a
/// polyline of segment samples. Selected point gets a highlighted
/// stroke. Mirrors the executor's centripetal Catmull-Rom convention.
fn draw_spline(
    painter: &egui::Painter,
    xform: &CanvasTransform,
    points: &[[f32; 2]],
    closed: bool,
    _selected: Option<usize>,
) {
    if points.len() < 2 {
        return;
    }
    let stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(140, 200, 255));
    let samples = sample_catmull_rom(points, 24, closed);
    let mut prev = xform.to_pixel(samples[0]);
    for s in samples.iter().skip(1) {
        let p = xform.to_pixel(*s);
        painter.line_segment([prev, p], stroke);
        prev = p;
    }
}

fn sample_catmull_rom(points: &[[f32; 2]], n: usize, closed: bool) -> Vec<[f32; 2]> {
    let m = points.len();
    if m < 2 {
        return points.to_vec();
    }
    let seg_count = if closed { m } else { m - 1 };
    let mut out = Vec::with_capacity(seg_count * n + 1);
    for i in 0..seg_count {
        let p0 = if closed {
            points[(i + m - 1) % m]
        } else if i == 0 {
            [
                2.0 * points[i][0] - points[i + 1][0],
                2.0 * points[i][1] - points[i + 1][1],
            ]
        } else {
            points[i - 1]
        };
        let p1 = points[i];
        let p2 = points[if closed { (i + 1) % m } else { i + 1 }];
        let p3 = if closed {
            points[(i + 2) % m]
        } else if i + 2 >= m {
            [2.0 * p2[0] - p1[0], 2.0 * p2[1] - p1[1]]
        } else {
            points[i + 2]
        };
        for s in 0..n {
            let t = s as f32 / n as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            let cx = 0.5
                * ((2.0 * p1[0])
                    + (-p0[0] + p2[0]) * t
                    + (2.0 * p0[0] - 5.0 * p1[0] + 4.0 * p2[0] - p3[0]) * t2
                    + (-p0[0] + 3.0 * p1[0] - 3.0 * p2[0] + p3[0]) * t3);
            let cy = 0.5
                * ((2.0 * p1[1])
                    + (-p0[1] + p2[1]) * t
                    + (2.0 * p0[1] - 5.0 * p1[1] + 4.0 * p2[1] - p3[1]) * t2
                    + (-p0[1] + 3.0 * p1[1] - 3.0 * p2[1] + p3[1]) * t3);
            out.push([cx, cy]);
        }
    }
    if !closed {
        out.push(points[m - 1]);
    }
    out
}

fn get_f(params: &HashMap<String, ParamValue>, key: &str, default: f32) -> f32 {
    match params.get(key) {
        Some(ParamValue::Float(v)) => *v,
        _ => default,
    }
}

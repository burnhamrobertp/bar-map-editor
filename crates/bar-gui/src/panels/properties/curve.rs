//! Contextual properties for the Curve node: an interactive transfer-curve
//! editor. Input runs along X (0..1), output along Y (0..1). Drag a point to
//! move it, click empty space to add one, right-click a point to remove it
//! (minimum 2). Endpoints are pinned in X. Control points live in
//! params["num_points"] + params["p{i}_x"/"p{i}_y"] and the drawn polyline is
//! the exact piecewise-linear function the executor evaluates.

use bar_graph::{NodeId, ParamValue};
use eframe::egui;

use crate::app::BarEditorApp;

const MAX_POINTS: usize = 12;
const GRAB_RADIUS: f32 = 11.0;

/// Executor default when a node has no explicit points yet (smoothstep S-curve),
/// so the editor shows exactly what the graph currently computes.
fn default_points() -> Vec<(f32, f32)> {
    vec![(0.0, 0.0), (0.25, 0.1), (0.5, 0.5), (0.75, 0.9), (1.0, 1.0)]
}

fn get_f(params: &std::collections::HashMap<String, ParamValue>, key: &str, d: f32) -> f32 {
    match params.get(key) {
        Some(ParamValue::Float(v)) => *v,
        _ => d,
    }
}

/// Index + pixel distance of the control point nearest `sp` (screen space).
fn nearest_point(pts: &[(f32, f32)], rect: egui::Rect, sp: egui::Pos2) -> Option<(usize, f32)> {
    pts.iter()
        .enumerate()
        .map(|(i, &(x, y))| {
            let c = egui::pos2(
                rect.left() + x * rect.width(),
                rect.bottom() - y * rect.height(),
            );
            (i, (c - sp).length())
        })
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

impl BarEditorApp {
    pub(crate) fn draw_curve_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        params: &std::collections::HashMap<String, ParamValue>,
    ) {
        // Read control points (or the executor's default S-curve if unset).
        let mut pts: Vec<(f32, f32)> = if params.contains_key("p0_x") {
            let n = match params.get("num_points") {
                Some(ParamValue::UInt(n)) => (*n).max(2) as usize,
                _ => 2,
            };
            (0..n)
                .map(|i| {
                    let x = get_f(params, &format!("p{i}_x"), i as f32 / (n - 1) as f32);
                    let y = get_f(params, &format!("p{i}_y"), x);
                    (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0))
                })
                .collect()
        } else {
            default_points()
        };
        pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let size = ui.available_width().clamp(160.0, 248.0);
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click_and_drag());

        let to_screen = |p: (f32, f32)| {
            egui::pos2(
                rect.left() + p.0 * rect.width(),
                rect.bottom() - p.1 * rect.height(),
            )
        };
        let to_data = |sp: egui::Pos2| {
            (
                ((sp.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
                ((rect.bottom() - sp.y) / rect.height()).clamp(0.0, 1.0),
            )
        };

        let grab_id = egui::Id::new(("curve_grab", node_id.0));
        let mut grabbed: Option<usize> = ui.data(|d| d.get_temp(grab_id)).flatten();

        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            p.rect_filled(rect, 2.0, egui::Color32::from_gray(24));
            for i in 1..4 {
                let t = i as f32 / 4.0;
                let gx = rect.left() + t * rect.width();
                let gy = rect.top() + t * rect.height();
                let g = egui::Stroke::new(0.5, egui::Color32::from_gray(45));
                p.line_segment(
                    [egui::pos2(gx, rect.top()), egui::pos2(gx, rect.bottom())],
                    g,
                );
                p.line_segment(
                    [egui::pos2(rect.left(), gy), egui::pos2(rect.right(), gy)],
                    g,
                );
            }
            // identity reference
            p.line_segment(
                [to_screen((0.0, 0.0)), to_screen((1.0, 1.0))],
                egui::Stroke::new(1.0, egui::Color32::from_gray(70)),
            );
            // the curve
            let poly: Vec<egui::Pos2> = pts.iter().map(|&pt| to_screen(pt)).collect();
            p.add(egui::Shape::line(
                poly,
                egui::Stroke::new(1.6, egui::Color32::from_rgb(130, 180, 255)),
            ));
            p.rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(60)),
                egui::StrokeKind::Outside,
            );
            for (i, &pt) in pts.iter().enumerate() {
                let c = to_screen(pt);
                let fill = if Some(i) == grabbed {
                    egui::Color32::WHITE
                } else {
                    egui::Color32::from_gray(210)
                };
                p.circle_filled(c, 4.0, fill);
                p.circle_stroke(c, 4.0, egui::Stroke::new(1.0, egui::Color32::from_gray(40)));
            }
        }

        let mut changed = false;
        let pointer = resp.interact_pointer_pos();

        if resp.drag_started() {
            grabbed = pointer
                .and_then(|sp| nearest_point(&pts, rect, sp))
                .filter(|&(_, d)| d <= GRAB_RADIUS)
                .map(|(i, _)| i);
            ui.data_mut(|d| d.insert_temp(grab_id, grabbed));
        }
        if resp.dragged() {
            if let (Some(gi), Some(sp)) = (grabbed, pointer) {
                let (mut x, y) = to_data(sp);
                if gi == 0 {
                    x = 0.0;
                } else if gi == pts.len() - 1 {
                    x = 1.0;
                }
                pts[gi] = (x, y);
                changed = true;
            }
        }
        if resp.drag_stopped() {
            ui.data_mut(|d| d.insert_temp::<Option<usize>>(grab_id, None));
        }

        // Click on empty space -> add a point there.
        if resp.clicked() {
            if let Some(sp) = pointer {
                let on_handle = nearest_point(&pts, rect, sp)
                    .map(|(_, d)| d <= 8.0)
                    .unwrap_or(false);
                if !on_handle && pts.len() < MAX_POINTS {
                    let (x, y) = to_data(sp);
                    pts.push((x.clamp(0.001, 0.999), y));
                    changed = true;
                }
            }
        }
        // Right-click a point -> delete it.
        if resp.secondary_clicked() {
            if let Some((i, d)) = pointer.and_then(|sp| nearest_point(&pts, rect, sp)) {
                if d <= GRAB_RADIUS && pts.len() > 2 {
                    pts.remove(i);
                    changed = true;
                }
            }
        }

        ui.add_space(2.0);
        ui.label(
            egui::RichText::new("drag to move - click to add - right-click to remove")
                .weak()
                .small(),
        );
        if ui.small_button("Reset to linear").clicked() {
            pts = vec![(0.0, 0.0), (1.0, 1.0)];
            changed = true;
        }

        if changed {
            pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            self.push_undo("Edit curve");
            if let Some(node) = self.graph.get_node_mut(node_id) {
                node.params
                    .insert("num_points".to_string(), ParamValue::UInt(pts.len() as u32));
                for (i, &(x, y)) in pts.iter().enumerate() {
                    node.params.insert(format!("p{i}_x"), ParamValue::Float(x));
                    node.params.insert(format!("p{i}_y"), ParamValue::Float(y));
                }
                node.mark_dirty();
            }
        }
    }
}

//! Contextual properties for the ColorRamp node.
//!
//! Shows a gradient preview bar with draggable stop handles, a
//! color picker for the selected stop, and add/delete controls.
//! Stop positions live in params["pos_i"] and colors in params["color_i"];
//! stop_count determines how many are active. Max 8 stops, min 2.

use bar_graph::{NodeId, ParamValue};
use eframe::egui;

use crate::app::BarEditorApp;

impl BarEditorApp {
    pub(crate) fn draw_color_ramp_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        params: &std::collections::HashMap<String, ParamValue>,
    ) {
        let mut changed: Vec<(String, ParamValue)> = Vec::new();

        let stop_count = match params.get("stop_count") {
            Some(ParamValue::UInt(n)) => (*n).clamp(2, 8) as usize,
            _ => 2,
        };

        // Read current stops from params.
        let mut stops: Vec<(f32, [u8; 3])> = (0..stop_count)
            .map(|i| {
                let pos = match params.get(&format!("pos_{i}")) {
                    Some(ParamValue::Float(v)) => v.clamp(0.0, 1.0),
                    _ => i as f32 / (stop_count - 1).max(1) as f32,
                };
                let hex = match params.get(&format!("color_{i}")) {
                    Some(ParamValue::String(s)) => s.clone(),
                    _ => "808080".to_string(),
                };
                let rgb = parse_hex(&hex).unwrap_or([128u8, 128, 128]);
                (pos, rgb)
            })
            .collect();

        // Sort stops by position for display purposes; track which param
        // index each sorted slot corresponds to.
        let mut order: Vec<usize> = (0..stop_count).collect();
        order.sort_by(|&a, &b| {
            stops[a]
                .0
                .partial_cmp(&stops[b].0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Temp memory key for selected stop (by display order index).
        let sel_id = egui::Id::new(("cr_sel", node_id.0));
        let mut sel_order_idx = ui
            .data(|d| d.get_temp::<usize>(sel_id))
            .unwrap_or(0)
            .min(stop_count.saturating_sub(1));

        // --- Gradient preview bar ---
        let bar_height = 24.0;
        let handle_zone = 16.0;
        let total_h = bar_height + handle_zone;
        let (bar_rect, bar_resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), total_h),
            egui::Sense::click(),
        );
        let bar_body =
            egui::Rect::from_min_size(bar_rect.min, egui::vec2(bar_rect.width(), bar_height));

        if ui.is_rect_visible(bar_rect) {
            let painter = ui.painter();

            // Paint gradient by sampling ~1px wide strips.
            let w = bar_rect.width() as usize;
            for px in 0..w {
                let t = px as f32 / (w - 1).max(1) as f32;
                let color = sample_gradient(&stops, &order, t);
                let strip = egui::Rect::from_min_size(
                    egui::pos2(bar_rect.left() + px as f32, bar_rect.top()),
                    egui::vec2(1.5, bar_height),
                );
                painter.rect_filled(
                    strip,
                    0.0,
                    egui::Color32::from_rgb(color[0], color[1], color[2]),
                );
            }
            // Border
            painter.rect_stroke(
                bar_body,
                2.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(60)),
                egui::StrokeKind::Outside,
            );

            // Handle zone: triangular markers below the bar.
            for (di, &pi) in order.iter().enumerate() {
                let x = bar_rect.left() + stops[pi].0 * bar_rect.width();
                let y_tip = bar_rect.top() + bar_height;
                let y_base = y_tip + handle_zone - 2.0;
                let is_sel = di == sel_order_idx;
                let fill = if is_sel {
                    egui::Color32::WHITE
                } else {
                    egui::Color32::from_gray(180)
                };
                let border = egui::Color32::from_gray(40);
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        egui::pos2(x, y_tip + 1.0),
                        egui::pos2(x - 5.0, y_base),
                        egui::pos2(x + 5.0, y_base),
                    ],
                    fill,
                    egui::Stroke::new(1.0, border),
                ));
            }
        }

        // Click on bar body -> add new stop at that position (if room).
        if bar_resp.clicked() {
            if let Some(pos) = bar_resp.interact_pointer_pos() {
                let t = ((pos.x - bar_rect.left()) / bar_rect.width()).clamp(0.0, 1.0);
                let in_bar = pos.y <= bar_rect.top() + bar_height;
                if in_bar && stop_count < 8 {
                    // Interpolate color at position t.
                    let [r, g, b] = sample_gradient(&stops, &order, t);
                    let new_idx = stop_count;
                    changed.push((format!("pos_{new_idx}"), ParamValue::Float(t)));
                    changed.push((
                        format!("color_{new_idx}"),
                        ParamValue::String(format!("{r:02X}{g:02X}{b:02X}")),
                    ));
                    changed.push((
                        "stop_count".to_string(),
                        ParamValue::UInt((stop_count + 1) as u32),
                    ));
                    // Select the new stop (last in sorted order after sort).
                    let new_sel = order
                        .iter()
                        .position(|_| true) // just pick 0; will be updated next frame
                        .unwrap_or(0);
                    sel_order_idx = new_sel;
                } else {
                    // Click in handle zone -- find closest handle.
                    let t_click = ((pos.x - bar_rect.left()) / bar_rect.width()).clamp(0.0, 1.0);
                    if let Some((di, _)) = order.iter().enumerate().min_by(|(_, &a), (_, &b)| {
                        let da = (stops[a].0 - t_click).abs();
                        let db = (stops[b].0 - t_click).abs();
                        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                    }) {
                        sel_order_idx = di;
                    }
                }
            }
        }

        // Drag on handle zone -> move selected stop.
        let handle_area = egui::Rect::from_min_size(
            egui::pos2(bar_rect.left(), bar_rect.top() + bar_height),
            egui::vec2(bar_rect.width(), handle_zone),
        );
        let drag_resp = ui.interact(
            handle_area,
            egui::Id::new(("cr_drag", node_id.0)),
            egui::Sense::click_and_drag(),
        );
        if drag_resp.clicked() {
            if let Some(pos) = drag_resp.interact_pointer_pos() {
                let t = ((pos.x - bar_rect.left()) / bar_rect.width()).clamp(0.0, 1.0);
                if let Some((di, _)) = order.iter().enumerate().min_by(|(_, &a), (_, &b)| {
                    let da = (stops[a].0 - t).abs();
                    let db = (stops[b].0 - t).abs();
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                }) {
                    sel_order_idx = di;
                }
            }
        }
        if drag_resp.dragged() {
            if let Some(pos) = drag_resp.interact_pointer_pos() {
                let t = ((pos.x - bar_rect.left()) / bar_rect.width()).clamp(0.0, 1.0);
                let pi = order[sel_order_idx.min(order.len().saturating_sub(1))];
                stops[pi].0 = t;
                changed.push((format!("pos_{pi}"), ParamValue::Float(t)));
            }
        }

        ui.data_mut(|d| d.insert_temp(sel_id, sel_order_idx));

        ui.add_space(4.0);

        // --- Selected stop editor ---
        let sel_pi = order.get(sel_order_idx).copied().unwrap_or(0);

        ui.horizontal(|ui| {
            ui.label("Position");
            let mut pos_val = stops[sel_pi].0;
            if ui
                .add(crate::panels::widgets::ParamSlider::new(
                    &mut pos_val,
                    0.0,
                    1.0,
                ))
                .changed()
            {
                changed.push((format!("pos_{sel_pi}"), ParamValue::Float(pos_val)));
            }
        });

        ui.horizontal(|ui| {
            ui.label("Color");
            let [r, g, b] = stops[sel_pi].1;
            let mut c32 = egui::Color32::from_rgb(r, g, b);
            if ui.color_edit_button_srgba(&mut c32).changed() {
                let hex = format!("{:02X}{:02X}{:02X}", c32.r(), c32.g(), c32.b());
                changed.push((format!("color_{sel_pi}"), ParamValue::String(hex)));
            }
        });

        ui.add_space(4.0);

        // --- Add / Delete stop buttons ---
        ui.horizontal(|ui| {
            if ui
                .add_enabled(stop_count < 8, egui::Button::new("+ Stop"))
                .on_disabled_hover_text("Maximum 8 stops")
                .clicked()
            {
                // Add stop at 0.5, interpolated color.
                let new_idx = stop_count;
                let [r, g, b] = sample_gradient(&stops, &order, 0.5);
                changed.push((format!("pos_{new_idx}"), ParamValue::Float(0.5)));
                changed.push((
                    format!("color_{new_idx}"),
                    ParamValue::String(format!("{r:02X}{g:02X}{b:02X}")),
                ));
                changed.push((
                    "stop_count".to_string(),
                    ParamValue::UInt((stop_count + 1) as u32),
                ));
            }

            if ui
                .add_enabled(stop_count > 2, egui::Button::new("- Stop"))
                .on_disabled_hover_text("Minimum 2 stops")
                .clicked()
            {
                // Remove the selected param-index stop by swapping it with the
                // last active slot (swap-remove pattern), then decrement count.
                let last = stop_count - 1;
                if sel_pi != last {
                    // Copy last slot into sel_pi's slot.
                    let (last_pos, last_col) = stops[last];
                    changed.push((format!("pos_{sel_pi}"), ParamValue::Float(last_pos)));
                    changed.push((
                        format!("color_{sel_pi}"),
                        ParamValue::String(format!(
                            "{:02X}{:02X}{:02X}",
                            last_col[0], last_col[1], last_col[2]
                        )),
                    ));
                }
                changed.push(("stop_count".to_string(), ParamValue::UInt(last as u32)));
                sel_order_idx = sel_order_idx.saturating_sub(1);
                ui.data_mut(|d| d.insert_temp(sel_id, sel_order_idx));
            }
        });

        if !changed.is_empty() {
            self.push_undo("Change parameter");
            if let Some(node) = self.graph.get_node_mut(node_id) {
                for (key, value) in changed {
                    node.params.insert(key, value);
                }
                node.mark_dirty();
            }
        }
    }
}

/// Sample the gradient (stops sorted by `order`) at normalized position t.
/// Returns [r, g, b] as u8.
fn sample_gradient(stops: &[(f32, [u8; 3])], order: &[usize], t: f32) -> [u8; 3] {
    if order.is_empty() {
        return [128, 128, 128];
    }
    if order.len() == 1 {
        return stops[order[0]].1;
    }
    let sorted: Vec<(f32, [u8; 3])> = order.iter().map(|&i| stops[i]).collect();
    if t <= sorted[0].0 {
        return sorted[0].1;
    }
    if t >= sorted[sorted.len() - 1].0 {
        return sorted[sorted.len() - 1].1;
    }
    let hi = sorted
        .iter()
        .position(|s| s.0 >= t)
        .unwrap_or(sorted.len() - 1);
    let lo = hi.saturating_sub(1);
    let span = sorted[hi].0 - sorted[lo].0;
    let frac = if span > 1e-6 {
        (t - sorted[lo].0) / span
    } else {
        0.0
    };
    let a = sorted[lo].1;
    let b = sorted[hi].1;
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * frac) as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * frac) as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * frac) as u8,
    ]
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some([r, g, b])
}

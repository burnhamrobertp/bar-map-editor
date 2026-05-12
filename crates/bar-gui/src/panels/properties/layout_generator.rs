//! Contextual properties for the LayoutGenerator node.
//!
//! Shows a shape_count slider, then one collapsible section per shape with
//! type dropdown, position, size, angle, height, and falloff controls.

use bar_graph::{NodeId, ParamValue};
use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::widgets::ParamSlider;

impl BarEditorApp {
    pub(crate) fn draw_layout_generator_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        params: &std::collections::HashMap<String, ParamValue>,
    ) {
        let mut changed: Vec<(String, ParamValue)> = Vec::new();

        let mut shape_count = match params.get("shape_count") {
            Some(ParamValue::UInt(n)) => (*n).clamp(1, 8),
            _ => 1,
        };

        ui.label("Shapes");
        let mut sc_f = shape_count as f32;
        if ui
            .add(ParamSlider::new(&mut sc_f, 1.0, 8.0).integer())
            .changed()
        {
            shape_count = sc_f as u32;
            changed.push(("shape_count".to_string(), ParamValue::UInt(shape_count)));
        }

        ui.add_space(4.0);

        for i in 0..shape_count as usize {
            let label = {
                let ty = match params.get(&format!("type_{i}")) {
                    Some(ParamValue::String(s)) => s.as_str(),
                    _ => "ellipse",
                };
                format!("Shape {i} ({ty})")
            };

            let open_id = egui::Id::new(("lg_open", node_id.0, i as u64));
            let open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(i == 0);

            ui.horizontal(|ui| {
                let arrow = if open { "\u{25BC}" } else { "\u{25B6}" };
                if ui
                    .add(
                        egui::Label::new(egui::RichText::new(format!("{arrow} {label}")).strong())
                            .sense(egui::Sense::click()),
                    )
                    .clicked()
                {
                    ui.data_mut(|d| d.insert_temp::<bool>(open_id, !open));
                }
            });

            if open {
                egui::Grid::new(("lg_grid", node_id.0, i as u64))
                    .num_columns(2)
                    .spacing([8.0, 3.0])
                    .show(ui, |ui| {
                        // Type
                        let type_key = format!("type_{i}");
                        let cur_type = match params.get(&type_key) {
                            Some(ParamValue::String(s)) => s.clone(),
                            _ => "ellipse".to_string(),
                        };
                        ui.label("type");
                        egui::ComboBox::from_id_salt(("lg_type", node_id.0, i as u64))
                            .selected_text(&cur_type)
                            .show_ui(ui, |ui| {
                                for choice in ["ellipse", "rectangle", "ridge"] {
                                    if ui.selectable_label(cur_type == choice, choice).clicked() {
                                        changed.push((
                                            type_key.clone(),
                                            ParamValue::String(choice.to_string()),
                                        ));
                                    }
                                }
                            });
                        ui.end_row();

                        // Position X
                        let mut x = get_f(params, &format!("x_{i}"), 0.5);
                        ui.label("x");
                        if ui.add(ParamSlider::new(&mut x, 0.0, 1.0)).changed() {
                            changed.push((format!("x_{i}"), ParamValue::Float(x)));
                        }
                        ui.end_row();

                        // Position Y
                        let mut y = get_f(params, &format!("y_{i}"), 0.5);
                        ui.label("y");
                        if ui.add(ParamSlider::new(&mut y, 0.0, 1.0)).changed() {
                            changed.push((format!("y_{i}"), ParamValue::Float(y)));
                        }
                        ui.end_row();

                        // Radius X
                        let mut rx = get_f(params, &format!("rx_{i}"), 0.2);
                        ui.label("radius x");
                        if ui.add(ParamSlider::new(&mut rx, 0.01, 1.0)).changed() {
                            changed.push((format!("rx_{i}"), ParamValue::Float(rx)));
                        }
                        ui.end_row();

                        // Radius Y
                        let mut ry = get_f(params, &format!("ry_{i}"), 0.2);
                        ui.label("radius y");
                        if ui.add(ParamSlider::new(&mut ry, 0.01, 1.0)).changed() {
                            changed.push((format!("ry_{i}"), ParamValue::Float(ry)));
                        }
                        ui.end_row();

                        // Angle
                        let mut angle = get_f(params, &format!("angle_{i}"), 0.0);
                        ui.label("angle");
                        if ui.add(ParamSlider::new(&mut angle, 0.0, 360.0)).changed() {
                            changed.push((format!("angle_{i}"), ParamValue::Float(angle)));
                        }
                        ui.end_row();

                        // Height
                        let mut height = get_f(params, &format!("height_{i}"), 0.5);
                        ui.label("height");
                        if ui.add(ParamSlider::new(&mut height, 0.0, 1.0)).changed() {
                            changed.push((format!("height_{i}"), ParamValue::Float(height)));
                        }
                        ui.end_row();

                        // Falloff
                        let mut falloff = get_f(params, &format!("falloff_{i}"), 0.5);
                        ui.label("falloff");
                        if ui.add(ParamSlider::new(&mut falloff, 0.0, 1.0)).changed() {
                            changed.push((format!("falloff_{i}"), ParamValue::Float(falloff)));
                        }
                        ui.end_row();
                    });
            }

            ui.add_space(2.0);
        }

        if !changed.is_empty() {
            self.push_undo("Change parameter");
            if let Some(node) = self.graph.get_node_mut(node_id) {
                for (k, v) in changed {
                    node.params.insert(k, v);
                }
                node.mark_dirty();
            }
        }
    }
}

fn get_f(params: &std::collections::HashMap<String, ParamValue>, key: &str, default: f32) -> f32 {
    match params.get(key) {
        Some(ParamValue::Float(v)) => *v,
        _ => default,
    }
}

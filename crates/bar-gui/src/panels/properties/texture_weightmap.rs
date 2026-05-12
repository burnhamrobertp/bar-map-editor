//! Contextual properties for the TextureWeightmap node.
//!
//! Shows a `layer_count` spinner (2-8), `priority_type` combo, then one row
//! per slot with priority and (in priority mode) exclusion drag-values.

use bar_graph::{NodeId, ParamValue, PortId};
use eframe::egui;

use crate::app::{BarEditorApp, PORT_Y_BASE, PORT_Y_STEP};

impl BarEditorApp {
    pub(crate) fn draw_texture_weightmap_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        params: &std::collections::HashMap<String, ParamValue>,
    ) {
        let mut changed: Vec<(String, ParamValue)> = Vec::new();
        let mut new_layer_count: Option<u32> = None;

        // --- layer_count spinner ---
        let mut layer_count = match params.get("layer_count") {
            Some(ParamValue::UInt(v)) => *v,
            _ => 2,
        }
        .clamp(2, 8);

        ui.label("Layers");
        let mut lc_f = layer_count as f32;
        if ui
            .add(crate::panels::widgets::ParamSlider::new(&mut lc_f, 2.0, 8.0).integer())
            .changed()
        {
            layer_count = lc_f as u32;
            new_layer_count = Some(layer_count);
            changed.push(("layer_count".to_string(), ParamValue::UInt(layer_count)));
        }

        ui.add_space(4.0);

        // --- priority_type combo ---
        ui.label("Blend mode");
        let current_type = match params.get("priority_type") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => "weighted_blend".to_string(),
        };
        egui::ComboBox::from_id_salt(("twm_type", node_id.0))
            .selected_text(&current_type)
            .show_ui(ui, |ui| {
                for choice in ["weighted_blend", "priority"] {
                    if ui
                        .selectable_label(current_type == choice, choice)
                        .clicked()
                    {
                        changed.push((
                            "priority_type".to_string(),
                            ParamValue::String(choice.to_string()),
                        ));
                    }
                }
            });

        ui.add_space(6.0);
        ui.separator();

        // Which slots currently have a wired texture_i?
        let connected: Vec<bool> = (0..layer_count as usize)
            .map(|i| {
                let port_name = format!("texture_{i}");
                self.graph
                    .connections()
                    .iter()
                    .any(|c| c.to.node_id == node_id && c.to.port_name == port_name)
            })
            .collect();

        let in_priority_mode = current_type == "priority";

        egui::Grid::new(("twm_layers", node_id.0))
            .num_columns(if in_priority_mode { 3 } else { 2 })
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.strong("Slot");
                ui.strong("Priority");
                if in_priority_mode {
                    ui.strong("Exclusion");
                }
                ui.end_row();

                for (i, &is_connected) in connected.iter().enumerate() {
                    let priority_key = format!("priority_{i}");
                    let exclusion_key = format!("exclusion_{i}");

                    if is_connected {
                        ui.label(format!("{i}"));

                        let mut prio = match params.get(&priority_key) {
                            Some(ParamValue::Float(v)) => *v,
                            _ => (7 - i) as f32,
                        };
                        if ui
                            .add(crate::panels::widgets::ParamSlider::new(
                                &mut prio, 0.0, 16.0,
                            ))
                            .changed()
                        {
                            changed.push((priority_key, ParamValue::Float(prio)));
                        }

                        if in_priority_mode {
                            let mut excl = match params.get(&exclusion_key) {
                                Some(ParamValue::Float(v)) => *v,
                                _ => 0.0,
                            };
                            if ui
                                .add(crate::panels::widgets::ParamSlider::new(
                                    &mut excl, 0.0, 1.0,
                                ))
                                .changed()
                            {
                                changed.push((exclusion_key, ParamValue::Float(excl)));
                            }
                        }
                    } else {
                        ui.weak(format!("{i}"));
                        ui.weak("--");
                        if in_priority_mode {
                            ui.weak("--");
                        }
                    }
                    ui.end_row();
                }
            });

        if !changed.is_empty() {
            self.push_undo("Change parameter");

            // If layer_count shrank, disconnect any wires into removed ports before resizing.
            if let Some(new_count) = new_layer_count {
                let old_count = match params.get("layer_count") {
                    Some(ParamValue::UInt(v)) => *v,
                    _ => 2,
                };
                if new_count < old_count {
                    let to_remove: Vec<(PortId, PortId)> = self
                        .graph
                        .connections()
                        .iter()
                        .filter(|c| {
                            if c.to.node_id != node_id {
                                return false;
                            }
                            // texture_{i} where i >= new_count
                            if let Some(idx_str) = c.to.port_name.strip_prefix("texture_") {
                                if let Ok(idx) = idx_str.parse::<u32>() {
                                    return idx >= new_count;
                                }
                            }
                            false
                        })
                        .map(|c| (c.from.clone(), c.to.clone()))
                        .collect();

                    for (from, to) in to_remove {
                        self.graph.disconnect(&from, &to);
                    }
                }

                if let Some(node) = self.graph.get_node_mut(node_id) {
                    node.resize_texture_weightmap_ports(new_count);
                }
                // Snap the canvas node height to fit the new port count.
                if let Some(visual) = self.visuals.node_visuals.get_mut(&node_id) {
                    visual.size.y = (PORT_Y_BASE + new_count as f32 * PORT_Y_STEP + 10.0).max(60.0);
                }
            }

            if let Some(node) = self.graph.get_node_mut(node_id) {
                for (key, value) in changed {
                    node.params.insert(key, value);
                }
                node.mark_dirty();
            }
        }
    }
}

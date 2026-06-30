//! Contextual properties for the Switch node.
//!
//! Shows an `input_count` spinner (2-8) that resizes the input ports, then a
//! `selected` slider naming which input is forwarded.

use bar_graph::{NodeId, ParamValue, PortId};
use eframe::egui;

use crate::app::{BarEditorApp, PORT_Y_BASE, PORT_Y_STEP};

impl BarEditorApp {
    pub(crate) fn draw_switch_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        params: &std::collections::HashMap<String, ParamValue>,
    ) {
        let mut changed: Vec<(String, ParamValue)> = Vec::new();
        let mut new_input_count: Option<u32> = None;

        let mut input_count = match params.get("input_count") {
            Some(ParamValue::UInt(v)) => *v,
            _ => 2,
        }
        .clamp(2, 8);

        ui.label("Inputs");
        let mut ic_f = input_count as f32;
        let e = crate::panels::widgets::ParamSlider::new(&mut ic_f, 2.0, 8.0)
            .integer()
            .show(ui);
        if e.commit && ic_f as u32 != input_count {
            input_count = ic_f as u32;
            new_input_count = Some(input_count);
            changed.push(("input_count".to_string(), ParamValue::UInt(input_count)));
        }

        ui.add_space(4.0);

        // `selected` is clamped to the current input range.
        let mut selected = match params.get("selected") {
            Some(ParamValue::UInt(v)) => *v,
            _ => 0,
        }
        .min(input_count - 1);

        ui.label("Selected input");
        let mut sel_f = selected as f32;
        let e = crate::panels::widgets::ParamSlider::new(&mut sel_f, 0.0, (input_count - 1) as f32)
            .integer()
            .show(ui);
        if e.commit && sel_f as u32 != selected {
            selected = sel_f as u32;
            changed.push(("selected".to_string(), ParamValue::UInt(selected)));
        }

        if !changed.is_empty() {
            self.push_undo("Change parameter");

            // If input_count shrank, disconnect wires into removed ports first.
            if let Some(new_count) = new_input_count {
                let old_count = match params.get("input_count") {
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
                            if let Some(idx_str) = c.to.port_name.strip_prefix("input_") {
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
                    node.resize_switch_ports(new_count);
                }
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

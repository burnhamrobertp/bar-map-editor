//! Group properties body: label / colour / collapse, plus the
//! macro-parameter binding editor for SubGraph groups. Distributed
//! `impl BarEditorApp` block.

use bar_graph::{NodeId, ParamValue};
use eframe::egui;

use crate::app::*;

impl BarEditorApp {
    pub(crate) fn draw_group_properties(&mut self, ui: &mut egui::Ui, gid: u64) {
        // Snapshot the current state into locals so the UI body
        // doesn't have to thread mutable borrows.
        let snapshot = match self.visuals.groups.get(&gid) {
            Some(g) => g.clone(),
            None => {
                self.selection.group = None;
                return;
            }
        };
        let mut label_buf = snapshot.label.clone();
        let mut color_idx = snapshot.color_idx;
        let mut is_subgraph = snapshot.is_subgraph;
        let mut collapsed = snapshot.collapsed;
        // Read-only views of the runtime port lists. They're
        // recomputed every frame from the subgraph's `SubgraphInput`
        // / `SubgraphOutput` member nodes by
        // `recompute_all_subgraph_io`, so the properties panel only
        // displays them; editing happens by adding / removing IO
        // nodes inside the subgraph.
        let inputs = &snapshot.subgraph_inputs;
        let outputs = &snapshot.subgraph_outputs;

        // Header row: editable label fills the width, close ✕ in the
        // top-right corner.
        let mut dirty = false;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::panels::properties::close_icon_button(ui) {
                    self.props.close_requested = true;
                }
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut label_buf)
                        .hint_text("Group label")
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Heading),
                );
                crate::panels::widgets::select_all_on_focus(ui, &resp, &label_buf);
                if resp.changed() {
                    dirty = true;
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Type:").weak());
            ui.label(if is_subgraph {
                "SubGraph"
            } else {
                "Visual group"
            });
        });
        ui.weak(format!("{} member node(s)", snapshot.member_ids.len()));
        ui.separator();

        // Colour picker — radio buttons over the fixed palette.
        ui.label(egui::RichText::new("Colour").weak());
        ui.horizontal_wrapped(|ui| {
            for (i, _rgb) in GROUP_PALETTE.iter().enumerate() {
                let i = i as u8;
                let tint = group_color(i);
                let size = egui::vec2(22.0, 22.0);
                let (rect, swatch_resp) = ui.allocate_exact_size(size, egui::Sense::click());
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 4.0, tint);
                if i == color_idx {
                    painter.rect_stroke(
                        rect,
                        4.0,
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 220, 120)),
                        egui::StrokeKind::Inside,
                    );
                }
                if swatch_resp.clicked() {
                    color_idx = i;
                    dirty = true;
                }
            }
        });

        ui.separator();
        // SubGraph toggle: a visual group can be promoted to a reusable
        // subgraph with explicit inputs/outputs. Demoting back drops
        // the port definitions automatically the next frame -- the
        // runtime port lists are recomputed from member IO nodes by
        // `recompute_all_subgraph_io` regardless of this checkbox.
        if ui
            .checkbox(&mut is_subgraph, "Use as a SubGraph (reusable)")
            .changed()
        {
            dirty = true;
            if !is_subgraph {
                collapsed = false;
            }
        }
        if is_subgraph {
            if ui
                .checkbox(&mut collapsed, "Collapsed (single block)")
                .changed()
            {
                dirty = true;
            }
            ui.add_space(4.0);
            // High-level macro parameters: the abstracted-knob layer.
            // Each one writes through directly to the bound inner-node
            // param the moment the user changes the slider, so the
            // user gets the "drop a Mountain Range, twiddle 4 sliders"
            // workflow without ever expanding the SubGraph.
            let macro_params_snapshot: Vec<crate::state::MacroParamRuntime> =
                snapshot.macro_params.clone();
            if !macro_params_snapshot.is_empty() {
                ui.label(egui::RichText::new("Parameters").strong());
                self.draw_macro_params(ui, &macro_params_snapshot);
                ui.separator();
            }
            // Read-only port summary. Editing happens by adding /
            // renaming SubgraphInput / SubgraphOutput nodes inside
            // the subgraph; `recompute_all_subgraph_io` rebuilds
            // this list every frame from those nodes.
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Inputs").strong());
            if inputs.is_empty() {
                ui.weak(
                    "No external input ports yet. Open the subgraph and \
                     drop a SubgraphInput node from the palette to add one.",
                );
            } else {
                for p in inputs {
                    ui.horizontal(|ui| {
                        ui.weak("•");
                        ui.label(&p.name);
                        ui.weak(format!("({})", p.kind));
                    });
                }
            }
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Outputs").strong());
            if outputs.is_empty() {
                ui.weak(
                    "No external output ports yet. Open the subgraph and \
                     drop a SubgraphOutput node from the palette to add one.",
                );
            } else {
                for p in outputs {
                    ui.horizontal(|ui| {
                        ui.weak("•");
                        ui.label(&p.name);
                        ui.weak(format!("({})", p.kind));
                    });
                }
            }
        }

        ui.separator();
        let delete_label = if is_subgraph {
            "Delete subgraph"
        } else {
            "Delete group…"
        };
        if ui.button(delete_label).clicked() {
            if is_subgraph {
                self.delete_subgraph_with_contents(gid);
            } else {
                self.selection.pending_group_delete = Some(gid);
            }
        }

        if dirty {
            self.push_undo("Edit group properties");
            if let Some(g) = self.visuals.groups.get_mut(&gid) {
                g.label = label_buf;
                g.color_idx = color_idx;
                g.is_subgraph = is_subgraph;
                g.collapsed = collapsed;
                // subgraph_inputs / subgraph_outputs are derived from
                // member IO nodes by `recompute_all_subgraph_io`
                // every frame, so the panel never writes them
                // directly.
                self.project.is_dirty = true;
            }
        }
    }

    /// Render the macro-parameter widgets for a SubGraph. Each
    /// param's value is read live from the bound inner-node param,
    /// and edits are written back immediately. The SubGraph stores
    /// only the binding — the inner node owns the canonical value.
    pub(crate) fn draw_macro_params(
        &mut self,
        ui: &mut egui::Ui,
        params: &[crate::state::MacroParamRuntime],
    ) {
        let mut writes: Vec<(NodeId, String, ParamValue)> = Vec::new();
        egui::Grid::new("macro_params_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for p in params {
                    let Some((nid, param_name)) = p.binding.clone() else {
                        ui.label(&p.label);
                        ui.weak("(unbound)");
                        ui.end_row();
                        continue;
                    };
                    let cur = self
                        .graph
                        .get_node(nid)
                        .and_then(|n| n.params.get(&param_name).cloned());
                    ui.label(&p.label);
                    match (p.kind.as_str(), cur) {
                        ("Float", Some(ParamValue::Float(v))) => {
                            let mut val = v;
                            let committed = if let (Some(lo), Some(hi)) = (p.min, p.max) {
                                crate::panels::widgets::ParamSlider::new(
                                    &mut val, lo as f32, hi as f32,
                                )
                                .show(ui)
                                .commit
                            } else {
                                ui.add(egui::DragValue::new(&mut val).speed(0.01)).changed()
                            };
                            if committed {
                                writes.push((nid, param_name.clone(), ParamValue::Float(val)));
                            }
                        }
                        ("UInt", Some(ParamValue::UInt(v))) => {
                            let mut val = v as i64;
                            let committed = if let (Some(lo), Some(hi)) = (p.min, p.max) {
                                let mut vf = val as f32;
                                let e = crate::panels::widgets::ParamSlider::new(
                                    &mut vf, lo as f32, hi as f32,
                                )
                                .integer()
                                .show(ui);
                                val = vf as i64;
                                e.commit
                            } else {
                                ui.add(egui::DragValue::new(&mut val)).changed()
                            };
                            if committed {
                                writes.push((
                                    nid,
                                    param_name.clone(),
                                    ParamValue::UInt(val.max(0) as u32),
                                ));
                            }
                        }
                        ("Int", Some(ParamValue::Int(v))) => {
                            let mut val = v;
                            let mut drag = egui::DragValue::new(&mut val);
                            if let (Some(lo), Some(hi)) = (p.min, p.max) {
                                drag = drag.range((lo as i32)..=(hi as i32));
                            }
                            if ui.add(drag).changed() {
                                writes.push((nid, param_name.clone(), ParamValue::Int(val)));
                            }
                        }
                        ("Bool", Some(ParamValue::Bool(v))) => {
                            let mut val = v;
                            if ui.checkbox(&mut val, "").changed() {
                                writes.push((nid, param_name.clone(), ParamValue::Bool(val)));
                            }
                        }
                        ("String", Some(ParamValue::String(v))) => {
                            let mut val = v;
                            let bound_node_type =
                                self.graph.get_node(nid).map(|n| n.node_type.clone());
                            let mut new_val: Option<String> = None;
                            if let Some(nt) = &bound_node_type {
                                if let Some(choices) = bar_graph::param_choices(nt, &param_name) {
                                    let id = ("macro_param_choice", nid.0, param_name.as_str());
                                    egui::ComboBox::from_id_salt(id)
                                        .selected_text(&val)
                                        .show_ui(ui, |ui| {
                                            for c in choices {
                                                if ui.selectable_label(val == *c, *c).clicked() {
                                                    new_val = Some((*c).to_string());
                                                }
                                            }
                                        });
                                } else if bar_graph::param_is_color(nt, &param_name) {
                                    let rgb = parse_hex_color(&val).unwrap_or([128, 128, 128]);
                                    let mut c32 = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                                    if ui.color_edit_button_srgba(&mut c32).changed() {
                                        new_val = Some(format!(
                                            "{:02X}{:02X}{:02X}",
                                            c32.r(),
                                            c32.g(),
                                            c32.b()
                                        ));
                                    }
                                } else {
                                    let r = ui.add(
                                        egui::TextEdit::singleline(&mut val)
                                            .desired_width(f32::INFINITY),
                                    );
                                    crate::panels::widgets::select_all_on_focus(ui, &r, &val);
                                    if r.changed() {
                                        new_val = Some(val);
                                    }
                                }
                            } else {
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut val)
                                        .desired_width(f32::INFINITY),
                                );
                                crate::panels::widgets::select_all_on_focus(ui, &r, &val);
                                if r.changed() {
                                    new_val = Some(val);
                                }
                            }
                            if let Some(nv) = new_val {
                                let pv = ParamValue::String(nv.clone());
                                writes.push((nid, param_name.clone(), pv.clone()));
                                if let Some(nt) = &bound_node_type {
                                    for (k, v) in
                                        bar_graph::param_side_effects(nt, &param_name, &pv)
                                    {
                                        writes.push((nid, k, v));
                                    }
                                }
                            }
                        }
                        _ => {
                            ui.weak("(missing or kind mismatch)");
                        }
                    }
                    ui.end_row();
                }
            });
        if !writes.is_empty() {
            // One undo entry per macro-param change keeps the history
            // granular; if the user sweeps a slider the undo stack
            // ends up with one entry per discrete value, matching
            // every other param widget in the editor.
            self.push_undo("Edit macro parameter");
            for (nid, name, val) in writes {
                if let Some(node) = self.graph.get_node_mut(nid) {
                    node.params.insert(name, val);
                    node.mark_dirty();
                }
            }
            self.project.is_dirty = true;
        }
    }
}

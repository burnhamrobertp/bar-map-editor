//! `Sculpt` node properties body: brush settings, layer / target
//! selectors, the per-layer delta heatmap, and the layer-inference
//! helpers used to colour the heatmap by which sculpt layer a delta
//! belongs to. Distributed `impl BarEditorApp` block plus a few
//! free helpers private to this module.

use std::collections::HashMap;

use bar_graph::{GraphEngine, NodeId, NodeType, ParamValue};
use eframe::egui;

use crate::app::*;

impl BarEditorApp {
    pub(crate) fn draw_sculpt_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        node_params: &HashMap<String, ParamValue>,
    ) {
        const DISPLAY: f32 = 240.0;

        let layer = infer_sculpt_layer(node_id, &self.graph);

        // Read-only layer badge so the user can see what mode they are in.
        let layer_label = match layer {
            SculptLayer::Heightmap => "Layer: Heightmap",
            SculptLayer::Metalmap => "Layer: Metal Map",
            SculptLayer::Typemap => "Layer: Type Map",
        };
        ui.label(egui::RichText::new(layer_label).color(sculpt_layer_color(layer)));
        ui.add_space(2.0);

        let mut resolution = match node_params.get("resolution") {
            Some(ParamValue::UInt(n)) => (*n).max(1) as usize,
            _ => 256,
        };
        let data_str = match node_params.get("data") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => String::new(),
        };

        // Decode or initialise. An empty string means pure passthrough;
        // once the canvas has any data we lock the resolution.
        let has_painted = !data_str.is_empty();
        let mut pixels: Vec<u8> = if has_painted {
            let decoded = mask_hex_decode(&data_str);
            if decoded.len() == resolution * resolution {
                decoded
            } else {
                vec![128u8; resolution * resolution]
            }
        } else {
            vec![128u8; resolution * resolution]
        };

        // Resolution dropdown — locked once the canvas has been touched.
        let mut new_resolution = resolution;
        ui.horizontal(|ui| {
            ui.label("Resolution:");
            ui.add_enabled_ui(!has_painted, |ui| {
                egui::ComboBox::from_id_salt(("sculpt_res", node_id.0))
                    .selected_text(format!("{0}x{0}", resolution))
                    .show_ui(ui, |ui| {
                        for &choice in &[64usize, 128, 256, 512] {
                            ui.selectable_value(
                                &mut new_resolution,
                                choice,
                                format!("{0}x{0}", choice),
                            );
                        }
                    });
            });
            if has_painted {
                ui.label("(locked -- clear to change)");
            }
        });

        ui.horizontal(|ui| {
            ui.label("Brush size:");
            ui.add(egui::Slider::new(&mut self.paint.paint_brush_radius, 1.0..=32.0).integer());
        });

        // Strength slider only meaningful for heightmap (soft delta);
        // metalmap / typemap use binary stamps and hide it.
        if layer == SculptLayer::Heightmap {
            ui.horizontal(|ui| {
                ui.label("Strength:");
                ui.add(
                    egui::Slider::new(&mut self.paint.sculpt_brush_strength, 0.05..=1.0)
                        .step_by(0.05),
                );
            });
        }

        let hint = match layer {
            SculptLayer::Heightmap => {
                "Left drag: raise  *  Right drag: lower  *  Middle/Ctrl: reset"
            }
            SculptLayer::Metalmap => "Left drag: add metal  *  Right drag: remove metal",
            SculptLayer::Typemap => "Left drag: set type  *  Right drag: clear type",
        };
        ui.label(hint);
        ui.add_space(4.0);

        // Stamp values for each button. Heightmap uses strength-scaled
        // values; metalmap and typemap use binary extremes.
        let (raise_val, lower_val) = match layer {
            SculptLayer::Heightmap => {
                let s = self.paint.sculpt_brush_strength;
                (
                    (128.0 + s * 127.0).round() as u8,
                    (128.0 - s * 127.0).round() as u8,
                )
            }
            SculptLayer::Metalmap | SculptLayer::Typemap => (255u8, 0u8),
        };

        let canvas_size = egui::Vec2::splat(DISPLAY);
        let (canvas_rect, canvas_resp) =
            ui.allocate_exact_size(canvas_size, egui::Sense::click_and_drag());

        let ctx = ui.ctx().clone();
        let mut changed = false;

        let primary = canvas_resp.dragged_by(egui::PointerButton::Primary);
        let secondary = canvas_resp.dragged_by(egui::PointerButton::Secondary);
        let middle = canvas_resp.dragged_by(egui::PointerButton::Middle)
            || (primary && ctx.input(|i| i.modifiers.ctrl));

        if primary || secondary || middle {
            let val = if middle {
                128u8
            } else if secondary {
                lower_val
            } else {
                raise_val
            };
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                let rel = pos - canvas_rect.min;
                let px = (rel.x / DISPLAY * resolution as f32) as i32;
                let py = (rel.y / DISPLAY * resolution as f32) as i32;
                let br = self.paint.paint_brush_radius as i32;
                for dy in -br..=br {
                    for dx in -br..=br {
                        if dx * dx + dy * dy <= br * br {
                            let nx = px + dx;
                            let ny = py + dy;
                            if nx >= 0
                                && ny >= 0
                                && nx < resolution as i32
                                && ny < resolution as i32
                            {
                                pixels[ny as usize * resolution + nx as usize] = val;
                                changed = true;
                            }
                        }
                    }
                }
            }
        }

        // Build display texture with layer-appropriate colorization.
        let color_image = egui::ColorImage {
            size: [resolution, resolution],
            pixels: pixels
                .iter()
                .map(|&v| sculpt_delta_color(v, layer))
                .collect(),
        };
        let needs_reupload = changed
            || self
                .paint
                .mask_textures
                .get(&node_id)
                .map(|t| t.size() != [resolution, resolution])
                .unwrap_or(true);
        let tex_handle = self.paint.mask_textures.entry(node_id).or_insert_with(|| {
            ctx.load_texture(
                "sculpt_delta",
                color_image.clone(),
                egui::TextureOptions::NEAREST,
            )
        });
        if needs_reupload {
            tex_handle.set(color_image, egui::TextureOptions::NEAREST);
        }

        ui.painter().image(
            tex_handle.id(),
            canvas_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        ui.painter().rect_stroke(
            canvas_rect,
            0.0,
            egui::Stroke::new(1.0, egui::Color32::from_gray(100)),
            egui::StrokeKind::Outside,
        );

        ui.add_space(6.0);
        let mut cleared = false;
        if ui.button("Clear Canvas").clicked() {
            // Back to pure passthrough -- empty string, not all-128.
            changed = true;
            cleared = true;
        }

        let resolution_changed = new_resolution != resolution && !has_painted;
        if resolution_changed {
            resolution = new_resolution;
            pixels = vec![128u8; resolution * resolution];
            changed = true;
        }

        if changed {
            self.push_undo("Sculpt delta");
            if let Some(node) = self.graph.get_node_mut(node_id) {
                let new_data = if cleared {
                    String::new()
                } else {
                    mask_hex_encode(&pixels)
                };
                node.params
                    .insert("data".to_string(), ParamValue::String(new_data));
                if resolution_changed || cleared {
                    node.params.insert(
                        "resolution".to_string(),
                        ParamValue::UInt(resolution as u32),
                    );
                }
                node.mark_dirty();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Sculpt helpers
// ---------------------------------------------------------------------------

/// Which Bundler layer a Sculpt node ultimately feeds. Inferred by
/// walking the downstream connection chain; never stored on the node.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SculptLayer {
    Heightmap,
    Metalmap,
    Typemap,
}

/// Walk the graph BFS-style from `node_id`'s "output" port and return
/// the first Bundler port name found downstream. Falls back to Heightmap.
fn infer_sculpt_layer(node_id: NodeId, graph: &GraphEngine) -> SculptLayer {
    let mut queue = vec![node_id];
    let mut visited = std::collections::HashSet::new();
    while let Some(current) = queue.pop() {
        if !visited.insert(current) {
            continue;
        }
        for conn in graph.connections() {
            if conn.from.node_id != current {
                continue;
            }
            if let Some(target) = graph.get_node(conn.to.node_id) {
                if target.node_type == NodeType::Bundler {
                    return match conn.to.port_name.as_str() {
                        "metalmap" => SculptLayer::Metalmap,
                        "typemap" => SculptLayer::Typemap,
                        _ => SculptLayer::Heightmap,
                    };
                }
                queue.push(conn.to.node_id);
            }
        }
    }
    SculptLayer::Heightmap
}

/// Accent color used for the layer badge.
fn sculpt_layer_color(layer: SculptLayer) -> egui::Color32 {
    match layer {
        SculptLayer::Heightmap => egui::Color32::from_rgb(160, 200, 160),
        SculptLayer::Metalmap => egui::Color32::from_rgb(220, 185, 80),
        SculptLayer::Typemap => egui::Color32::from_rgb(100, 180, 220),
    }
}

/// Per-pixel canvas color for the sculpt delta display.
///
/// Heightmap: standard gray-scale gradient centered at mid-gray (128=neutral).
/// Metalmap:  neutral=dark, add=warm gold, remove=deep shadow.
/// Typemap:   neutral=dark, add=cool teal, remove=deep shadow.
fn sculpt_delta_color(val: u8, layer: SculptLayer) -> egui::Color32 {
    match layer {
        SculptLayer::Heightmap => egui::Color32::from_gray(val),
        SculptLayer::Metalmap => {
            if val >= 128 {
                let t = (val - 128) as f32 / 127.0;
                egui::Color32::from_rgb(
                    (50.0 + t * 205.0) as u8,
                    (40.0 + t * 155.0) as u8,
                    (20.0 + t * 20.0) as u8,
                )
            } else {
                let t = val as f32 / 128.0;
                egui::Color32::from_gray((t * 50.0) as u8)
            }
        }
        SculptLayer::Typemap => {
            if val >= 128 {
                let t = (val - 128) as f32 / 127.0;
                egui::Color32::from_rgb(
                    (20.0 + t * 20.0) as u8,
                    (60.0 + t * 160.0) as u8,
                    (80.0 + t * 175.0) as u8,
                )
            } else {
                let t = val as f32 / 128.0;
                egui::Color32::from_gray((t * 50.0) as u8)
            }
        }
    }
}

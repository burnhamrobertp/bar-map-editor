//! `PaintedHeightmap` node properties body: brush radius / strength
//! / falloff / target, plus the "lock to this node" affordance.
//! Distributed `impl BarEditorApp` block.

use std::collections::HashMap;

use bar_graph::{NodeId, ParamValue};
use eframe::egui;

use crate::app::*;

impl BarEditorApp {
    pub(crate) fn draw_painted_heightmap_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        node_params: &HashMap<String, ParamValue>,
    ) {
        const DISPLAY: f32 = 240.0;

        // Hand-painted heightmaps are always square; the imported path
        // can be rectangular but isn't user-editable here. Prefer the
        // legacy `resolution` param for the brush canvas and fall back
        // to min(width, height) for imported nodes.
        let mut resolution = match node_params.get("resolution") {
            Some(ParamValue::UInt(n)) => (*n).max(1) as usize,
            _ => {
                let w = match node_params.get("width") {
                    Some(ParamValue::UInt(n)) => (*n).max(1) as usize,
                    _ => 256,
                };
                let h = match node_params.get("height") {
                    Some(ParamValue::UInt(n)) => (*n).max(1) as usize,
                    _ => w,
                };
                w.min(h)
            }
        };
        let data_str = match node_params.get("data") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => String::new(),
        };
        let mut pixels = mask_hex_decode(&data_str);
        let has_painted = pixels.len() == resolution * resolution && pixels.iter().any(|p| *p != 0);
        if pixels.len() != resolution * resolution {
            pixels = vec![0u8; resolution * resolution];
        }

        // Resolution dropdown — disabled once the user has painted.
        let mut new_resolution = resolution;
        ui.horizontal(|ui| {
            ui.label("Resolution:");
            ui.add_enabled_ui(!has_painted, |ui| {
                egui::ComboBox::from_id_salt(("painted_hm_res", node_id.0))
                    .selected_text(format!("{0}×{0}", resolution))
                    .show_ui(ui, |ui| {
                        for &choice in &[64usize, 128, 256, 512] {
                            ui.selectable_value(
                                &mut new_resolution,
                                choice,
                                format!("{0}×{0}", choice),
                            );
                        }
                    });
            });
            if has_painted {
                ui.label("(locked — clear to change)");
            }
        });

        ui.horizontal(|ui| {
            ui.label("Brush size:");
            ui.add(egui::Slider::new(&mut self.paint.paint_brush_radius, 1.0..=32.0).integer());
        });
        ui.label("Left drag: raise  ·  Right drag: erase");
        ui.add_space(4.0);

        let canvas_size = egui::Vec2::splat(DISPLAY);
        let (canvas_rect, canvas_resp) =
            ui.allocate_exact_size(canvas_size, egui::Sense::click_and_drag());

        let ctx = ui.ctx().clone();
        let mut changed = false;
        if canvas_resp.dragged_by(egui::PointerButton::Primary)
            || canvas_resp.dragged_by(egui::PointerButton::Secondary)
        {
            let erase = canvas_resp.dragged_by(egui::PointerButton::Secondary);
            let val = if erase { 0u8 } else { 255u8 };
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

        let color_image = egui::ColorImage {
            size: [resolution, resolution],
            pixels: pixels
                .iter()
                .map(|&g| egui::Color32::from_gray(g))
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
                "painted_heightmap",
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
            pixels = vec![0u8; resolution * resolution];
            changed = true;
            cleared = true;
        }

        let resolution_changed = new_resolution != resolution && !has_painted;
        if resolution_changed {
            resolution = new_resolution;
            pixels = vec![0u8; resolution * resolution];
            changed = true;
        }

        if changed {
            let new_data = mask_hex_encode(&pixels);
            self.push_undo("Paint heightmap");
            if let Some(node) = self.graph.get_node_mut(node_id) {
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

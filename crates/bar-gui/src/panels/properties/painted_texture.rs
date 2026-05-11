//! `PaintedTexture` node properties body: brush radius / strength /
//! colour / falloff. Distributed `impl BarEditorApp` block.

use std::collections::HashMap;

use bar_graph::{NodeId, ParamValue};
use eframe::egui;

use crate::app::*;

impl BarEditorApp {
    pub(crate) fn draw_painted_texture_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        node_params: &HashMap<String, ParamValue>,
    ) {
        const PAINT_RES: usize = 256;
        const DISPLAY: f32 = 240.0;

        let data_str = match node_params.get("data") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => String::new(),
        };
        let mut pixels = mask_hex_decode(&data_str);
        if pixels.len() != PAINT_RES * PAINT_RES * 3 {
            pixels = vec![0u8; PAINT_RES * PAINT_RES * 3];
        }

        // Brush colour — packed 0xRRGGBB stored as a hex string in
        // params so it persists across edits.
        let color_hex = match node_params.get("brush_color") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => "8B7355".to_string(),
        };
        let mut rgb = parse_hex_color(&color_hex).unwrap_or([0x8B, 0x73, 0x55]);
        let mut color_changed = false;
        ui.horizontal(|ui| {
            ui.label("Brush colour:");
            let mut c32 = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            if ui.color_edit_button_srgba(&mut c32).changed() {
                rgb = [c32.r(), c32.g(), c32.b()];
                color_changed = true;
            }
        });

        ui.horizontal(|ui| {
            ui.label("Brush size:");
            ui.add(egui::Slider::new(&mut self.paint.paint_brush_radius, 1.0..=32.0).integer());
        });
        ui.label("Left drag: paint  ·  Right drag: erase");
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
            let stamp = if erase { [0u8, 0, 0] } else { rgb };
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                let rel = pos - canvas_rect.min;
                let px = (rel.x / DISPLAY * PAINT_RES as f32) as i32;
                let py = (rel.y / DISPLAY * PAINT_RES as f32) as i32;
                let br = self.paint.paint_brush_radius as i32;
                for dy in -br..=br {
                    for dx in -br..=br {
                        if dx * dx + dy * dy <= br * br {
                            let nx = px + dx;
                            let ny = py + dy;
                            if nx >= 0 && ny >= 0 && nx < PAINT_RES as i32 && ny < PAINT_RES as i32
                            {
                                let idx = (ny as usize * PAINT_RES + nx as usize) * 3;
                                pixels[idx] = stamp[0];
                                pixels[idx + 1] = stamp[1];
                                pixels[idx + 2] = stamp[2];
                                changed = true;
                            }
                        }
                    }
                }
            }
        }

        let color_image = egui::ColorImage {
            size: [PAINT_RES, PAINT_RES],
            pixels: (0..PAINT_RES * PAINT_RES)
                .map(|i| {
                    let o = i * 3;
                    egui::Color32::from_rgb(pixels[o], pixels[o + 1], pixels[o + 2])
                })
                .collect(),
        };
        let needs_reupload = changed
            || self
                .paint
                .mask_textures
                .get(&node_id)
                .map(|t| t.size() != [PAINT_RES, PAINT_RES])
                .unwrap_or(true);
        let tex_handle = self.paint.mask_textures.entry(node_id).or_insert_with(|| {
            ctx.load_texture(
                "painted_texture",
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
        if ui.button("Clear Canvas").clicked() {
            pixels = vec![0u8; PAINT_RES * PAINT_RES * 3];
            changed = true;
        }

        if changed || color_changed {
            self.push_undo("Paint texture");
            if let Some(node) = self.graph.get_node_mut(node_id) {
                if changed {
                    let new_data = mask_hex_encode(&pixels);
                    node.params
                        .insert("data".to_string(), ParamValue::String(new_data));
                }
                if color_changed {
                    let hex = format!("{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
                    node.params
                        .insert("brush_color".to_string(), ParamValue::String(hex));
                }
                node.mark_dirty();
            }
        }
    }
}

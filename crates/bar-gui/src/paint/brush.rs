//! Brush dab application onto sculpt-layer nodes.
//!
//! These methods live on `BarEditorApp` because each dab touches both the
//! `paint` live cache and the graph. The pure math lives in
//! `super::brush_math`; this file wires it into the editor's per-stroke flow.

use bar_graph::{NodeId, ParamValue};

use crate::app::{mask_hex_encode, BarEditorApp};
use crate::paint::brush_math::{apply_brush_dab, stamp_color_dab_in_buffer};
use crate::paint::{BrushTool, LivePaintBuffer};

/// Native resolution of a PaintedTexture canvas (matches executor.rs constant).
const PAINTED_TEXTURE_RES: u32 = 256;

impl BarEditorApp {
    /// Apply the height brush to `node_id` (a `PaintedHeightmap` or `Sculpt`
    /// node) at heightmap-pixel coordinates.
    ///
    /// Initialises the node's live buffer on the first call of a stroke.
    /// Mutates `paint.heightmap` in parallel for instant 3D viewport feedback.
    /// Returns true iff any data changed.
    pub fn apply_brush_to_sculpt_layer(
        &mut self,
        node_id: NodeId,
        hx: f32,
        hy: f32,
        stroke_starting: bool,
    ) -> bool {
        let (map_w, map_h) = match self.paint.heightmap.as_ref() {
            Some(hm) => (hm.width() as f32, hm.height() as f32),
            None => return false,
        };

        // Capture flatten target at stroke start.
        if stroke_starting && self.paint.brush.tool == BrushTool::Flatten {
            if let Some(hm) = self.paint.heightmap.as_ref() {
                let ix = (hx.round() as i32).clamp(0, hm.width() as i32 - 1) as u32;
                let iy = (hy.round() as i32).clamp(0, hm.height() as i32 - 1) as u32;
                self.paint.brush.flatten_target = hm.get(ix, iy);
            }
        }

        // Determine node resolution.
        let node_res = match self.graph.get_node(node_id) {
            Some(n) => match n.params.get("resolution") {
                Some(ParamValue::UInt(r)) => *r,
                _ => 256,
            },
            None => return false,
        };

        // Initialise live buffer from the node's current data param.
        if !self.paint.live_paint.contains_key(&node_id) {
            let pixels = match self.graph.get_node(node_id) {
                Some(n) => match n.params.get("data") {
                    Some(ParamValue::String(s)) => hex_decode_bytes(s),
                    _ => vec![],
                },
                None => return false,
            };
            let expected = (node_res * node_res) as usize;
            let mut f32_data = vec![0.0f32; expected];
            for (i, &b) in pixels.iter().take(expected).enumerate() {
                f32_data[i] = b as f32 / 255.0;
            }
            let live_hm = bar_data::Heightmap::frbar_data(node_res, node_res, f32_data)
                .unwrap_or_else(|_| bar_data::Heightmap::new(node_res, node_res).unwrap());
            self.paint
                .live_paint
                .insert(node_id, LivePaintBuffer::Height(live_hm));
        }

        // Scale coordinates and radius from map resolution to node resolution.
        let scale_x = node_res as f32 / map_w;
        let scale_y = node_res as f32 / map_h;
        let mut scaled_brush = self.paint.brush.clone();
        scaled_brush.radius_px *= scale_x;
        let scaled_hx = hx * scale_x;
        let scaled_hy = hy * scale_y;

        if let Some(LivePaintBuffer::Height(live_hm)) = self.paint.live_paint.get_mut(&node_id) {
            apply_brush_dab(live_hm, scaled_hx, scaled_hy, &scaled_brush);
        }

        // Mutate the inspector heightmap for instant 3D viewport feedback.
        if let Some(hm) = self.paint.heightmap.as_mut() {
            apply_brush_dab(hm, hx, hy, &self.paint.brush);
            self.paint.heightmap_rev = self.paint.heightmap_rev.wrapping_add(1);
        }

        self.paint.brush_stroking = true;
        self.project.is_dirty = true;
        true
    }

    /// Apply the color brush to `node_id` (a `PaintedTexture` node) at
    /// heightmap-pixel coordinates.
    ///
    /// Mutates `paint.color_buffer` for instant viewport feedback.
    pub fn apply_color_brush_to_sculpt_layer(&mut self, node_id: NodeId, hx: f32, hy: f32) -> bool {
        let (hm_w, hm_h) = match self.paint.heightmap.as_ref() {
            Some(hm) => (hm.width(), hm.height()),
            None => return false,
        };
        let map_dim = (hm_w.max(hm_h) as f32).max(1.0);
        let u = (hx / hm_w as f32).clamp(0.0, 1.0);
        let v = (hy / hm_h as f32).clamp(0.0, 1.0);
        let ru = (self.paint.brush.radius_px / map_dim).max(0.001);
        let [r, g, b] = self.paint.brush.color_rgb;

        // Initialise live color buffer from node's current data param.
        if !self.paint.live_paint.contains_key(&node_id) {
            let pixels = match self.graph.get_node(node_id) {
                Some(n) => match n.params.get("data") {
                    Some(ParamValue::String(s)) => hex_decode_bytes(s),
                    _ => vec![],
                },
                None => return false,
            };
            let res = PAINTED_TEXTURE_RES as usize;
            let expected = res * res * 3;
            let mut cb =
                bar_data::ColorBuffer::new(PAINTED_TEXTURE_RES, PAINTED_TEXTURE_RES).unwrap();
            if pixels.len() == expected {
                for py in 0..res {
                    for px in 0..res {
                        let idx = (py * res + px) * 3;
                        cb.set(
                            px as u32,
                            py as u32,
                            [
                                pixels[idx] as f32 / 255.0,
                                pixels[idx + 1] as f32 / 255.0,
                                pixels[idx + 2] as f32 / 255.0,
                                1.0,
                            ],
                        );
                    }
                }
            }
            self.paint
                .live_paint
                .insert(node_id, LivePaintBuffer::Color(cb));
        }

        // Apply dab to the live color buffer at native texture resolution.
        if let Some(LivePaintBuffer::Color(live_cb)) = self.paint.live_paint.get_mut(&node_id) {
            stamp_color_dab_in_buffer(live_cb, u, v, ru, [r, g, b]);
        }

        // Mirror into the live color_buffer cache for instant viewport feedback.
        if let Some(ref mut cb) = self.paint.color_buffer {
            stamp_color_dab_in_buffer(cb, u, v, ru, [r, g, b]);
        }

        self.paint.brush_stroking = true;
        self.project.is_dirty = true;
        true
    }

    /// Encode the live paint buffer for `node_id` and write it to the node's
    /// `data` param. Pushes an undo snapshot and marks the node dirty so the
    /// preview re-evaluates. Removes the buffer from `live_paint`.
    pub fn flush_live_paint(&mut self, node_id: NodeId) {
        let Some(buffer) = self.paint.live_paint.remove(&node_id) else {
            return;
        };
        let data_hex = match buffer {
            LivePaintBuffer::Height(hm) => {
                let res = hm.width();
                let hm_ref = &hm;
                let pixels: Vec<u8> = (0..res)
                    .flat_map(|y| {
                        (0..res).map(move |x| {
                            let v = hm_ref.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);
                            (v * 255.0).round() as u8
                        })
                    })
                    .collect();
                mask_hex_encode(&pixels)
            }
            LivePaintBuffer::Color(cb) => {
                let res = cb.width();
                let cb_ref = &cb;
                let pixels: Vec<u8> = (0..res)
                    .flat_map(|y| {
                        (0..res).flat_map(move |x| {
                            let rgba = cb_ref.get(x, y).unwrap_or([0.0; 4]);
                            [
                                (rgba[0] * 255.0).round() as u8,
                                (rgba[1] * 255.0).round() as u8,
                                (rgba[2] * 255.0).round() as u8,
                            ]
                        })
                    })
                    .collect();
                mask_hex_encode(&pixels)
            }
        };
        self.push_undo("Paint layer");
        if let Some(node) = self.graph.get_node_mut(node_id) {
            node.params
                .insert("data".to_string(), ParamValue::String(data_hex));
            node.mark_dirty();
        }
    }

    /// End a 3D viewport sculpt stroke: flush the live buffer to the graph,
    /// clear the stroke flag, and release the Flatten target.
    pub fn end_brush_stroke_on_layer(&mut self, node_id: NodeId) {
        self.flush_live_paint(node_id);
        self.paint.brush_stroking = false;
        self.paint.brush.flatten_target = None;
    }

    /// End a preview-only stroke (2D inspector). Clears stroke flags without
    /// writing back to node params.
    pub fn end_brush_stroke(&mut self) {
        self.paint.brush_stroking = false;
        self.paint.brush.flatten_target = None;
    }
}

/// Decode a hex string produced by `mask_hex_encode` back to raw bytes.
fn hex_decode_bytes(s: &str) -> Vec<u8> {
    let s = s.as_bytes();
    let n = s.len() / 2;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let hi = nibble(s[i * 2]);
        let lo = nibble(s[i * 2 + 1]);
        out.push((hi << 4) | lo);
    }
    out
}

fn nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

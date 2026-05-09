//! Brush dab application that touches multiple sub-states.
//!
//! These methods live on `BarEditorApp` because each dab touches
//! `paint` (live cache + sculpt overlay), `project` (mark dirty), and
//! sometimes the brush state itself (Flatten target). The pure dab-
//! application math is in `super::brush_math`; this file wires it
//! into the editor's per-stroke flow.

use crate::app::BarEditorApp;
use crate::paint::brush_math::{
    apply_brush_dab, stamp_color_dab_in_buffer, stamp_value_dab_in_heightmap,
};
use crate::paint::BrushTool;

impl BarEditorApp {
    /// Apply the current brush at heightmap pixel coordinates. Call
    /// this once per dab. Setting `stroke_starting = true` captures
    /// the Flatten target at stroke start. Returns true iff the
    /// heightmap actually changed.
    ///
    /// Two effects per dab:
    /// 1. `sculpt.height_delta` is updated for persistent save/export.
    /// 2. The inspector heightmap is mutated in-place for instant feedback.
    pub fn apply_brush_at_heightmap(&mut self, hx: f32, hy: f32, stroke_starting: bool) -> bool {
        let (hm_w, hm_h) = match self.paint.heightmap.as_ref() {
            Some(hm) => (hm.width() as f32, hm.height() as f32),
            None => return false,
        };
        let dim_w = hm_w as u32;
        let dim_h = hm_h as u32;
        if stroke_starting && self.paint.brush.tool == BrushTool::Flatten {
            let hm = self.paint.heightmap.as_ref().unwrap();
            let ix = (hx.round() as i32).clamp(0, hm.width() as i32 - 1) as u32;
            let iy = (hy.round() as i32).clamp(0, hm.height() as i32 - 1) as u32;
            self.paint.brush.flatten_target = hm.get(ix, iy);
        }
        // Write to the persistent sculpt height delta.
        if self.paint.sculpt.height_delta.is_none() {
            self.paint.sculpt.height_delta = bar_data::Heightmap::new(dim_w, dim_h).ok();
        }
        if let Some(ref mut delta) = self.paint.sculpt.height_delta {
            apply_brush_dab(delta, hx, hy, &self.paint.brush);
        }
        // Mutate the inspector heightmap for instant visual feedback.
        if let Some(hm) = self.paint.heightmap.as_mut() {
            apply_brush_dab(hm, hx, hy, &self.paint.brush);
            self.paint.heightmap_rev = self.paint.heightmap_rev.wrapping_add(1);
        }
        self.paint.sculpt.dirty = true;
        self.paint.brush_stroking = true;
        self.project.is_dirty = true;
        true
    }

    /// Mark the end of a 3D-viewport sculpt stroke. Pairs with
    /// `apply_brush_at_heightmap`. Releases the per-stroke Flatten
    /// target so the next stroke captures a fresh one.
    pub fn end_brush_stroke(&mut self) {
        self.paint.brush_stroking = false;
        self.paint.brush.flatten_target = None;
    }

    /// Paint one colour brush dab at heightmap-pixel coordinates.
    /// Routes to a `TextureSculpt` overlay node inserted between the
    /// existing `Bundler.texture` source and the Bundler. The dab is
    /// recorded as a normalised-space entry in the node's `dabs`
    /// param; on next eval the executor reads the upstream Color,
    /// replays every recorded dab on top, and outputs the composite.
    /// Upstream texture pipelines (AutoTexture, imported, painted)
    /// flow through unchanged — the brush is purely additive overlay
    /// in the same shape as the heightmap `Sculpt` node.
    ///
    /// Returns true iff a dab was recorded. False when no upstream
    /// texture exists yet (the user needs to wire one in first).
    pub fn apply_color_brush_at_heightmap(&mut self, hx: f32, hy: f32) -> bool {
        let (hm_w, hm_h) = match self.paint.heightmap.as_ref() {
            Some(hm) => (hm.width(), hm.height()),
            None => return false,
        };
        let map_dim = (hm_w.max(hm_h) as f32).max(1.0);
        let u = (hx / hm_w as f32).clamp(0.0, 1.0);
        let v = (hy / hm_h as f32).clamp(0.0, 1.0);
        let ru = (self.paint.brush.radius_px / map_dim).max(0.001);
        let [r, g, b] = self.paint.brush.color_rgb;
        // Write to the persistent sculpt texture overlay.
        if self.paint.sculpt.texture_overlay.is_none() {
            self.paint.sculpt.texture_overlay = bar_data::ColorBuffer::new(hm_w, hm_h).ok();
        }
        if let Some(ref mut cb) = self.paint.sculpt.texture_overlay {
            stamp_color_dab_in_buffer(cb, u, v, ru, [r, g, b]);
        }
        // Mirror into the live cache for instant viewport feedback.
        if let Some(ref mut cb) = self.paint.color_buffer {
            stamp_color_dab_in_buffer(cb, u, v, ru, [r, g, b]);
        }
        self.paint.sculpt.dirty = true;
        self.project.is_dirty = true;
        true
    }

    /// Paint one metalmap dab into the sculpt metal overlay.
    pub fn apply_metal_brush_at_heightmap(&mut self, hx: f32, hy: f32) -> bool {
        let (hm_w, hm_h) = match self.paint.heightmap.as_ref() {
            Some(hm) => (hm.width(), hm.height()),
            None => return false,
        };
        let map_dim = (hm_w.max(hm_h) as f32).max(1.0);
        let u = (hx / hm_w as f32).clamp(0.0, 1.0);
        let v = (hy / hm_h as f32).clamp(0.0, 1.0);
        let ru = (self.paint.brush.radius_px / map_dim).max(0.001);
        let value = self.paint.brush.paint_value.clamp(0.0, 1.0);
        if self.paint.sculpt.metal_overlay.is_none() {
            self.paint.sculpt.metal_overlay = bar_data::Heightmap::new(hm_w, hm_h).ok();
        }
        if self.paint.sculpt.metal_alpha.is_none() {
            self.paint.sculpt.metal_alpha = bar_data::Heightmap::new(hm_w, hm_h).ok();
        }
        if let Some(ref mut hm) = self.paint.sculpt.metal_overlay {
            stamp_value_dab_in_heightmap(hm, u, v, ru, value);
        }
        if let Some(ref mut hm) = self.paint.sculpt.metal_alpha {
            stamp_value_dab_in_heightmap(hm, u, v, ru, 1.0);
        }
        // Mirror into the live metalmap cache for instant feedback.
        if self.paint.metalmap.is_none() {
            self.paint.metalmap = bar_data::Heightmap::new(hm_w, hm_h).ok();
        }
        if let Some(ref mut hm) = self.paint.metalmap {
            stamp_value_dab_in_heightmap(hm, u, v, ru, value);
        }
        self.paint.sculpt.dirty = true;
        self.project.is_dirty = true;
        true
    }

    /// Paint one typemap dab into the sculpt type overlay.
    pub fn apply_type_brush_at_heightmap(&mut self, hx: f32, hy: f32) -> bool {
        let (hm_w, hm_h) = match self.paint.heightmap.as_ref() {
            Some(hm) => (hm.width(), hm.height()),
            None => return false,
        };
        let map_dim = (hm_w.max(hm_h) as f32).max(1.0);
        let u = (hx / hm_w as f32).clamp(0.0, 1.0);
        let v = (hy / hm_h as f32).clamp(0.0, 1.0);
        let ru = (self.paint.brush.radius_px / map_dim).max(0.001);
        let value = self.paint.brush.paint_value.clamp(0.0, 1.0);
        if self.paint.sculpt.type_overlay.is_none() {
            self.paint.sculpt.type_overlay = bar_data::Heightmap::new(hm_w, hm_h).ok();
        }
        if self.paint.sculpt.type_alpha.is_none() {
            self.paint.sculpt.type_alpha = bar_data::Heightmap::new(hm_w, hm_h).ok();
        }
        if let Some(ref mut hm) = self.paint.sculpt.type_overlay {
            stamp_value_dab_in_heightmap(hm, u, v, ru, value);
        }
        if let Some(ref mut hm) = self.paint.sculpt.type_alpha {
            stamp_value_dab_in_heightmap(hm, u, v, ru, 1.0);
        }
        // Mirror into the live typemap cache for instant feedback.
        if self.paint.typemap.is_none() {
            self.paint.typemap = bar_data::Heightmap::new(hm_w, hm_h).ok();
        }
        if let Some(ref mut hm) = self.paint.typemap {
            stamp_value_dab_in_heightmap(hm, u, v, ru, value);
        }
        self.paint.sculpt.dirty = true;
        self.project.is_dirty = true;
        true
    }
}

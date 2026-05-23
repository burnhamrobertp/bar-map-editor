//! Brush dab application onto sculpt-layer nodes.
//!
//! These methods live on `BarEditorApp` because each dab touches both the
//! `paint` live cache and the graph. The pure math lives in
//! `super::brush_math`; this file wires it into the editor's per-stroke flow.

use bar_graph::{NodeId, ParamValue};

use crate::app::BarEditorApp;
use crate::paint::brush_math::{apply_brush_dab, stamp_color_dab_in_buffer, stamp_value_dab};
use crate::paint::{BrushTool, FCLayerKind, LivePaintBuffer, PaintKey};

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

        // Initialise live buffer from the node's binary asset (the executor
        // reads the same asset, so what we paint into the buffer must match
        // the current on-disk state -- otherwise the first stroke replaces
        // the existing heightmap with zeros plus the dab).
        if !self.paint.live_paint.contains_key(&PaintKey::Node(node_id)) {
            let asset_path = self
                .graph
                .get_node(node_id)
                .and_then(|n| n.params.get("asset_path"))
                .and_then(|v| match v {
                    ParamValue::String(s) if !s.is_empty() => Some(s.clone()),
                    _ => None,
                });
            let live_hm = asset_path
                .as_deref()
                .and_then(read_height_asset)
                .unwrap_or_else(|| bar_data::Heightmap::new(node_res, node_res).unwrap());
            self.paint
                .live_paint
                .insert(PaintKey::Node(node_id), LivePaintBuffer::Height(live_hm));
        }

        // Scale coordinates and radius from map resolution to node resolution.
        let scale_x = node_res as f32 / map_w;
        let scale_y = node_res as f32 / map_h;
        let mut scaled_brush = self.paint.brush.clone();
        scaled_brush.radius_px *= scale_x;
        let scaled_hx = hx * scale_x;
        let scaled_hy = hy * scale_y;

        if let Some(LivePaintBuffer::Height(live_hm)) =
            self.paint.live_paint.get_mut(&PaintKey::Node(node_id))
        {
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

        // Initialise live color buffer from the node's binary asset (the
        // executor reads the same asset, so what we paint into the buffer
        // must match the current on-disk state).
        if !self.paint.live_paint.contains_key(&PaintKey::Node(node_id)) {
            let asset_path = self
                .graph
                .get_node(node_id)
                .and_then(|n| n.params.get("asset_path"))
                .and_then(|v| match v {
                    ParamValue::String(s) if !s.is_empty() => Some(s.clone()),
                    _ => None,
                });
            let cb = asset_path
                .as_deref()
                .and_then(read_color_asset)
                .unwrap_or_else(|| {
                    bar_data::ColorBuffer::new(PAINTED_TEXTURE_RES, PAINTED_TEXTURE_RES).unwrap()
                });
            self.paint
                .live_paint
                .insert(PaintKey::Node(node_id), LivePaintBuffer::Color(cb));
        }

        // Apply dab to the live color buffer at native texture resolution.
        if let Some(LivePaintBuffer::Color(live_cb)) =
            self.paint.live_paint.get_mut(&PaintKey::Node(node_id))
        {
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

    /// Persist the live paint buffer for `node_id` to the node's binary
    /// asset file (the executors read this file every eval, so writing here
    /// is what makes painted edits survive the next eval). Pushes an undo
    /// snapshot and marks the node dirty so the preview re-evaluates.
    /// Removes the buffer from `live_paint`.
    pub fn flush_live_paint(&mut self, node_id: NodeId) {
        let Some(buffer) = self.paint.live_paint.remove(&PaintKey::Node(node_id)) else {
            return;
        };
        let asset_path = self
            .graph
            .get_node(node_id)
            .and_then(|n| n.params.get("asset_path"))
            .and_then(|v| match v {
                ParamValue::String(s) if !s.is_empty() => Some(s.clone()),
                _ => None,
            });
        let Some(asset_path) = asset_path else {
            tracing::warn!(
                ?node_id,
                "Paint flush: node has no asset_path; painted edits dropped"
            );
            return;
        };
        // Record undo BEFORE writing so the snapshot captures the
        // pre-stroke asset bytes (read off disk inside push_undo).
        let path_buf = std::path::PathBuf::from(&asset_path);
        self.push_undo_with_painted("Paint layer", std::iter::once(path_buf));
        match buffer {
            LivePaintBuffer::Height(hm) => {
                // PaintedHeightmap is the only Height-buffer node now
                // (Sculpt was removed). FinalComposition paint layers
                // in Phase 2 will dispatch encoding by layer kind, not
                // by node_type.
                let kind = bar_project::AssetKind::GrayscaleF32;
                if let Err(e) = write_height_asset(&asset_path, &hm, kind) {
                    tracing::error!(error = %e, path = %asset_path, "Paint flush: write heightmap asset failed");
                    return;
                }
            }
            LivePaintBuffer::Color(cb) => {
                if let Err(e) = write_color_asset(&asset_path, &cb) {
                    tracing::error!(error = %e, path = %asset_path, "Paint flush: write color asset failed");
                    return;
                }
            }
            LivePaintBuffer::MaskedValue { .. } => {
                // PaintedHeightmap / PaintedTexture (2D paint nodes)
                // don't use the masked-value buffer -- that's only
                // produced by FC metalmap / typemap brush handlers,
                // flushed via `flush_live_paint_to_fc_layer`. Reaching
                // here means someone stuck the wrong variant in the
                // 2D-paint key; just drop it loudly rather than write
                // garbage to disk.
                tracing::warn!(
                    ?node_id,
                    "Paint flush: MaskedValue live buffer found for a 2D-paint node; dropped"
                );
                return;
            }
        }
        if let Some(node) = self.graph.get_node_mut(node_id) {
            node.mark_dirty();
        }
        // NOTE: deliberately do NOT bump `paint.asset_revision` here.
        // The brush has already uploaded the painted heightmap directly
        // to the GPU and overwritten the inspector mirror in
        // `apply_brush_to_sculpt_layer`, so re-firing the eval would
        // produce identical output after a visible "reload" gap while
        // the eval thread runs. The asset file is now the source of
        // truth for any future eval (e.g. after undo / a graph edit).
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

    /// Apply the heightmap brush to `FinalComposition`'s heightmap
    /// layer. Mirrors `apply_brush_to_sculpt_layer` but targets FC's
    /// per-kind asset slot instead of a graph node's own asset.
    /// On the first stroke of an unpainted slot, mints a new asset
    /// id + path (under `<project>/final_composition/<id>.bin`) and
    /// stamps it onto the FC node params, then creates the file with
    /// "neutral" bytes so the live buffer can read it back.
    pub fn apply_brush_to_fc_heightmap_layer(
        &mut self,
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

        let Some(fc_node_id) = find_final_composition_node(&self.graph) else {
            return false;
        };

        // Layer resolution. FC heightmap layers are u8 deltas at a
        // fixed working resolution that matches the inspector heightmap;
        // there's no per-layer `resolution` param yet (the heightmap
        // layer just tracks the inspector's dims so dabs round-trip
        // without resampling).
        let node_res = map_w.max(map_h) as u32;

        let key = PaintKey::FCLayer(FCLayerKind::Heightmap);

        // Initialise live buffer from the FC heightmap-layer asset.
        // If the slot is unpainted (asset_id empty), seed the buffer
        // with neutral deltas (all 0.5 in [0,1] f32, == byte 128).
        if !self.paint.live_paint.contains_key(&key) {
            let asset_path = fc_layer_asset_path(&self.graph, fc_node_id, FCLayerKind::Heightmap);
            let live_hm = asset_path
                .as_deref()
                .and_then(read_height_asset)
                .unwrap_or_else(|| {
                    // Fresh layer: all-neutral delta buffer.
                    let mut hm = bar_data::Heightmap::new(node_res, node_res).unwrap();
                    for y in 0..hm.height() {
                        for x in 0..hm.width() {
                            let _ = hm.set(x, y, 0.5);
                        }
                    }
                    hm
                });
            self.paint
                .live_paint
                .insert(key, LivePaintBuffer::Height(live_hm));
        }

        // Live buffer is at map resolution -- scale is 1:1 for FC heightmap.
        if let Some(LivePaintBuffer::Height(live_hm)) = self.paint.live_paint.get_mut(&key) {
            apply_brush_dab(live_hm, hx, hy, &self.paint.brush);
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

    /// Apply the color brush to `FinalComposition`'s color paint layer.
    /// Stamps `brush.color_rgb` at the painted pixels into a live
    /// `ColorBuffer` whose alpha channel doubles as the per-pixel
    /// "painted this layer" mask (RGB+alpha=255 = painted; alpha=0 =
    /// passes the underlying graph color through).
    pub fn apply_color_brush_to_fc_color_layer(&mut self, hx: f32, hy: f32) -> bool {
        let (hm_w, hm_h) = match self.paint.heightmap.as_ref() {
            Some(hm) => (hm.width(), hm.height()),
            None => return false,
        };
        let Some(fc_node_id) = find_final_composition_node(&self.graph) else {
            return false;
        };
        let map_dim = (hm_w.max(hm_h) as f32).max(1.0);
        let u = (hx / hm_w as f32).clamp(0.0, 1.0);
        let v = (hy / hm_h as f32).clamp(0.0, 1.0);
        let ru = (self.paint.brush.radius_px / map_dim).max(0.001);
        let [r, g, b] = self.paint.brush.color_rgb;

        let key = PaintKey::FCLayer(FCLayerKind::Color);
        if !self.paint.live_paint.contains_key(&key) {
            let asset_path = fc_layer_asset_path(&self.graph, fc_node_id, FCLayerKind::Color);
            let cb = asset_path
                .as_deref()
                .and_then(read_rgba_color_asset)
                .unwrap_or_else(|| {
                    // Fresh layer: all-transparent RGBA (alpha=0 means
                    // "this pixel passes the upstream colour through").
                    bar_data::ColorBuffer::new(hm_w, hm_h).unwrap()
                });
            self.paint
                .live_paint
                .insert(key, LivePaintBuffer::Color(cb));
        }
        if let Some(LivePaintBuffer::Color(live_cb)) = self.paint.live_paint.get_mut(&key) {
            // The existing stamp helper writes alpha=1.0 at painted
            // pixels, which is exactly the "mask" semantic the FC
            // color layer's composite expects.
            stamp_color_dab_in_buffer(live_cb, u, v, ru, [r, g, b]);
        }
        // Mirror into the color_buffer cache for instant viewport
        // feedback (same path the PaintedTexture brush uses).
        if let Some(ref mut cb) = self.paint.color_buffer {
            stamp_color_dab_in_buffer(cb, u, v, ru, [r, g, b]);
        }

        self.paint.brush_stroking = true;
        self.project.is_dirty = true;
        true
    }

    /// Apply the value-stamping brush to FC's metalmap or typemap
    /// paint layer. Uses `brush.paint_value` as the stamped value
    /// (range `[0, 1]`); painted pixels get marked as "touched" so
    /// the flush logic can write byte values vs the `0xFF` "untouched"
    /// sentinel that the FC executor's overlay composite expects.
    pub fn apply_value_brush_to_fc_layer(&mut self, kind: FCLayerKind, hx: f32, hy: f32) -> bool {
        debug_assert!(matches!(kind, FCLayerKind::Metalmap | FCLayerKind::Typemap));
        let (map_w, map_h) = match self.paint.heightmap.as_ref() {
            Some(hm) => (hm.width(), hm.height()),
            None => return false,
        };
        let Some(_fc_node_id) = find_final_composition_node(&self.graph) else {
            return false;
        };
        let key = PaintKey::FCLayer(kind);
        self.paint.live_paint.entry(key).or_insert_with(|| {
            // Fresh: zero-value buffer, no pixels touched yet.
            let value = bar_data::Heightmap::new(map_w, map_h).unwrap();
            let touched = vec![false; (map_w as usize) * (map_h as usize)];
            LivePaintBuffer::MaskedValue { value, touched }
        });
        if let Some(LivePaintBuffer::MaskedValue { value, touched }) =
            self.paint.live_paint.get_mut(&key)
        {
            stamp_value_dab(
                value,
                touched,
                hx,
                hy,
                self.paint.brush.radius_px,
                self.paint.brush.paint_value,
            );
        }
        self.paint.brush_stroking = true;
        self.project.is_dirty = true;
        true
    }

    /// End an FC-layer brush stroke: flush the live buffer to disk
    /// (encoding to the layer's on-disk format), stamp the FC node's
    /// asset id / path on first use, clear stroke flags. Mirror of
    /// `end_brush_stroke_on_layer` but for FC layer targets.
    pub fn end_brush_stroke_on_fc_layer(&mut self, kind: FCLayerKind) {
        self.flush_live_paint_to_fc_layer(kind);
        self.paint.brush_stroking = false;
        self.paint.brush.flatten_target = None;
    }

    /// Persist the live FC-layer buffer to disk. FC's per-kind
    /// `{kind}_layer_asset_id` and `{kind}_layer_asset_path` are
    /// guaranteed to be populated at project bootstrap (scan, new
    /// project, macro drop, load) -- so this just resolves the path,
    /// snapshots the pre-stroke file bytes for undo, and writes the
    /// new bytes. The asset file itself comes into existence here on
    /// the first stroke into a kind.
    fn flush_live_paint_to_fc_layer(&mut self, kind: FCLayerKind) {
        let key = PaintKey::FCLayer(kind);
        let Some(buffer) = self.paint.live_paint.remove(&key) else {
            return;
        };
        let Some(fc_node_id) = find_final_composition_node(&self.graph) else {
            return;
        };
        let Some(asset_path) = fc_layer_asset_path(&self.graph, fc_node_id, kind) else {
            tracing::warn!(
                ?kind,
                "Paint flush: FC layer asset_path missing (should have been populated at bootstrap)"
            );
            return;
        };
        // Snapshot for undo. If the file already exists, its current
        // bytes get interned in paint_history; if not (first stroke
        // into this kind), the snapshot captures an "asset absent"
        // state and undo restores that by deleting the file -- the
        // composite then falls through to pass-through.
        self.push_undo_with_painted(
            "Paint FC layer",
            vec![std::path::PathBuf::from(&asset_path)],
        );
        // Ensure the asset directory exists -- the file is created
        // lazily on first paint, so the directory might not exist yet
        // either.
        if let Some(parent) = std::path::Path::new(&asset_path).parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::error!(error = %e, dir = %parent.display(), "Could not create FC asset dir");
                return;
            }
        }
        // Per-kind on-disk encoding:
        //   Heightmap         -> GrayscaleU8 deltas (128 = neutral).
        //   Color             -> RgbaU8 (alpha=0 is the untouched mask).
        //   Metalmap, Typemap -> GrayscaleU8 sentinel (0xFF = untouched,
        //                        else value*254).
        let write_result = match (buffer, kind) {
            (LivePaintBuffer::Height(hm), FCLayerKind::Heightmap) => {
                write_height_asset(&asset_path, &hm, bar_project::AssetKind::GrayscaleU8)
            }
            (LivePaintBuffer::Color(cb), FCLayerKind::Color) => {
                write_rgba_color_asset(&asset_path, &cb)
            }
            (
                LivePaintBuffer::MaskedValue { value, touched },
                FCLayerKind::Metalmap | FCLayerKind::Typemap,
            ) => write_sentinel_value_asset(&asset_path, &value, &touched),
            _ => {
                tracing::warn!(
                    ?kind,
                    "Paint flush: live buffer variant doesn't match FC layer kind"
                );
                return;
            }
        };
        if let Err(e) = write_result {
            tracing::error!(error = %e, path = %asset_path, "Paint flush: write FC layer asset failed");
            return;
        }
        if let Some(node) = self.graph.get_node_mut(fc_node_id) {
            node.mark_dirty();
        }
    }
}

/// Walk the graph for the project's single `FinalComposition` node.
/// Returns `None` only if FC is missing (should never happen after the
/// auto-insert in `scan_to_project`, but the brush flow tolerates it
/// by no-op'ing rather than panicking).
pub(crate) fn find_final_composition_node(graph: &bar_graph::GraphEngine) -> Option<NodeId> {
    graph
        .nodes()
        .iter()
        .find(|(_, n)| n.node_type == bar_graph::NodeType::FinalComposition)
        .map(|(&id, _)| id)
}

/// Look up FC's per-kind layer asset path. Returns `None` when the
/// slot is unpainted (asset_path empty / unset) or when the FC node
/// itself can't be found.
pub(crate) fn fc_layer_asset_path(
    graph: &bar_graph::GraphEngine,
    fc_node_id: NodeId,
    kind: FCLayerKind,
) -> Option<String> {
    let path_key = format!("{}_layer_asset_path", kind.param_prefix());
    let node = graph.get_node(fc_node_id)?;
    match node.params.get(&path_key) {
        Some(ParamValue::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// Load an `RgbaU8` FC color-layer asset into a `ColorBuffer`. The
/// alpha channel doubles as the per-pixel "painted" mask (alpha=0 ==
/// untouched, passes upstream colour through; alpha=255 == painted).
/// Returns `None` if the file is missing / wrong kind / wrong size.
fn read_rgba_color_asset(asset_path: &str) -> Option<bar_data::ColorBuffer> {
    let path = std::path::Path::new(asset_path);
    if !path.exists() {
        return None;
    }
    let (header, data) = bar_project::read_asset_file(path).ok()?;
    if !matches!(header.kind, bar_project::AssetKind::RgbaU8) {
        return None;
    }
    let w = header.width.max(1);
    let h = header.height.max(1);
    let need = (w as usize) * (h as usize) * 4;
    if data.len() != need {
        return None;
    }
    let mut cb = bar_data::ColorBuffer::new(w, h).ok()?;
    for y in 0..h {
        for x in 0..w {
            let idx = ((y * w + x) * 4) as usize;
            cb.set(
                x,
                y,
                [
                    data[idx] as f32 / 255.0,
                    data[idx + 1] as f32 / 255.0,
                    data[idx + 2] as f32 / 255.0,
                    data[idx + 3] as f32 / 255.0,
                ],
            );
        }
    }
    Some(cb)
}

/// Persist an FC color-layer live buffer as `RgbaU8`. Alpha is carried
/// straight through from the buffer (the brush stamp sets it to 1.0
/// at painted pixels and the unpainted defaults stay at 0.0, which is
/// the "untouched" sentinel the executor's overlay composite reads).
fn write_rgba_color_asset(
    asset_path: &str,
    cb: &bar_data::ColorBuffer,
) -> Result<(), Box<dyn std::error::Error>> {
    let w = cb.width();
    let h = cb.height();
    let mut bytes = Vec::with_capacity((w as usize) * (h as usize) * 4);
    for y in 0..h {
        for x in 0..w {
            let rgba = cb.get(x, y).unwrap_or([0.0; 4]);
            bytes.push((rgba[0] * 255.0).round() as u8);
            bytes.push((rgba[1] * 255.0).round() as u8);
            bytes.push((rgba[2] * 255.0).round() as u8);
            bytes.push((rgba[3] * 255.0).round() as u8);
        }
    }
    let header = bar_project::AssetHeader {
        kind: bar_project::AssetKind::RgbaU8,
        width: w,
        height: h,
    };
    bar_project::write_asset_file(std::path::Path::new(asset_path), header, &bytes)
        .map_err(|e| e.into())
}

/// Persist a `MaskedValue` live buffer (metalmap / typemap) as
/// `GrayscaleU8` with the executor's sentinel encoding: byte `0xFF`
/// at untouched pixels (so the composite passes the upstream value
/// through), `(value * 254).round()` at touched pixels.
fn write_sentinel_value_asset(
    asset_path: &str,
    value: &bar_data::Heightmap,
    touched: &[bool],
) -> Result<(), Box<dyn std::error::Error>> {
    let w = value.width();
    let h = value.height();
    if touched.len() != (w as usize) * (h as usize) {
        return Err("write_sentinel_value_asset: touched mask size mismatch".into());
    }
    let mut bytes = Vec::with_capacity(touched.len());
    let data = value.data();
    for (i, &t) in touched.iter().enumerate() {
        if !t {
            bytes.push(0xFFu8);
        } else {
            let v = data[i].clamp(0.0, 1.0);
            bytes.push((v * 254.0).round() as u8);
        }
    }
    let header = bar_project::AssetHeader {
        kind: bar_project::AssetKind::GrayscaleU8,
        width: w,
        height: h,
    };
    bar_project::write_asset_file(std::path::Path::new(asset_path), header, &bytes)
        .map_err(|e| e.into())
}

/// Load the on-disk paint state for a `PaintedHeightmap` / `Sculpt` node
/// from its binary asset, preserving native resolution. Returns `None` if
/// the file doesn't exist, can't be read, or holds an unexpected kind --
/// callers fall back to a blank heightmap.
fn read_height_asset(asset_path: &str) -> Option<bar_data::Heightmap> {
    let path = std::path::Path::new(asset_path);
    if !path.exists() {
        return None;
    }
    let (header, data) = bar_project::read_asset_file(path).ok()?;
    let w = header.width.max(1);
    let h = header.height.max(1);
    match header.kind {
        bar_project::AssetKind::GrayscaleF32 => {
            let need = (w as usize) * (h as usize) * 4;
            if data.len() != need {
                return None;
            }
            let f32_data: Vec<f32> = data
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            bar_data::Heightmap::frbar_data(w, h, f32_data).ok()
        }
        bar_project::AssetKind::GrayscaleU8 => {
            let f32_data: Vec<f32> = data.iter().map(|&b| (b as f32) / 255.0).collect();
            bar_data::Heightmap::frbar_data(w, h, f32_data).ok()
        }
        bar_project::AssetKind::RgbU8 | bar_project::AssetKind::RgbaU8 => None,
    }
}

/// Persist a height live buffer to its binary asset, encoded according
/// to the requested `kind`:
///
/// - `GrayscaleF32` -- absolute heights in `[0, 1]` written verbatim
///   (used by `PaintedHeightmap`; full precision avoids the terracing
///   u8 quantisation would cause on maps with more than ~256 elevation
///   levels).
/// - `GrayscaleU8` -- delta encoding (used by `Sculpt`). The live
///   buffer's `[0, 1]` range maps to `[0, 255]`; the executor reads
///   128 as "no change" so the brush convention (0.5 neutral, > 0.5
///   positive delta, < 0.5 negative) round-trips correctly.
///
/// `RgbU8` is rejected (use `write_color_asset`).
fn write_height_asset(
    asset_path: &str,
    hm: &bar_data::Heightmap,
    kind: bar_project::AssetKind,
) -> Result<(), Box<dyn std::error::Error>> {
    let w = hm.width();
    let h = hm.height();
    let bytes = match kind {
        bar_project::AssetKind::GrayscaleF32 => {
            let mut out = Vec::with_capacity((w as usize) * (h as usize) * 4);
            for v in hm.data() {
                out.extend_from_slice(&v.to_le_bytes());
            }
            out
        }
        bar_project::AssetKind::GrayscaleU8 => hm
            .data()
            .iter()
            .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect(),
        bar_project::AssetKind::RgbU8 | bar_project::AssetKind::RgbaU8 => {
            return Err(
                "write_height_asset: Rgb / Rgba kinds not supported (use write_color_asset)".into(),
            );
        }
    };
    let header = bar_project::AssetHeader {
        kind,
        width: w,
        height: h,
    };
    bar_project::write_asset_file(std::path::Path::new(asset_path), header, &bytes)
        .map_err(|e| e.into())
}

/// Load the on-disk paint state for a `PaintedTexture` node, preserving
/// native resolution. Returns `None` if missing / wrong kind.
fn read_color_asset(asset_path: &str) -> Option<bar_data::ColorBuffer> {
    let path = std::path::Path::new(asset_path);
    if !path.exists() {
        return None;
    }
    let (header, data) = bar_project::read_asset_file(path).ok()?;
    let w = header.width.max(1);
    let h = header.height.max(1);
    if !matches!(header.kind, bar_project::AssetKind::RgbU8) {
        return None;
    }
    let need = (w as usize) * (h as usize) * 3;
    if data.len() != need {
        return None;
    }
    let mut cb = bar_data::ColorBuffer::new(w, h).ok()?;
    for y in 0..h {
        for x in 0..w {
            let idx = ((y * w + x) * 3) as usize;
            cb.set(
                x,
                y,
                [
                    data[idx] as f32 / 255.0,
                    data[idx + 1] as f32 / 255.0,
                    data[idx + 2] as f32 / 255.0,
                    1.0,
                ],
            );
        }
    }
    Some(cb)
}

/// Persist a `PaintedTexture` live buffer to its binary asset as `RgbU8`.
fn write_color_asset(
    asset_path: &str,
    cb: &bar_data::ColorBuffer,
) -> Result<(), Box<dyn std::error::Error>> {
    let w = cb.width();
    let h = cb.height();
    let mut bytes = Vec::with_capacity((w as usize) * (h as usize) * 3);
    for y in 0..h {
        for x in 0..w {
            let rgba = cb.get(x, y).unwrap_or([0.0; 4]);
            bytes.push((rgba[0] * 255.0).round() as u8);
            bytes.push((rgba[1] * 255.0).round() as u8);
            bytes.push((rgba[2] * 255.0).round() as u8);
        }
    }
    let header = bar_project::AssetHeader {
        kind: bar_project::AssetKind::RgbU8,
        width: w,
        height: h,
    };
    bar_project::write_asset_file(std::path::Path::new(asset_path), header, &bytes)
        .map_err(|e| e.into())
}

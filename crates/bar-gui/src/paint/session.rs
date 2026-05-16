//! Brush configuration, live paint caches, and sculpt-layer selection used
//! by the 2D inspector and the 3D sculpt viewport.
//!
//! `PaintSession` groups all per-project paint state so the application root
//! does not declare each field individually. Its lifetime is one project:
//! `invalidate_on_graph_reset` clears everything on project switch / new
//! project / graph reset.

use bar_graph::NodeId;
use eframe::egui;
use std::collections::HashMap;

/// Key into `PaintSession::live_paint`. Distinguishes 2D-paint nodes
/// (each with its own asset) from `FinalComposition`'s per-kind paint
/// layers. A single `PaintSession` may hold live buffers for both
/// (e.g. user paints PaintedHeightmap then switches to FC heightmap
/// without flushing -- each accumulates separately).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaintKey {
    /// 2D-paint graph node (PaintedHeightmap, PaintedTexture).
    Node(NodeId),
    /// FinalComposition paint layer of a specific kind.
    FCLayer(FCLayerKind),
}

/// Which paint layer of `FinalComposition` a brush stroke targets.
/// Set by the Sculpt3D layer tool buttons; consumed by the brush flow
/// in `viewport.rs` and the flush logic in `paint/brush.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FCLayerKind {
    Heightmap,
    Color,
    Metalmap,
    Typemap,
}

impl FCLayerKind {
    /// Snake-case prefix used in `FinalComposition` node params:
    /// `<prefix>_layer_asset_id`, `<prefix>_layer_asset_path`.
    pub fn param_prefix(self) -> &'static str {
        match self {
            Self::Heightmap => "heightmap",
            Self::Color => "color",
            Self::Metalmap => "metalmap",
            Self::Typemap => "typemap",
        }
    }

    /// Human-readable label for tool buttons / panels.
    pub fn label(self) -> &'static str {
        match self {
            Self::Heightmap => "Heightmap",
            Self::Color => "Color",
            Self::Metalmap => "Metalmap",
            Self::Typemap => "Typemap",
        }
    }
}

/// What primary action a left-click + drag in the 2D Inspector does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorMode {
    /// Click to add / drag existing markers / right-click to delete.
    Spawns,
    /// Drag-paint with the heightmap brush.
    Sculpt,
}

/// Active editing tool. Determines what viewport clicks do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushTool {
    /// Feature layer: click to select/place features.
    Pointer,
    Raise,
    Lower,
    Smooth,
    /// Pull pixels toward a target height captured at stroke start.
    Flatten,
}

impl BrushTool {
    pub(crate) fn label(self) -> &'static str {
        match self {
            BrushTool::Pointer => "Pointer",
            BrushTool::Raise => "Raise",
            BrushTool::Lower => "Lower",
            BrushTool::Smooth => "Smooth",
            BrushTool::Flatten => "Flatten",
        }
    }
}

/// In-memory buffer for a live brush stroke on a paintable node.
/// Held in `PaintSession::live_paint` while the mouse is down; flushed to
/// node params and cleared on stroke end.
pub enum LivePaintBuffer {
    Height(bar_data::Heightmap),
    Color(bar_data::ColorBuffer),
    /// Per-pixel value + "has been painted this stroke" mask. Used by
    /// the quantised FC paint layers (metalmap, typemap) where the
    /// encoded on-disk byte reserves `0xFF` as a "no paint" sentinel,
    /// so the live buffer needs to track which pixels are touched
    /// separately from the value at them.
    MaskedValue {
        value: bar_data::Heightmap,
        touched: Vec<bool>,
    },
}

/// Live brush configuration shared between the 2D Inspector and the 3D
/// sculpt viewport.
#[derive(Clone, Debug)]
pub struct BrushState {
    pub tool: BrushTool,
    /// Radius in heightmap pixels (1 px = 8 elmos).
    pub radius_px: f32,
    /// Strength per dab (heightmap is f32 [0,1]; 0.01 = 1% of full range).
    pub strength: f32,
    /// Falloff exponent (1.0 = linear, 2.0 = squared, sharper centre).
    pub falloff: f32,
    /// Target height for Flatten mode, captured at stroke start.
    pub flatten_target: Option<f32>,
    /// Brush colour for color-target layers. Packed RGB; alpha is implicit 1.0.
    pub color_rgb: [u8; 3],
    /// Stamp value for value-layer painting. Range [0, 1].
    pub paint_value: f32,
}

impl Default for BrushState {
    fn default() -> Self {
        Self {
            // Pointer is the no-op default: the brush cursor doesn't render
            // and viewport clicks select features. Selecting a sculpt layer
            // promotes the tool to `Raise` (see `sculpt3d::draw_layer_row`),
            // so the user doesn't lose access to the brushes -- they're just
            // not active on first entry.
            tool: BrushTool::Pointer,
            color_rgb: [0x8B, 0x73, 0x55],
            paint_value: 1.0,
            radius_px: 32.0,
            strength: 0.02,
            falloff: 2.0,
            flatten_target: None,
        }
    }
}

pub struct PaintSession {
    pub inspector_mode: InspectorMode,
    pub brush: BrushState,
    /// True while a sculpt stroke is in progress (mouse held down).
    pub brush_stroking: bool,
    /// The node currently active in the sculpt layer panel.
    /// Brush strokes in the 3D viewport write to this node's live buffer.
    pub selected_sculpt_layer: Option<NodeId>,
    /// Which `FinalComposition` paint layer the Sculpt3D layer panel
    /// has selected. When `Some`, brush strokes write to FC's
    /// per-kind layer asset rather than to a 2D-paint node. The two
    /// selections (`selected_sculpt_layer` vs `selected_fc_layer`) are
    /// mutually exclusive -- whichever was set most recently is the
    /// active brush target.
    pub selected_fc_layer: Option<FCLayerKind>,
    /// Per-target live paint buffers, held for the duration of one stroke.
    /// Cleared when the stroke ends and the buffer is flushed to disk.
    /// Keys distinguish 2D-paint nodes (`Node(id)`) from FC layers
    /// (`FCLayer(kind)`) so a single stroke on either path uses the
    /// right buffer.
    pub live_paint: HashMap<PaintKey, LivePaintBuffer>,
    /// Brush radius (heightmap pixels) for `PaintedHeightmap` /
    /// `PaintedTexture` / `Sculpt` in-node paint canvases.
    pub paint_brush_radius: f32,
    /// Strength for the Sculpt node's delta brush (0.0-1.0).
    pub sculpt_brush_strength: f32,
    /// Last heightmap fed in by `bar-app` after a preview eval.
    pub heightmap: Option<bar_data::Heightmap>,
    /// Bumped whenever `heightmap` is replaced.
    pub heightmap_rev: u64,
    /// Bumped whenever a paint asset file is mutated by the brush flush
    /// or restored by an undo/redo. Mixed into `preview_cache_key` so
    /// the eval re-fires even when the graph params didn't change (the
    /// painted bytes live in `<project>/assets/*.bin`, not in the graph,
    /// so the upstream content hash on its own is blind to them).
    pub asset_revision: u64,
    /// Cached egui texture for the 2D inspector backdrop.
    pub texture: Option<egui::TextureHandle>,
    pub texture_rev: u64,
    pub color_buffer: Option<bar_data::ColorBuffer>,
    pub metalmap: Option<bar_data::Heightmap>,
    pub typemap: Option<bar_data::Heightmap>,
    /// Retained egui texture handles for `PaintedHeightmap` canvases.
    pub mask_textures: HashMap<NodeId, egui::TextureHandle>,
}

impl Default for PaintSession {
    fn default() -> Self {
        Self {
            inspector_mode: InspectorMode::Spawns,
            brush: BrushState::default(),
            brush_stroking: false,
            selected_sculpt_layer: None,
            selected_fc_layer: None,
            live_paint: HashMap::new(),
            paint_brush_radius: 4.0,
            sculpt_brush_strength: 0.5,
            heightmap: None,
            heightmap_rev: 0,
            asset_revision: 0,
            texture: None,
            texture_rev: 0,
            color_buffer: None,
            metalmap: None,
            typemap: None,
            mask_textures: HashMap::new(),
        }
    }
}

impl PaintSession {
    /// Drop the live caches so the next graph eval repopulates them.
    /// Called on project switch / new project / graph reset.
    pub fn invalidate_on_graph_reset(&mut self) {
        self.brush = BrushState::default();
        self.brush_stroking = false;
        self.selected_sculpt_layer = None;
        self.selected_fc_layer = None;
        self.live_paint.clear();
        self.heightmap = None;
        self.heightmap_rev = self.heightmap_rev.wrapping_add(1);
        self.texture = None;
        self.texture_rev = self.texture_rev.wrapping_add(1);
        self.color_buffer = None;
        self.metalmap = None;
        self.typemap = None;
        self.mask_textures.clear();
    }
}

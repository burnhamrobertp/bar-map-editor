//! Brush, sculpt-lock, and per-layer paint caches used by the 2D
//! inspector and the 3D viewport for per-stroke feedback. The caches
//! mirror whatever the most recent graph eval produced; brush dabs
//! mutate them in place between evals so the user sees strokes land
//! before the eval has caught up.
//!
//! `PaintSession` is grouped here so the application root doesn't have
//! to declare every paint-cache field separately. Its lifetime is one
//! project: `invalidate_on_graph_reset` drops the live caches on
//! project switch / new project / graph reset.

use bar_graph::NodeId;
use eframe::egui;
use std::collections::HashMap;

/// What primary action a left-click + drag in the 2D Inspector does.
/// Switched via the radio control at the top of the inspector window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorMode {
    /// Click to add / drag existing markers / right-click to delete.
    Spawns,
    /// Drag-paint with the heightmap brush.
    Sculpt,
}

/// Heightmap-sculpting brush mode. Each tool applies a different
/// transformation to the pixels under the brush footprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushTool {
    Raise,
    Lower,
    Smooth,
    /// Pull pixels toward a target height (the height under the cursor when the
    /// stroke started).
    Flatten,
}

impl BrushTool {
    pub(crate) fn label(self) -> &'static str {
        match self {
            BrushTool::Raise => "Raise",
            BrushTool::Lower => "Lower",
            BrushTool::Smooth => "Smooth",
            BrushTool::Flatten => "Flatten",
        }
    }
}

/// What kind of data the brush writes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushTarget {
    Heightmap,
    Color,
    Metalmap,
    Typemap,
}

impl BrushTarget {
    pub(crate) fn label(self) -> &'static str {
        match self {
            BrushTarget::Heightmap => "Heightmap",
            BrushTarget::Color => "Colour",
            BrushTarget::Metalmap => "Metal",
            BrushTarget::Typemap => "Type",
        }
    }
    pub(crate) fn is_available(self) -> bool {
        true
    }
}

/// Live brush configuration shared between the 2D Inspector and the
/// 3D viewport. Pixel-radius applies to the heightmap; the inspector
/// scales it to its rendered image size.
#[derive(Clone, Debug)]
pub struct BrushState {
    pub tool: BrushTool,
    pub target: BrushTarget,
    /// Radius in heightmap pixels (1 px = 8 elmos).
    pub radius_px: f32,
    /// Strength in normalized heightmap units per stroke-application.
    /// (Heightmap is f32 [0,1]; 0.01 = 1% of full range per dab.)
    pub strength: f32,
    /// Falloff exponent (1.0 = linear, 2.0 = squared, sharper centre).
    pub falloff: f32,
    /// Target height for Flatten mode, captured at stroke start.
    pub flatten_target: Option<f32>,
    /// Brush colour for `BrushTarget::Color`. Packed RGB; alpha is
    /// implicit 1.0 -- full coverage.
    pub color_rgb: [u8; 3],
    /// Stamp value for `BrushTarget::Metalmap` / `BrushTarget::Typemap`.
    /// Range `[0, 1]` -- for metal it's density (0 = none, 1 = max);
    /// for type it's a quantised id (multiplied by 255 at export
    /// time).
    pub paint_value: f32,
}

impl Default for BrushState {
    fn default() -> Self {
        Self {
            tool: BrushTool::Raise,
            target: BrushTarget::Heightmap,
            color_rgb: [0x8B, 0x73, 0x55],
            paint_value: 1.0,
            radius_px: 32.0,
            strength: 0.02,
            falloff: 2.0,
            flatten_target: None,
        }
    }
}

/// Persistent sculpt + paint layers. Mirrored to disk via the sculpt
/// sidecar; merged with graph evaluation output at export time.
#[derive(Default)]
pub struct SculptState {
    /// Signed height delta. Zero where unmodified.
    pub height_delta: Option<bar_data::Heightmap>,
    pub metal_overlay: Option<bar_data::Heightmap>,
    pub metal_alpha: Option<bar_data::Heightmap>,
    pub type_overlay: Option<bar_data::Heightmap>,
    pub type_alpha: Option<bar_data::Heightmap>,
    /// RGBA texture overlay. rgb = colour, alpha = coverage.
    pub texture_overlay: Option<bar_data::ColorBuffer>,
    pub dirty: bool,
}

pub struct PaintSession {
    pub inspector_mode: InspectorMode,
    pub brush: BrushState,
    /// True while a sculpt stroke is in progress (mouse held
    /// down). Used to capture the Flatten target at stroke start.
    pub brush_stroking: bool,
    /// Project-level in-memory sculpt data.
    pub sculpt: SculptState,
    /// Brush radius (heightmap pixels) for `PaintedHeightmap` /
    /// `PaintedTexture` / `Sculpt` in-node paint canvases. The 2D
    /// inspector brush uses `brush.radius_px` instead.
    pub paint_brush_radius: f32,
    /// Strength for the Sculpt node's delta brush (0.0-1.0).
    pub sculpt_brush_strength: f32,
    /// Last heightmap fed in by `bar-app` after a preview eval.
    pub heightmap: Option<bar_data::Heightmap>,
    /// Bumped whenever `heightmap` is replaced.
    pub heightmap_rev: u64,
    /// Cached egui texture for the 2D inspector backdrop.
    pub texture: Option<egui::TextureHandle>,
    pub texture_rev: u64,
    pub color_buffer: Option<bar_data::ColorBuffer>,
    pub metalmap: Option<bar_data::Heightmap>,
    pub typemap: Option<bar_data::Heightmap>,
    /// Retained egui texture handles for `PaintedHeightmap` canvases.
    pub mask_textures: HashMap<NodeId, egui::TextureHandle>,
    /// Populated by `pack_sculpt_record` before `build_project` is
    /// called. Taken with `.take()` in `build_project`.
    pub pending_sculpt_record: Option<bar_project::SculptRecord>,
}

impl Default for PaintSession {
    fn default() -> Self {
        Self {
            inspector_mode: InspectorMode::Spawns,
            brush: BrushState::default(),
            brush_stroking: false,
            sculpt: SculptState::default(),
            paint_brush_radius: 4.0,
            sculpt_brush_strength: 0.5,
            heightmap: None,
            heightmap_rev: 0,
            texture: None,
            texture_rev: 0,
            color_buffer: None,
            metalmap: None,
            typemap: None,
            mask_textures: HashMap::new(),
            pending_sculpt_record: None,
        }
    }
}

impl PaintSession {
    /// Drop the live caches so the next graph eval repopulates them.
    /// Called on project switch / new project / graph reset.
    pub fn invalidate_on_graph_reset(&mut self) {
        self.brush = BrushState::default();
        self.brush_stroking = false;
        self.sculpt = SculptState::default();
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

//! Brush + paint logic.
//!
//! **STATUS: TEMPORARILY BROKEN / ON HOLD (Sculpt3D paint flow).**
//! The brush flow that writes into FinalComposition's per-kind paint
//! layers has known correctness issues -- multi-stroke undo can drop
//! work, and the resolution of the viewport's heightmap can collapse
//! on undo. The 2D-inspector paint flow (PaintedHeightmap /
//! PaintedTexture node-local brushes) still works and stays in this
//! module; the FC-targeted brush paths (`apply_brush_to_fc_*_layer`,
//! `flush_live_paint_to_fc_layer`) are the parts under review. See
//! `docs/TODO.md` "On hold" and `docs/3d-painting-plan.md`. Don't
//! rework the FC paint flow without agreeing on a new direction.
//!
//! - `session` -- the `PaintSession` sub-state struct plus the enums it
//!   composes.
//! - `brush_math` -- pure dab-application functions (no `BarEditorApp`
//!   dependency, fully unit-testable).

pub(crate) mod brush;
pub(crate) mod brush_math;
pub(crate) mod session;

pub use session::{
    BrushState, BrushTool, FCLayerKind, InspectorMode, LivePaintBuffer, PaintKey, PaintSession,
};

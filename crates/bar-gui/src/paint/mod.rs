//! Brush + paint logic.
//!
//! - `session` -- the `PaintSession` sub-state struct (brush, sculpt
//!   layers, live paint caches), plus the brush-mode and target
//!   enums it composes.
//! - `brush_math` -- pure dab-application functions (no `BarEditorApp`
//!   dependency, fully unit-testable).

pub(crate) mod brush_math;
pub(crate) mod session;

pub use session::{BrushState, BrushTarget, BrushTool, InspectorMode, PaintSession, SculptState};

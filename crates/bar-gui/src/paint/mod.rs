//! Brush + paint logic.
//!
//! - `session` -- the `PaintSession` sub-state struct plus the enums it
//!   composes.
//! - `brush_math` -- pure dab-application functions (no `BarEditorApp`
//!   dependency, fully unit-testable).

pub(crate) mod brush;
pub(crate) mod brush_math;
pub(crate) mod session;

pub use session::{BrushState, BrushTool, InspectorMode, LivePaintBuffer, PaintSession};

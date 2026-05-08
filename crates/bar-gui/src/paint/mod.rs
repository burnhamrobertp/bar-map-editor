//! Brush + paint logic.
//!
//! `brush_math` holds the pure dab-application functions (no
//! `BarEditorApp` dependency, fully unit-testable). The brush state
//! struct (`BrushState`) and live paint caches (`PaintSession`) still
//! live in `crate::app` for now -- they migrate during Stage 2 of the
//! architecture refactor.

pub(crate) mod brush_math;

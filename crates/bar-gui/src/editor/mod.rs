//! Canvas-side editor state, grouped by concern. Each sub-state is a
//! plain field cluster owned by `BarEditorApp` -- they exist as named
//! types so related fields and the methods that operate on them live
//! in one place, instead of being scattered across `app.rs`.
//!
//! Stage 2 introduces these as field-grouping wrappers (no behaviour
//! change). Stage 3 migrates methods that only touch one cluster to
//! that cluster's `impl` block.

pub(crate) mod canvas;
pub(crate) mod map;
pub(crate) mod preview;
pub(crate) mod props_panel;
pub(crate) mod selection;
pub(crate) mod validation;
pub(crate) mod visuals;

pub(crate) use canvas::{CanvasState, CanvasView, DragConnection};
pub use map::SmfLightingSnapshot;
pub(crate) use map::{MapState, RecipeMeta};
pub(crate) use preview::PreviewState;
pub use preview::{BarVersionState, ExportStatus};
pub(crate) use props_panel::{
    PendingPropsOpen, PropsPanelState, PropsTarget, PROPS_OPEN_DELAY_MS, PROPS_OPEN_MOVE_TOLERANCE,
};
pub(crate) use selection::SelectionState;
pub(crate) use validation::{MapInfoTab, ValidationFilter, ValidationState};
pub(crate) use visuals::VisualsState;

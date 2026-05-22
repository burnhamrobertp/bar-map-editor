//! Viewport overlays -- self-contained renderers that paint on top of
//! the 3D viewport, not into a `panels::` layout slot.
//!
//! Distinct from `panels` because these aren't layout pieces -- they
//! project world-space data through the active camera and decorate the
//! viewport with rings, gizmos, and labels. Distinct from BAR's notion
//! of a "widget" (`luaui/Widgets/*.lua`) because that term is taken;
//! these are the BME-side equivalents.
//!
//! Each module here exposes a `paint` (or `compute_geometry` +
//! `paint`) free function plus any input / dim structs the call site
//! needs to assemble from app state.

pub mod metal_spots;
pub mod sun;

//! Top-level layouts -- the GUI panels each `Layout` variant draws.
//!
//! Each layout file draws only its bar-gui panels (palette, canvas,
//! side panels). The 3D viewport is owned by the `bar-app` layout
//! manager, which claims the central panel after bar-gui runs.
//!
//! Add a new layout: one file in this directory + one match arm in
//! `dispatch.rs`. No changes needed in `app.rs` or the eframe loop.

pub mod dispatch;
pub mod node_graph;
pub mod preview;
pub mod sculpt3d;
pub mod shell;

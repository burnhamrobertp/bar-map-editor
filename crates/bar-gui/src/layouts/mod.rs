//! Top-level layouts — the screen-level composition each `Layout`
//! variant produces by orchestrating panels from `crate::panels`.
//!
//! A layout is **stateless** and **pure UI/UX**: it never forks the
//! data model. The same `BarEditorApp` underlies every layout, so
//! switching from one to another is a UI swap with no migration
//! step — the user's project, brush state, paint caches, undo
//! history all stay put.
//!
//! The active layout is selected via `BarEditorApp::active_layout`
//! and persisted in user settings. eframe's `update` dispatches
//! through `crate::layouts::dispatch::draw_active`.
//!
//! New layouts are added as a single file plus a match arm:
//!
//! ```ignore
//! pub mod sculpt_focus;
//!
//! // …in dispatch.rs:
//! match app.active_layout {
//!     Layout::Standard => standard::draw(app, ctx, frame),
//!     Layout::SculptFocus => sculpt_focus::draw(app, ctx, frame),
//! }
//! ```
//!
//! A layout is allowed to read every `BarEditorApp` field but
//! should only call into `crate::panels` for actual rendering —
//! never duplicate panel logic inline. That keeps the panels as
//! the single point of truth for "how does the inspector render"
//! across every layout that wants one.

pub mod dispatch;
pub mod preview;
pub mod sculpt3d;
pub mod shell;
pub mod standard;

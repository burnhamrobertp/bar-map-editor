//! Layout dispatch — picks the active layout and delegates to it.
//!
//! Today there's only one layout (`Standard`). The match arm is
//! one line; the value of routing through this dispatcher is
//! that adding a second layout means adding one match arm here
//! and one new file in `super::`, with zero churn in `app.rs` or
//! the eframe `update` body.

use eframe::egui;

use crate::app::{BarEditorApp, Layout};

/// Render the panels that compose the user's currently active
/// layout. Called from `BarEditorApp::update` after the per-frame
/// pre-work (validation refresh, file dialog poll, …) and before
/// the post-work (autosave, repaint scheduling).
pub fn draw_active(
    app: &mut BarEditorApp,
    ctx: &egui::Context,
    frame: &mut eframe::Frame,
) {
    match app.active_layout() {
        Layout::Standard => super::standard::draw(app, ctx, frame),
    }
}

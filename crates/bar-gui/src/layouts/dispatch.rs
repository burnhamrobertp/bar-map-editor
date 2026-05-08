//! Layout dispatch — picks the active layout and delegates to it.
//!
//! Today there's only one layout (`Standard`). The match arm is
//! one line; the value of routing through this dispatcher is
//! that adding a second layout means adding one match arm here
//! and one new file in `super::`, with zero churn in `app.rs` or
//! the eframe `update` body.

use eframe::egui;

use crate::app::{BarEditorApp, Layout};
use crate::panels;

/// Render the panels that compose the user's currently active
/// layout. Calls pre-frame work and the persistent shell chrome
/// (menu bar, status bar, action bar, floating windows) before
/// routing to the layout-specific panels.
pub fn draw_active(
    app: &mut BarEditorApp,
    ctx: &egui::Context,
    frame: &mut eframe::Frame,
) {
    app.pre_frame_work(ctx, frame);
    app.draw_shell(ctx, frame);

    if !app.has_project() {
        egui::CentralPanel::default().show(ctx, |ui| {
            panels::welcome::draw(app, ui);
        });
        return;
    }

    match app.active_layout() {
        Layout::Standard => super::standard::draw(app, ctx, frame),
        Layout::Sculpt3D => super::sculpt3d::draw(app, ctx, frame),
    }
}

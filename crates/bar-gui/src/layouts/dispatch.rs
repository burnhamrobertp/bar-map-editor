//! Layout dispatch -- picks the active layout and delegates to it.

use eframe::egui;

use crate::app::{BarEditorApp, Layout};
use crate::panels;

/// Render the panels that compose the user's currently active
/// layout. Calls pre-frame work and the persistent shell chrome
/// (menu bar, status bar, action bar, floating windows) before
/// routing to the layout-specific panels.
pub fn draw_active(app: &mut BarEditorApp, ctx: &egui::Context, frame: &mut eframe::Frame) {
    app.pre_frame_work(ctx, frame);
    app.draw_shell(ctx, frame);

    if !app.has_project() {
        egui::CentralPanel::default().show(ctx, |ui| {
            panels::welcome::draw(app, ui);
        });
        return;
    }

    match app.active_layout() {
        Layout::NodeGraph => super::node_graph::draw(app, ctx, frame),
        Layout::Sculpt3D => super::sculpt3d::draw(app, ctx, frame),
        Layout::Preview => super::preview::draw(app, ctx, frame),
    }
}

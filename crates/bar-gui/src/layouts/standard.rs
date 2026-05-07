//! Standard layout — node graph editor with left palette, central
//! canvas, plus all floating windows (inspector, map info, etc.).
//!
//! The shell (menu bar, status bar, action bar, modals) is drawn by
//! `dispatch::draw_active` before this function is called, so this
//! module only owns the layout-specific panels.

use eframe::egui;

use crate::app::BarEditorApp;

pub fn draw(
    app: &mut BarEditorApp,
    ctx: &egui::Context,
    _frame: &mut eframe::Frame,
) {
    app.draw_node_palette_panel(ctx);
    app.draw_standard_central_panel(ctx);
}

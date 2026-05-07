//! Standard layout — the only layout today, mirroring what the
//! editor has always shown: top toolbar, left palette, centre
//! canvas (with the contextual panels overlaid), bottom status
//! bar, plus floating windows for the inspector / map info /
//! validation details / file editor / settings / about.
//!
//! When the panel split lands, this module's `draw` will become
//! ~30 lines of "ask each panel to render itself" calls. Until
//! then the body still lives in `BarEditorApp::update_panels`
//! (the legacy path), and this module just delegates to it. The
//! delegation point is here from day one so future layouts have
//! a stable place to plug in without churning `app.rs`.

use eframe::egui;

use crate::app::BarEditorApp;

pub fn draw(
    app: &mut BarEditorApp,
    ctx: &egui::Context,
    frame: &mut eframe::Frame,
) {
    // Currently delegates to `BarEditorApp::update_panels`, which
    // owns the existing top-bar / palette / canvas / status-bar
    // composition until the panel split moves them out one by one.
    // Each panel migration replaces one section of that method
    // with a `panels::foo::draw(app, ui)` call here.
    app.update_panels(ctx, frame);
}

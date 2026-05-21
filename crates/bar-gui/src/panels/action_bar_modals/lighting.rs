//! Lighting modal -- sun direction + ground / unit material
//! parameters. Pure schema-driven.

use bar_project::recipe_fields::LIGHTING_SPECS;
use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::{modal_frame, render_specs, FieldFindings};

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_lighting_editor {
        return;
    }
    let mut open = app.dialog.show_lighting_editor;
    modal_frame(ctx, &mut open, "Lighting", "lighting_editor_modal", |ui| {
        let findings = FieldFindings::from(app.validation.findings());
        render_specs(ui, app, LIGHTING_SPECS, &findings);
    });
    app.dialog.show_lighting_editor = open;
}

//! Water modal -- underwater shading, surface tint, sun specular,
//! Fresnel reflection, wave normals. Pure schema-driven.

use bar_project::recipe_fields::WATER_SPECS;
use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::{modal_frame, render_specs, FieldFindings};

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_water_editor {
        return;
    }
    let mut open = app.dialog.show_water_editor;
    modal_frame(ctx, &mut open, "Water", "water_editor_modal", |ui| {
        let findings = FieldFindings::from(app.validation.findings());
        render_specs(ui, app, WATER_SPECS, &findings);
    });
    app.dialog.show_water_editor = open;
}

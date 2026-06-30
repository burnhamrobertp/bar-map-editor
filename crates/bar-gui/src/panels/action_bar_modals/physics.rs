//! Physics modal -- gameplay-tuning numerics that live on
//! `MapSettings` directly (gravity, hardness, tides, metal, etc.).
//! Pure schema-driven: every field renders through
//! `render_specs(PHYSICS_SPECS)`.

use bar_project::recipe_fields::PHYSICS_SPECS;
use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::{
    modal_frame, render_specs, settings_toolbar, FieldFindings,
};
use crate::t;

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_physics_editor {
        return;
    }
    let mut open = app.dialog.show_physics_editor;
    modal_frame(
        ctx,
        &mut open,
        &t!("editor.modals.physics.title"),
        "physics_editor_modal",
        |ui| {
            let findings = FieldFindings::from(app.validation.findings());
            let (query, advanced) = settings_toolbar(ui, "physics");
            render_specs(ui, app, PHYSICS_SPECS, &findings, &query, advanced);
        },
    );
    app.dialog.show_physics_editor = open;
}

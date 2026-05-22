//! Atmosphere modal -- wind and sky/cloud appearance.
//! Fog (distance fog + height fog + custom.clouds) has its own modal.
//! Sun colour moved to the Lighting modal with other sun properties.

use bar_project::recipe_fields::ATMOSPHERE_SPECS;
use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::{modal_frame, render_specs, FieldFindings};
use crate::panels::field_editor::section_heading;
use crate::panels::file_picker::FilePickerField;

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_atmosphere_editor {
        return;
    }
    let mut open = app.dialog.show_atmosphere_editor;
    modal_frame(
        ctx,
        &mut open,
        "Atmosphere",
        "atmosphere_editor_modal",
        |ui| {
            let findings = FieldFindings::from(app.validation.findings());
            render_specs(ui, app, ATMOSPHERE_SPECS, &findings);
            ui.add_space(12.0);
            draw_skybox(ui, app);
        },
    );
    app.dialog.show_atmosphere_editor = open;
}

fn draw_skybox(ui: &mut egui::Ui, app: &mut BarEditorApp) {
    section_heading(ui, "Skybox");
    let project_path_opt: Option<std::path::PathBuf> = app.project.path.clone();
    let parent_window = app.parent_window();
    let mut filename = app
        .map_settings()
        .atmosphere
        .skybox
        .clone()
        .unwrap_or_default();
    let changed = FilePickerField::new("Cubemap", "passthrough/maps")
        .extensions(&["dds"])
        .title("Select skybox DDS cubemap")
        .allow_clear(true)
        .hint("(empty = procedural sky)")
        .show(
            ui,
            &mut filename,
            project_path_opt.as_deref(),
            parent_window.as_ref(),
        );
    if changed {
        app.push_undo("Edit skybox");
        app.map_settings_mut().atmosphere.skybox = if filename.is_empty() {
            None
        } else {
            Some(filename)
        };
    }
}

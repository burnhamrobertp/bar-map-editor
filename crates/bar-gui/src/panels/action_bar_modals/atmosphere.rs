//! Atmosphere modal -- wind, fog, sky/cloud/sun colours, plus the
//! bespoke skybox file picker and `custom.fog` block (neither of
//! which fits the schema renderer cleanly).

use bar_project::recipe_fields::ATMOSPHERE_SPECS;
use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::{
    drive_drag_intent, modal_frame, render_specs, FieldFindings,
};
use crate::panels::field_editor::heading_with_info;

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
            ui.add_space(12.0);
            draw_height_fog(ui, app);
        },
    );
    app.dialog.show_atmosphere_editor = open;
}

/// Skybox: a DDS cubemap picked from disk and copied into the
/// project's `passthrough/maps/` directory. The schema's
/// `OptionText` kind would only get a plain TextEdit; this gives
/// the user a Browse... button + a Clear button alongside.
fn draw_skybox(ui: &mut egui::Ui, app: &mut BarEditorApp) {
    ui.heading("Skybox");
    let project_path_opt: Option<std::path::PathBuf> = app.project.path.clone();
    let mut browse_pick: Option<String> = None;
    let mut clear_clicked = false;
    ui.horizontal(|ui| {
        if ui.button("Browse for DDS cubemap...").clicked() {
            if let Some(picked) = rfd::FileDialog::new()
                .set_title("Select skybox DDS cubemap")
                .add_filter("DDS cubemap", &["dds"])
                .pick_file()
            {
                let filename = picked
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string());
                if let (Some(name), Some(project_dir)) = (filename, project_path_opt.as_deref()) {
                    let dst_dir = project_dir.join("passthrough").join("maps");
                    let dst = dst_dir.join(&name);
                    let copy_result = std::fs::create_dir_all(&dst_dir)
                        .and_then(|_| std::fs::copy(&picked, &dst));
                    match copy_result {
                        Ok(_) => browse_pick = Some(name),
                        Err(e) => tracing::warn!(err = %e, "Failed to copy skybox DDS"),
                    }
                } else if let Some(name) = picked
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
                {
                    browse_pick = Some(name);
                }
            }
        }
        let has_skybox = app
            .map_settings()
            .atmosphere
            .skybox
            .as_deref()
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        if has_skybox && ui.button("Clear").clicked() {
            clear_clicked = true;
        }
    });
    if let Some(name) = browse_pick {
        app.push_undo("Edit skybox");
        app.map_settings_mut().atmosphere.skybox = Some(name);
    }
    if clear_clicked {
        app.push_undo("Clear skybox");
        app.map_settings_mut().atmosphere.skybox = None;
    }
}

/// `custom.fog` block: BAR widget that tints fragments below a
/// ceiling height toward the fog colour. Not a `MapSettings` direct
/// field, so it stays bespoke -- the toggle, colour, ceiling, and
/// attenuation each commit independently.
fn draw_height_fog(ui: &mut egui::Ui, app: &mut BarEditorApp) {
    heading_with_info(
        ui,
        "Height fog",
        "Tints fragments below the ceiling toward the fog colour, \
         attenuated per elmo. Used for the cool underwater cast on \
         maps like Aurelia.",
    );

    let fog_current = app.map_settings().custom_fog.clone();
    let mut fog_enabled = fog_current.enabled;
    let mut fog_color = fog_current.color;
    let mut fog_height = fog_current.height_elmos;
    let mut fog_atten = fog_current.atten;

    let mut fog_enabled_changed = false;
    ui.horizontal(|ui| {
        if ui.checkbox(&mut fog_enabled, "Enabled").changed() {
            fog_enabled_changed = true;
        }
    });
    if fog_enabled_changed {
        app.push_undo("Toggle height fog");
        app.map_settings_mut().custom_fog.enabled = fog_enabled;
    }

    ui.add_enabled_ui(fog_enabled, |ui| {
        let mut color_changed = false;
        ui.horizontal(|ui| {
            ui.label("Colour");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut linear = bar_render::color::srgb_to_linear_rgb(fog_color);
                if ui.color_edit_button_rgb(&mut linear).changed() {
                    fog_color = bar_render::color::linear_to_srgb_rgb(linear);
                    color_changed = true;
                }
            });
        });
        if color_changed {
            app.push_undo("Edit fog colour");
            app.map_settings_mut().custom_fog.color = fog_color;
        }

        let mut height_resp = None;
        ui.horizontal(|ui| {
            ui.label("Ceiling height (elmos)");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                height_resp = Some(
                    ui.add(
                        egui::DragValue::new(&mut fog_height)
                            .range(-1024.0..=1024.0)
                            .speed(1.0),
                    ),
                );
            });
        });
        let height_resp = height_resp.expect("height response captured above");
        if height_resp.changed() {
            app.map_settings_mut().custom_fog.height_elmos = fog_height;
        }
        drive_drag_intent(app, &height_resp, "fog height");

        let mut atten_resp = None;
        ui.horizontal(|ui| {
            ui.label("Attenuation (per elmo)");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                atten_resp = Some(
                    ui.add(
                        egui::DragValue::new(&mut fog_atten)
                            .range(0.0..=1.0)
                            .speed(0.001),
                    ),
                );
            });
        });
        let atten_resp = atten_resp.expect("atten response captured above");
        if atten_resp.changed() {
            app.map_settings_mut().custom_fog.atten = fog_atten;
        }
        drive_drag_intent(app, &atten_resp, "fog atten");
    });
}

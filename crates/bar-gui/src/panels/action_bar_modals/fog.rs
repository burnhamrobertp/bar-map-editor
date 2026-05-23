//! Fog modal -- distance fog, height fog (custom.fog), and volumetric
//! clouds (custom.clouds). Distance fog is schema-driven via FOG_SPECS;
//! the other two sections are bespoke because they use non-MapSettings
//! types or need custom widget layout.

use bar_project::recipe_fields::{CLOUDS_SPECS, FOG_SPECS};
use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::{
    drive_drag_intent, modal_frame, render_specs, FieldFindings,
};
use crate::panels::field_editor::{heading_with_info, section_heading};
use crate::t;

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_fog_editor {
        return;
    }
    let mut open = app.dialog.show_fog_editor;
    modal_frame(
        ctx,
        &mut open,
        &t!("editor.modals.fog.title"),
        "fog_editor_modal",
        |ui| {
            let findings = FieldFindings::from(app.validation.findings());
            render_specs(ui, app, FOG_SPECS, &findings);
            ui.add_space(12.0);
            draw_height_fog(ui, app);
            ui.add_space(12.0);
            draw_clouds(ui, app, &findings);
        },
    );
    app.dialog.show_fog_editor = open;
}

/// `custom.fog` block: BAR widget that tints fragments below a ceiling
/// height toward the fog colour.
fn draw_height_fog(ui: &mut egui::Ui, app: &mut BarEditorApp) {
    heading_with_info(
        ui,
        &t!("editor.modals.fog.height_heading"),
        &t!("editor.modals.fog.height_info"),
    );

    let fog_current = app.map_settings().custom_fog.clone();
    let mut fog_enabled = fog_current.enabled;
    let mut fog_color = fog_current.color;
    let mut fog_height = fog_current.height_elmos;
    let mut fog_atten = fog_current.atten;

    let mut fog_enabled_changed = false;
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut fog_enabled, t!("common.enabled"))
            .changed()
        {
            fog_enabled_changed = true;
        }
    });
    if fog_enabled_changed {
        app.push_undo(&t!("editor.modals.fog.undo_toggle_height"));
        app.map_settings_mut().custom_fog.enabled = fog_enabled;
    }

    ui.add_enabled_ui(fog_enabled, |ui| {
        let mut color_changed = false;
        ui.horizontal(|ui| {
            ui.label(t!("common.colour"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut linear = bar_render::color::srgb_to_linear_rgb(fog_color);
                if ui.color_edit_button_rgb(&mut linear).changed() {
                    fog_color = bar_render::color::linear_to_srgb_rgb(linear);
                    color_changed = true;
                }
            });
        });
        if color_changed {
            app.push_undo(&t!("editor.modals.fog.undo_edit_colour"));
            app.map_settings_mut().custom_fog.color = fog_color;
        }

        let mut height_resp = None;
        ui.horizontal(|ui| {
            ui.label(t!("editor.modals.fog.field.ceiling_height"));
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
        drive_drag_intent(app, &height_resp, &t!("editor.modals.fog.undo_height"));

        let mut atten_resp = None;
        ui.horizontal(|ui| {
            ui.label(t!("editor.modals.fog.field.attenuation"));
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
        drive_drag_intent(app, &atten_resp, &t!("editor.modals.fog.undo_atten"));
    });
}

/// `custom.clouds` block: volumetric cloud layer widget.
fn draw_clouds(ui: &mut egui::Ui, app: &mut BarEditorApp, findings: &FieldFindings) {
    section_heading(ui, &t!("editor.modals.fog.clouds_heading"));
    render_specs(ui, app, CLOUDS_SPECS, findings);
}

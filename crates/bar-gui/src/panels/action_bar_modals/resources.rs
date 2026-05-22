//! Resources modal -- splat distribution / detail-normal texture
//! filenames, the four-channel per-channel sampling arrays, and the
//! legacy single-detail-texture path. Stays bespoke because the
//! `splat_tex_scales` / `splat_tex_mults` arrays are
//! `Option<[f32; 4]>` (not on the `FieldSpec` schema's allowed
//! shapes) and the texture filenames want plain-text editing with
//! atomic commit semantics.

use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::{
    drive_drag_intent, drive_text_edit_intent, modal_frame,
};
use crate::panels::field_editor::{heading_with_info, section_heading};

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_resources_editor {
        return;
    }
    let mut open = app.dialog.show_resources_editor;
    modal_frame(
        ctx,
        &mut open,
        "Resources",
        "resources_editor_modal",
        |ui| {
            heading_with_info(
                ui,
                "Splat distribution",
                "Four-channel mask that selects which of the four \
                 detail-normal textures contributes per pixel.",
            );
            text_field_atomic(ui, app, "splatDistrTex", |r| &mut r.splat_distr_tex);

            ui.add_space(8.0);
            section_heading(ui, "Splat detail-normal textures");
            text_field_atomic(ui, app, "splatDetailNormalTex1", |r| {
                &mut r.splat_detail_normal_tex_1
            });
            text_field_atomic(ui, app, "splatDetailNormalTex2", |r| {
                &mut r.splat_detail_normal_tex_2
            });
            text_field_atomic(ui, app, "splatDetailNormalTex3", |r| {
                &mut r.splat_detail_normal_tex_3
            });
            text_field_atomic(ui, app, "splatDetailNormalTex4", |r| {
                &mut r.splat_detail_normal_tex_4
            });
            let mut splat_alpha = app
                .map_settings()
                .resources
                .splat_detail_normal_diffuse_alpha;
            if ui
                .checkbox(&mut splat_alpha, "Diffuse alpha contribution")
                .changed()
            {
                app.push_undo("Toggle splat diffuse alpha");
                app.map_settings_mut()
                    .resources
                    .splat_detail_normal_diffuse_alpha = splat_alpha;
            }

            ui.add_space(8.0);
            heading_with_info(
                ui,
                "Per-channel sampling",
                "UV scale per splat channel (`splats.texScales`) and \
                 mix multiplier (`splats.texMults`).",
            );
            splat_array_atomic(
                ui,
                app,
                "texScales",
                |r| r.splat_tex_scales,
                |r, v| r.splat_tex_scales = Some(v),
                [1.0; 4],
                (0.0001, 1000.0),
            );
            splat_array_atomic(
                ui,
                app,
                "texMults",
                |r| r.splat_tex_mults,
                |r, v| r.splat_tex_mults = Some(v),
                [1.0; 4],
                (0.0, 1000.0),
            );

            ui.add_space(8.0);
            heading_with_info(
                ui,
                "Legacy detail texture",
                "Older single-tiling-texture path. Used by the renderer \
                 only when no splat distribution texture is set above.",
            );
            text_field_atomic(ui, app, "detailTex", |r| &mut r.detail_tex);

            ui.add_space(8.0);
            section_heading(ui, "Per-pixel masks");
            text_field_atomic(ui, app, "specularTex", |r| &mut r.specular_tex);
            text_field_atomic(ui, app, "skyReflectModTex", |r| &mut r.sky_reflect_mod_tex);
        },
    );
    app.dialog.show_resources_editor = open;
}

/// Single-line text field on `MapSettings.resources`. Snapshot on
/// focus-gain, push on focus-loss. Empty string is allowed (engine
/// treats it as "unset").
fn text_field_atomic(
    ui: &mut egui::Ui,
    app: &mut BarEditorApp,
    label: &str,
    field: fn(&mut bar_project::ResourcesSettings) -> &mut String,
) {
    let mut value = field(&mut app.map_settings_mut().resources).clone();
    let mut resp_taken = None;
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let edit = egui::TextEdit::singleline(&mut value)
                .desired_width(200.0)
                .hint_text("(empty = unset)");
            resp_taken = Some(ui.add(edit));
        });
    });
    let resp = resp_taken.expect("text field response captured above");
    if resp.changed() {
        *field(&mut app.map_settings_mut().resources) = value.clone();
    }
    drive_text_edit_intent(app, &resp, label, resp.changed());
}

/// Four-channel splat array (`splats.texScales` / `texMults`). Renders
/// four DragValue rows; touching any channel promotes the whole array
/// to `Some` with the seed defaults filled in for untouched channels.
fn splat_array_atomic(
    ui: &mut egui::Ui,
    app: &mut BarEditorApp,
    label_base: &str,
    get: fn(&bar_project::ResourcesSettings) -> Option<[f32; 4]>,
    set_some: fn(&mut bar_project::ResourcesSettings, [f32; 4]),
    seed: [f32; 4],
    range: (f32, f32),
) {
    let mut current = get(&app.map_settings().resources).unwrap_or(seed);
    let labels = ["R", "G", "B", "A"];
    let mut intents: Vec<egui::Response> = Vec::with_capacity(4);
    let mut changed_any = false;
    for (i, ch) in labels.iter().enumerate() {
        let mut resp_taken = None;
        ui.horizontal(|ui| {
            ui.label(format!("{label_base} {ch}"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let resp = ui.add(
                    egui::DragValue::new(&mut current[i])
                        .range(range.0..=range.1)
                        .speed((range.1 - range.0) / 1000.0),
                );
                resp_taken = Some(resp);
            });
        });
        let resp = resp_taken.expect("splat channel response captured above");
        if resp.changed() {
            changed_any = true;
        }
        intents.push(resp);
    }
    if changed_any {
        set_some(&mut app.map_settings_mut().resources, current);
    }
    for r in &intents {
        drive_drag_intent(app, r, label_base);
    }
}

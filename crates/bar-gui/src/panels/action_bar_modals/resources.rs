//! Resources modal -- splat distribution / detail-normal texture
//! filenames, the four-channel per-channel sampling arrays, and the
//! legacy single-detail-texture path. Stays bespoke because the
//! `splat_tex_scales` / `splat_tex_mults` arrays are
//! `Option<[f32; 4]>` (not on the `FieldSpec` schema's allowed
//! shapes) and the texture filenames want plain-text editing with
//! atomic commit semantics.

use bar_data::{generate_detail_normal, DetailNormalPreset};
use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::{
    drive_drag_intent, drive_text_edit_intent, modal_frame,
};
use crate::panels::field_editor::{heading_with_info, section_heading};
use crate::t;

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_resources_editor {
        return;
    }
    let mut open = app.dialog.show_resources_editor;
    modal_frame(
        ctx,
        &mut open,
        &t!("editor.modals.resources.title"),
        "resources_editor_modal",
        |ui| {
            detail_normal_section(app, ui);
            ui.add_space(8.0);

            heading_with_info(
                ui,
                &t!("editor.modals.resources.splat_distr_heading"),
                &t!("editor.modals.resources.splat_distr_info"),
            );
            text_field_atomic(ui, app, "splatDistrTex", |r| &mut r.splat_distr_tex);

            ui.add_space(8.0);
            section_heading(ui, &t!("editor.modals.resources.detail_normals_heading"));
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
                .checkbox(
                    &mut splat_alpha,
                    t!("editor.modals.resources.diffuse_alpha"),
                )
                .changed()
            {
                app.push_undo(&t!("editor.modals.resources.undo_diffuse_alpha"));
                app.map_settings_mut()
                    .resources
                    .splat_detail_normal_diffuse_alpha = splat_alpha;
            }

            ui.add_space(8.0);
            heading_with_info(
                ui,
                &t!("editor.modals.resources.per_channel_heading"),
                &t!("editor.modals.resources.per_channel_info"),
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
                &t!("editor.modals.resources.legacy_detail_heading"),
                &t!("editor.modals.resources.legacy_detail_info"),
            );
            text_field_atomic(ui, app, "detailTex", |r| &mut r.detail_tex);

            ui.add_space(8.0);
            section_heading(ui, &t!("editor.modals.resources.per_pixel_masks_heading"));
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
                .hint_text(t!("editor.modals.resources.field_hint_unset"));
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

/// Filename prefix for generated detail-normal presets. Lets us recognise our
/// own files (for selection highlight + cleanup) vs an imported texture.
const DN_PREFIX: &str = "detailnormal_";

/// `<project>/passthrough/` -- where resource textures live so the preview and
/// bundler resolve them (same convention as splat / minimap textures).
fn dn_passthrough_dir(app: &BarEditorApp) -> Option<std::path::PathBuf> {
    app.project.path.as_ref().map(|p| p.join("passthrough"))
}

/// Remove any previously-generated preset PNG so only one detail normal is
/// active at a time (keeps the bundle clean; an imported file is left alone).
fn dn_remove_generated(dir: &std::path::Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if let Some(name) = e.file_name().to_str() {
                if name.starts_with(DN_PREFIX) && name.ends_with(".png") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
}

fn dn_strength(app: &BarEditorApp) -> f32 {
    if app.dialog.detail_normal_strength <= 0.0 {
        1.0
    } else {
        app.dialog.detail_normal_strength
    }
}

fn dn_apply_preset(app: &mut BarEditorApp, preset: DetailNormalPreset) {
    let Some(dir) = dn_passthrough_dir(app) else {
        app.set_status("Save the project before adding surface detail");
        return;
    };
    let strength = dn_strength(app);
    let nm = generate_detail_normal(preset, 256, strength);
    if std::fs::create_dir_all(&dir).is_err() {
        app.set_status("Could not write the surface-detail texture");
        return;
    }
    dn_remove_generated(&dir);
    // Strength in the name so the preview's load cache reloads when it changes.
    let filename = format!(
        "{DN_PREFIX}{}_s{:02}.png",
        preset.label().to_lowercase(),
        (strength * 10.0).round() as i32
    );
    if nm.save_png(&dir.join(&filename)).is_err() {
        app.set_status("Could not write the surface-detail texture");
        return;
    }
    app.push_undo("Set surface detail");
    app.map_settings_mut().resources.detail_normal_tex = filename;
}

fn dn_import(app: &mut BarEditorApp) {
    let Some(dir) = dn_passthrough_dir(app) else {
        app.set_status("Save the project before adding surface detail");
        return;
    };
    let Some(src) = rfd::FileDialog::new()
        .add_filter("Image", &["png", "dds", "tga", "bmp", "jpg", "jpeg"])
        .pick_file()
    else {
        return;
    };
    let Some(basename) = src.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() || std::fs::copy(&src, dir.join(&basename)).is_err() {
        app.set_status("Could not import the surface-detail texture");
        return;
    }
    dn_remove_generated(&dir);
    app.push_undo("Import surface detail");
    app.map_settings_mut().resources.detail_normal_tex = basename;
}

fn dn_clear(app: &mut BarEditorApp) {
    if let Some(dir) = dn_passthrough_dir(app) {
        dn_remove_generated(&dir);
    }
    app.push_undo("Clear surface detail");
    app.map_settings_mut().resources.detail_normal_tex.clear();
}

/// Surface-detail (detailNormalTex) picker: pick a tiling rock/gravel/sand
/// bump or import one, adjust strength. Preview updates live; the file is
/// bundled with the map. Replaces hand-typing a normal-map filename.
fn detail_normal_section(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    section_heading(ui, "Surface detail");
    ui.label(
        egui::RichText::new(
            "Fine close-up bumpiness (rock grain, gravel) the heightmap is too coarse to show. \
             Tiles across the terrain; layered over the height-derived normals.",
        )
        .small()
        .weak(),
    );

    if app.dialog.detail_normal_strength <= 0.0 {
        app.dialog.detail_normal_strength = 1.0;
    }
    let current = app.map_settings().resources.detail_normal_tex.clone();
    let presets = [
        DetailNormalPreset::Rock,
        DetailNormalPreset::Gravel,
        DetailNormalPreset::Sand,
    ];
    let sel = |p: DetailNormalPreset| {
        current.starts_with(&format!("{DN_PREFIX}{}_", p.label().to_lowercase()))
    };
    let sel_none = current.is_empty();
    let is_import = !sel_none && !presets.iter().any(|&p| sel(p));

    enum Act {
        None,
        Preset(DetailNormalPreset),
        Import,
    }
    let mut act: Option<Act> = None;
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if ui.selectable_label(sel_none, "None").clicked() {
            act = Some(Act::None);
        }
        for p in presets {
            if ui.selectable_label(sel(p), p.label()).clicked() {
                act = Some(Act::Preset(p));
            }
        }
        if ui.selectable_label(is_import, "Import\u{2026}").clicked() {
            act = Some(Act::Import);
        }
    });

    let mut s = app.dialog.detail_normal_strength;
    let resp = ui.add(egui::Slider::new(&mut s, 0.2..=3.0).text("Strength"));
    if resp.changed() {
        app.dialog.detail_normal_strength = s;
    }
    let regen = resp.drag_stopped() || resp.lost_focus();

    match act {
        Some(Act::None) => dn_clear(app),
        Some(Act::Preset(p)) => dn_apply_preset(app, p),
        Some(Act::Import) => dn_import(app),
        None => {
            if regen {
                if let Some(p) = presets.into_iter().find(|&p| sel(p)) {
                    dn_apply_preset(app, p);
                }
            }
        }
    }
}

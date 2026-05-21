//! Identity modal -- `RecipeMeta` text fields (name / shortname /
//! description / author / version / tip), the `depend` list edited
//! as a comma-joined string, and `mapHardness` (which lives on
//! `MapSettings` but reads naturally alongside the identity fields).
//!
//! All bespoke: `RecipeMeta` isn't the `MapSettings` type the
//! schema's `FieldSpec` arrays are parameterised over, and the
//! multiline description / depend list don't fit a single-line
//! `FieldKind`.

use bar_project::engine_defaults as ed;
use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::{
    drive_drag_intent, drive_text_edit_intent, modal_frame,
};
use crate::t;

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_identity_editor {
        return;
    }
    let mut open = app.dialog.show_identity_editor;
    modal_frame(ctx, &mut open, "Identity", "identity_editor_modal", |ui| {
        draw_text_fields(ui, app);
        draw_description(ui, app);
        draw_depend(ui, app);
        draw_map_hardness(ui, app);
    });
    app.dialog.show_identity_editor = open;
}

fn draw_text_fields(ui: &mut egui::Ui, app: &mut BarEditorApp) {
    type MetaGet = fn(&crate::editor::RecipeMeta) -> String;
    type MetaSet = fn(&mut crate::editor::RecipeMeta, String);
    let text_fields: &[(&str, MetaGet, MetaSet, &str)] = &[
        (
            "name",
            |m| m.name.clone().unwrap_or_default(),
            |m, v| m.name = if v.is_empty() { None } else { Some(v) },
            "(map name)",
        ),
        (
            "shortname",
            |m| m.shortname.clone().unwrap_or_default(),
            |m, v| m.shortname = if v.is_empty() { None } else { Some(v) },
            "",
        ),
        (
            "author",
            |m| m.author.clone().unwrap_or_default(),
            |m, v| m.author = if v.is_empty() { None } else { Some(v) },
            "",
        ),
        (
            "version",
            |m| m.version.clone().unwrap_or_default(),
            |m, v| m.version = if v.is_empty() { None } else { Some(v) },
            "",
        ),
        (
            "tip",
            |m| m.tip.clone().unwrap_or_default(),
            |m, v| m.tip = if v.is_empty() { None } else { Some(v) },
            "",
        ),
    ];
    for (label, getter, setter, hint) in text_fields {
        let mut buf = getter(app.recipe_meta_mut());
        let mut resp_taken = None;
        ui.horizontal(|ui| {
            ui.label(*label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let edit = egui::TextEdit::singleline(&mut buf)
                    .desired_width(220.0)
                    .hint_text(*hint);
                let resp = ui.add(edit);
                crate::panels::widgets::select_all_on_focus(ui, &resp, &buf);
                resp_taken = Some(resp);
            });
        });
        let resp = resp_taken.expect("text field response captured above");
        let changed = resp.changed();
        if changed {
            setter(app.recipe_meta_mut(), buf.clone());
        }
        drive_text_edit_intent(app, &resp, label, changed);
    }
}

fn draw_description(ui: &mut egui::Ui, app: &mut BarEditorApp) {
    let mut desc = app.recipe_meta_mut().description.clone();
    let mut desc_resp = None;
    ui.horizontal(|ui| {
        ui.label(t!("common.description"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let edit = egui::TextEdit::multiline(&mut desc)
                .desired_width(220.0)
                .desired_rows(3);
            let resp = ui.add(edit);
            crate::panels::widgets::select_all_on_focus(ui, &resp, &desc);
            desc_resp = Some(resp);
        });
    });
    let desc_resp = desc_resp.expect("description response captured above");
    if desc_resp.changed() {
        app.recipe_meta_mut().description = desc.clone();
    }
    drive_text_edit_intent(app, &desc_resp, "description", desc_resp.changed());
}

fn draw_depend(ui: &mut egui::Ui, app: &mut BarEditorApp) {
    let mut depend_joined = app.recipe_meta_mut().depend.join(", ");
    let mut depend_resp = None;
    ui.horizontal(|ui| {
        ui.label("depend");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let edit = egui::TextEdit::singleline(&mut depend_joined)
                .desired_width(220.0)
                .hint_text("Map Helper v1");
            depend_resp = Some(ui.add(edit));
        });
    });
    let depend_resp = depend_resp.expect("depend response captured above");
    if depend_resp.changed() {
        app.recipe_meta_mut().depend = depend_joined
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    drive_text_edit_intent(app, &depend_resp, "depend", depend_resp.changed());
}

fn draw_map_hardness(ui: &mut egui::Ui, app: &mut BarEditorApp) {
    let mut hardness = app.map_settings().map_hardness.unwrap_or(ed::MAP_HARDNESS);
    let mut hardness_resp = None;
    ui.horizontal(|ui| {
        ui.label("mapHardness");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let resp = ui.add(egui::DragValue::new(&mut hardness).range(1u32..=65535u32));
            hardness_resp = Some(resp);
        });
    });
    let hardness_resp = hardness_resp.expect("hardness response captured above");
    if hardness_resp.changed() {
        app.map_settings_mut().map_hardness = Some(hardness);
    }
    drive_drag_intent(app, &hardness_resp, "mapHardness");
}

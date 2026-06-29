//! Identity modal -- `RecipeMeta` text fields (name / shortname /
//! description / author / version / tip), the `depend` list edited
//! as a comma-joined string, and `mapHardness` (which lives on
//! `MapSettings` but reads naturally alongside the identity fields).
//!
//! All bespoke: `RecipeMeta` isn't the `MapSettings` type the
//! schema's `FieldSpec` arrays are parameterised over, and the
//! multiline description / depend list don't fit a single-line
//! `FieldKind`.

use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::{drive_text_edit_intent, modal_frame};
use crate::t;

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_identity_editor {
        return;
    }
    let mut open = app.dialog.show_identity_editor;
    modal_frame(
        ctx,
        &mut open,
        &t!("editor.modals.identity.title"),
        "identity_editor_modal",
        |ui| {
            egui::Grid::new("identity_fields")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    draw_text_fields(ui, app);
                    draw_description(ui, app);
                    draw_depend(ui, app);
                });
        },
    );
    app.dialog.show_identity_editor = open;
}

fn draw_text_fields(ui: &mut egui::Ui, app: &mut BarEditorApp) {
    type MetaGet = fn(&crate::editor::RecipeMeta) -> String;
    type MetaSet = fn(&mut crate::editor::RecipeMeta, String);
    // Field labels + hints are i18n keys; resolved at the use site
    // so the static array stays small.
    let text_fields: &[(&str, MetaGet, MetaSet, &str)] = &[
        (
            "editor.modals.identity.field.name",
            |m| m.name.clone().unwrap_or_default(),
            |m, v| m.name = if v.is_empty() { None } else { Some(v) },
            "editor.modals.identity.field.name_hint",
        ),
        (
            "editor.modals.identity.field.shortname",
            |m| m.shortname.clone().unwrap_or_default(),
            |m, v| m.shortname = if v.is_empty() { None } else { Some(v) },
            "",
        ),
        (
            "editor.modals.identity.field.author",
            |m| m.author.clone().unwrap_or_default(),
            |m, v| m.author = if v.is_empty() { None } else { Some(v) },
            "",
        ),
        (
            "editor.modals.identity.field.version",
            |m| m.version.clone().unwrap_or_default(),
            |m, v| m.version = if v.is_empty() { None } else { Some(v) },
            "",
        ),
        (
            "editor.modals.identity.field.tip",
            |m| m.tip.clone().unwrap_or_default(),
            |m, v| m.tip = if v.is_empty() { None } else { Some(v) },
            "",
        ),
    ];
    for (label_key, getter, setter, hint_key) in text_fields {
        let label = t!(label_key);
        let hint = if hint_key.is_empty() {
            String::new()
        } else {
            t!(hint_key)
        };
        let mut buf = getter(app.recipe_meta_mut());
        ui.label(&label);
        let edit = egui::TextEdit::singleline(&mut buf)
            .desired_width(320.0)
            .hint_text(&hint);
        let resp = ui.add(edit);
        crate::panels::widgets::select_all_on_focus(ui, &resp, &buf);
        ui.end_row();
        let changed = resp.changed();
        if changed {
            setter(app.recipe_meta_mut(), buf.clone());
        }
        drive_text_edit_intent(app, &resp, &label, changed);
    }
}

fn draw_description(ui: &mut egui::Ui, app: &mut BarEditorApp) {
    let mut desc = app.recipe_meta_mut().description.clone();
    ui.label(t!("common.description"));
    let edit = egui::TextEdit::multiline(&mut desc)
        .desired_width(320.0)
        .desired_rows(3);
    let desc_resp = ui.add(edit);
    crate::panels::widgets::select_all_on_focus(ui, &desc_resp, &desc);
    ui.end_row();
    if desc_resp.changed() {
        app.recipe_meta_mut().description = desc.clone();
    }
    drive_text_edit_intent(
        app,
        &desc_resp,
        &t!("common.description"),
        desc_resp.changed(),
    );
}

fn draw_depend(ui: &mut egui::Ui, app: &mut BarEditorApp) {
    let mut depend_joined = app.recipe_meta_mut().depend.join(", ");
    let depend_label = t!("editor.modals.identity.field.depend");
    ui.label(&depend_label);
    let depend_resp = ui.add(
        egui::TextEdit::singleline(&mut depend_joined)
            .desired_width(320.0)
            .hint_text(t!("editor.modals.identity.field.depend_hint")),
    );
    ui.end_row();
    if depend_resp.changed() {
        app.recipe_meta_mut().depend = depend_joined
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    drive_text_edit_intent(
        app,
        &depend_resp,
        &t!("editor.modals.identity.field.depend"),
        depend_resp.changed(),
    );
}

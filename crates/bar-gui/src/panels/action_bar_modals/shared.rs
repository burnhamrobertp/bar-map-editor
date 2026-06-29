//! Shared infrastructure for the action-bar modals.
//!
//! Every modal in this `modals/` subtree opens the same kind of egui
//! `Window` (scrollable, resizable, with a stable id), iterates over
//! its `FieldSpec` slice via the schema-driven renderer in
//! `field_editor`, and routes validation findings back to the
//! widgets through `FieldFindings`. Pulling those into one place
//! keeps each per-modal file tiny and focused on its own bespoke
//! pieces (file pickers, multiline text, splat arrays, etc.).

use bar_project::field_schema::{FieldKind, FieldSpec};
use bar_project::recipe::MapSettings;
use eframe::egui;
use std::collections::HashMap;

use crate::app::BarEditorApp;
use crate::panels::field_editor::{process_intent, render_field, scrollbar_clearance, FieldIntent};

/// Validation findings keyed by `(category, field_id)` so the modal
/// renderer can decorate each widget with the worst-severity finding
/// touching that field. Built once per draw frame from
/// `app.validation.findings()`.
pub(crate) struct FieldFindings {
    by_field: HashMap<(String, String), bar_project::Severity>,
}

impl FieldFindings {
    pub(crate) fn from(findings: &[bar_project::Finding]) -> Self {
        let mut by_field: HashMap<(String, String), bar_project::Severity> = HashMap::new();
        for f in findings {
            if let Some(field) = f.field.as_deref() {
                by_field
                    .entry((f.category.clone(), field.to_string()))
                    .and_modify(|s| *s = worst_severity(*s, f.severity))
                    .or_insert(f.severity);
            }
        }
        Self { by_field }
    }

    pub(crate) fn field(&self, tab: &str, field: &str) -> Option<bar_project::Severity> {
        self.by_field
            .get(&(tab.to_string(), field.to_string()))
            .copied()
    }
}

fn worst_severity(a: bar_project::Severity, b: bar_project::Severity) -> bar_project::Severity {
    use bar_project::Severity::*;
    match (a, b) {
        (Error, _) | (_, Error) => Error,
        (Warning, _) | (_, Warning) => Warning,
        _ => Info,
    }
}

/// Settings search + "Advanced" toggle that sits at the top of a
/// settings modal. Query text and the advanced flag persist in egui
/// memory keyed by `id` so each modal keeps its own. Returns the
/// lowercased query and the advanced flag for [`render_specs`] to
/// filter on. Call once, as the first thing in the modal body, so the
/// search is consistently in the same place across panels.
pub(crate) fn settings_toolbar(ui: &mut egui::Ui, id: &str) -> (String, bool) {
    let q_id = egui::Id::new(("settings_search", id));
    let a_id = egui::Id::new(("settings_advanced", id));
    let mut query: String = ui.data(|d| d.get_temp(q_id)).unwrap_or_default();
    let mut advanced: bool = ui.data(|d| d.get_temp(a_id)).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.toggle_value(&mut advanced, "Advanced")
                .on_hover_text("Show every engine field, not just the commonly-tuned ones");
            ui.add(
                egui::TextEdit::singleline(&mut query)
                    .hint_text("Search settings")
                    .desired_width(f32::INFINITY),
            );
        });
    });
    ui.data_mut(|d| {
        d.insert_temp(q_id, query.clone());
        d.insert_temp(a_id, advanced);
    });
    ui.add_space(4.0);
    (query.trim().to_lowercase(), advanced)
}

/// Walk a schema slice, call `render_field` for each visible spec, and
/// fan the returned intent through `process_intent`. A field shows when
/// it matches `query` (while searching) or otherwise when it is Common
/// or `advanced` is on -- advanced fields appear inline in their own
/// `group` section, not a separate region. Inserts a sub-section
/// heading on each `group` transition among shown fields. Skips
/// `PassthroughTexture` (bespoke pickers live upstream).
pub(crate) fn render_specs(
    ui: &mut egui::Ui,
    app: &mut BarEditorApp,
    specs: &[FieldSpec<MapSettings>],
    findings: &FieldFindings,
    query: &str,
    advanced: bool,
) {
    let mut intents: Vec<(&'static str, FieldIntent)> = Vec::new();
    {
        let settings = app.map_settings_mut();
        let mut last_group: &str = "";
        for spec in specs {
            if matches!(spec.kind, FieldKind::PassthroughTexture { .. }) {
                continue;
            }
            let show = if query.is_empty() {
                advanced || bar_project::recipe_fields::is_common(spec.id)
            } else {
                spec.label.to_lowercase().contains(query)
            };
            if !show {
                continue;
            }
            if !spec.group.is_empty() && spec.group != last_group {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(spec.group).strong());
                ui.add_space(2.0);
            }
            last_group = spec.group;
            let severity = findings.field(spec.category, spec.id);
            let intent = render_field(ui, spec, settings, severity);
            intents.push((spec.label, intent));
        }
    }
    for (label, intent) in intents {
        process_intent(app, label, intent);
    }
}

/// Atomic-commit wrapper for a text-edit response: snapshots on
/// focus-gain into the dialog's single-slot pending-edit, pushes on
/// focus-loss. Use this for bespoke text inputs that can't go
/// through the schema renderer (multiline description, dimension
/// quantisation, comma-joined depend list, etc.).
pub(crate) fn drive_text_edit_intent(
    app: &mut BarEditorApp,
    resp: &egui::Response,
    label: &str,
    _changed: bool,
) {
    if resp.gained_focus() && app.dialog.field_edit_in_progress.is_none() {
        let snap = app.snapshot(&format!("Edit {}", label));
        app.dialog.field_edit_in_progress = Some(snap);
    }
    if resp.lost_focus() {
        if let Some(snap) = app.dialog.field_edit_in_progress.take() {
            app.history.push(snap);
        }
        app.mark_dirty();
    }
}

/// Atomic-commit wrapper for a numeric DragValue response: snapshot
/// on drag-start / focus-gain, push on drag-stop / focus-loss.
pub(crate) fn drive_drag_intent(app: &mut BarEditorApp, resp: &egui::Response, label: &str) {
    if (resp.drag_started() || resp.gained_focus()) && app.dialog.field_edit_in_progress.is_none() {
        let snap = app.snapshot(&format!("Edit {}", label));
        app.dialog.field_edit_in_progress = Some(snap);
    }
    if resp.drag_stopped() || resp.lost_focus() {
        if let Some(snap) = app.dialog.field_edit_in_progress.take() {
            app.history.push(snap);
        }
        app.mark_dirty();
    }
}

/// Shared boilerplate every action-bar modal uses: an egui `Window`
/// with a stable id (so position / size persist across re-opens), a
/// vertical `ScrollArea`, and a right-side `scrollbar_clearance` so
/// the right-aligned input widgets never sit under the scrollbar.
pub(crate) fn modal_frame(
    ctx: &egui::Context,
    open: &mut bool,
    title: &str,
    id_source: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    egui::Window::new(title)
        .id(egui::Id::new(id_source))
        .open(open)
        .resizable(true)
        .collapsible(false)
        .default_size([560.0, 560.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    scrollbar_clearance(ui, body);
                });
        });
}

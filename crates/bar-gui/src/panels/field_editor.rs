//! Generic schema-driven field renderer for the map-settings modals.
//!
//! One `render_field` function dispatches on [`FieldKind`] and draws
//! the right egui widget for each kind. Replaces every per-modal
//! helper (`drag_f32_opt`, `opt_color_row`, `checkbox_opt`,
//! `drag_row`, etc.) and gives the four modals one consistent UX +
//! commit + undo story.
//!
//! ## Atomic-commit + undo model
//!
//! Each widget owns its commit timing through egui's
//! [`Response`](egui::Response) signals:
//!
//! | Kind                | "Edit started"     | "Edit committed"    |
//! |---------------------|--------------------|---------------------|
//! | F32 / U32 DragValue | `drag_started()`   | `drag_stopped()`    |
//! | Same, click-to-type | `gained_focus()`   | `lost_focus()`      |
//! | Text / OptionText   | `gained_focus()`   | `lost_focus()`      |
//! | Bool combo / Color  | (atomic)           | `changed()`         |
//! | PassthroughTexture  | (atomic)           | (after file picker) |
//!
//! The transitions drive [`FieldIntent`] returned from `render_field`,
//! which [`process_intent`] turns into a single `app.push_undo(label)`
//! per atomic commit. A long drag still produces exactly one entry
//! because the snapshot is taken at drag start and only pushed at
//! drag stop (and only if the value actually moved).
//!
//! ## Tight binding (no displayed-vs-stored desync)
//!
//! For numeric widgets, the user-visible value is `(spec.get)(state)`
//! evaluated every frame -- there is no shadow buffer that can drift
//! from the recipe. For text inputs, egui owns the in-progress edit
//! buffer per its standard widget contract; the moment focus leaves,
//! the buffer is parsed (and hard-clamped) and `(spec.set)` writes
//! the result back, so the displayed text and the recipe converge
//! every time the widget is idle.
//!
//! ## Findings decoration
//!
//! The optional `finding_severity` parameter wraps the field row in a
//! coloured outline matching the validation severity (red for
//! `Error`, yellow for `Warning`, blue for `Info`). Matches the
//! existing `outline_finding` behaviour in `mapinfo_editor.rs` so
//! visual styling stays consistent across the editor.

use bar_project::field_schema::{categories, FieldKind, FieldSpec, FieldValue};
use bar_project::Severity;
use eframe::egui;

use crate::app::BarEditorApp;

/// What happened to a field this frame. Returned by `render_field` so
/// the caller can drive the undo + dirty-flag side-effects without
/// the renderer needing `&mut BarEditorApp` (which would conflict
/// with the `&mut state` borrow when state is reached through app).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldIntent {
    /// Nothing happened this frame.
    None,
    /// User just began an edit session (drag started, gained focus).
    /// Caller should snapshot if a session isn't already in flight.
    EditStarted,
    /// User just committed an in-flight edit (drag stopped, lost
    /// focus). Caller should push the active snapshot.
    EditCommitted,
    /// Single-frame discrete commit (combo selection, checkbox
    /// toggle, browse-to-pick). Caller snapshots + pushes
    /// atomically.
    EditAtomic,
}

/// Wire a [`FieldIntent`] returned from `render_field` into the app's
/// undo / dirty machinery. Single source of truth for "what happens
/// on a field event" so each modal stays a simple iteration over its
/// schema.
pub fn process_intent(app: &mut BarEditorApp, label: &str, intent: FieldIntent) {
    match intent {
        FieldIntent::None => {}
        FieldIntent::EditStarted => {
            if app.dialog.field_edit_in_progress.is_none() {
                let snap = app.snapshot(&format!("Edit {}", label));
                app.dialog.field_edit_in_progress = Some(snap);
            }
        }
        FieldIntent::EditCommitted => {
            if let Some(snap) = app.dialog.field_edit_in_progress.take() {
                app.history.push(snap);
            }
            app.mark_dirty();
        }
        FieldIntent::EditAtomic => {
            let snap = app.snapshot(&format!("Edit {}", label));
            app.history.push(snap);
            app.mark_dirty();
        }
    }
}

/// Outline the field row matching the validation severity. Mirrors
/// `mapinfo_editor::outline_finding` so the visual language across
/// modals stays identical even though that function isn't reused
/// directly (this module needs a slightly different API shape).
fn outline_severity<R>(
    ui: &mut egui::Ui,
    severity: Option<Severity>,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let Some(sev) = severity else {
        return body(ui);
    };
    let colour = match sev {
        Severity::Error => egui::Color32::from_rgb(220, 60, 60),
        Severity::Warning => egui::Color32::from_rgb(220, 180, 60),
        Severity::Info => egui::Color32::from_rgb(80, 140, 220),
    };
    egui::Frame::group(ui.style())
        .stroke(egui::Stroke::new(1.5, colour))
        .corner_radius(2.0)
        .inner_margin(egui::Margin::symmetric(2, 1))
        .show(ui, body)
        .inner
}

/// Draw the label + optional info-icon tooltip target.
fn draw_label(ui: &mut egui::Ui, spec_label: &str, description: Option<&str>) {
    ui.label(spec_label);
    if let Some(desc) = description {
        info_icon(ui, desc);
    }
}

/// Allocate a small info icon (the blocky serif `i`) and attach
/// `tooltip` as hover text. Use this anywhere a paragraph of
/// explanatory copy would otherwise sit next to a heading or field --
/// the icon is the single visual idiom for "extra context lives
/// here, hover to read it." Matches the pattern next to "Feature
/// Lights" in the feature popover.
pub fn info_icon(ui: &mut egui::Ui, tooltip: &str) {
    let size = egui::vec2(11.0, 11.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    let color = if resp.hovered() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    crate::panels::icons::paint_info_icon(ui.painter(), rect, color);
    resp.on_hover_text(tooltip);
}

/// Draw a section heading row with an info icon at the right
/// carrying `tooltip` as hover text. Replaces the
/// `heading + paragraph` pattern that used to clutter the modals.
pub fn heading_with_info(ui: &mut egui::Ui, heading: &str, tooltip: &str) {
    ui.horizontal(|ui| {
        ui.heading(heading);
        info_icon(ui, tooltip);
    });
}

/// Wrap a `ScrollArea` body so it always leaves room on the right
/// for a vertical scrollbar. Egui draws the scrollbar overlaid on
/// the content rect, so without this padding the right-aligned
/// input widgets (DragValue, color swatch) end up underneath the
/// scrollbar handle whenever the contents overflow.
pub fn scrollbar_clearance<R>(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::default()
        .inner_margin(egui::Margin {
            right: 14,
            ..Default::default()
        })
        .show(ui, body)
        .inner
}

/// Render one schema field. The state type `S` is whichever struct
/// `spec` describes (usually `bar_project::MapSettings`). Caller
/// supplies the severity if any (read from `FieldFindings`); the
/// renderer paints the outline.
pub fn render_field<S>(
    ui: &mut egui::Ui,
    spec: &FieldSpec<S>,
    state: &mut S,
    finding_severity: Option<Severity>,
) -> FieldIntent
where
    S: 'static,
{
    outline_severity(ui, finding_severity, |ui| match &spec.kind {
        FieldKind::F32 {
            hard,
            soft: _,
            unit,
        } => render_f32_opt(ui, spec, state, *hard, unit),
        FieldKind::U32 {
            hard,
            soft: _,
            unit,
        } => render_u32_opt(ui, spec, state, *hard, unit),
        FieldKind::Bool => render_bool_opt(ui, spec, state),
        FieldKind::Color => render_color_opt(ui, spec, state),
        FieldKind::Vec3 { hard, soft: _ } => render_vec_opt::<S, 3>(ui, spec, state, *hard),
        FieldKind::Vec4 { hard, soft: _ } => render_vec_opt::<S, 4>(ui, spec, state, *hard),
        FieldKind::Text { max_len } => render_text(ui, spec, state, *max_len),
        FieldKind::OptionText { max_len } => render_option_text(ui, spec, state, *max_len),
        FieldKind::PassthroughTexture { extensions } => {
            render_passthrough_texture(ui, spec, state, extensions)
        }
    })
}

// ──────────────────────────────────────────────────────────────────
// Per-kind renderers
// ──────────────────────────────────────────────────────────────────

/// Read the engine default for kind that returns an f32-shaped value,
/// or 0.0 if the spec carries something unexpected.
fn default_f32<S>(spec: &FieldSpec<S>) -> f32 {
    use bar_project::field_schema::DefaultValue;
    match spec.default {
        DefaultValue::F32(v) => v,
        _ => 0.0,
    }
}

fn default_u32<S>(spec: &FieldSpec<S>) -> u32 {
    use bar_project::field_schema::DefaultValue;
    match spec.default {
        DefaultValue::U32(v) => v,
        _ => 0,
    }
}

fn default_color<S>(spec: &FieldSpec<S>) -> [f32; 3] {
    use bar_project::field_schema::DefaultValue;
    match spec.default {
        DefaultValue::Color(v) | DefaultValue::Vec3(v) => v,
        _ => [0.5, 0.5, 0.5],
    }
}

fn default_vec3<S>(spec: &FieldSpec<S>) -> [f32; 3] {
    use bar_project::field_schema::DefaultValue;
    match spec.default {
        DefaultValue::Vec3(v) | DefaultValue::Color(v) => v,
        _ => [0.0; 3],
    }
}

fn default_vec4<S>(spec: &FieldSpec<S>) -> [f32; 4] {
    use bar_project::field_schema::DefaultValue;
    match spec.default {
        DefaultValue::Vec4(v) => v,
        _ => [0.0; 4],
    }
}

fn default_bool<S>(spec: &FieldSpec<S>) -> bool {
    use bar_project::field_schema::DefaultValue;
    match spec.default {
        DefaultValue::Bool(v) => v,
        _ => false,
    }
}

fn render_f32_opt<S>(
    ui: &mut egui::Ui,
    spec: &FieldSpec<S>,
    state: &mut S,
    hard: (f32, f32),
    unit: &str,
) -> FieldIntent
where
    S: 'static,
{
    let current = match (spec.get)(state) {
        FieldValue::F32(v) => v,
        _ => return FieldIntent::None,
    };
    let default = default_f32(spec);
    let is_unset = current.is_none();
    let displayed = current.unwrap_or(default);
    let mut value = displayed;
    let mut intent = FieldIntent::None;

    ui.horizontal(|ui| {
        draw_label(ui, spec.label, spec.description);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !unit.is_empty() {
                ui.label(egui::RichText::new(unit).weak());
            }
            // Always-editable DragValue. When the recipe value is
            // None, the displayed number is the engine default and a
            // small "default" hint sits to its left; the first
            // user-driven change writes through `commit` which
            // promotes the recipe to `Some(value)`. Right-click reverts
            // the field back to `None` (fall through to engine
            // default).
            let resp = ui.add(
                egui::DragValue::new(&mut value)
                    .range(hard.0..=hard.1)
                    .speed((hard.1 - hard.0) / 1000.0),
            );
            if resp.drag_started() || resp.gained_focus() {
                intent = FieldIntent::EditStarted;
            }
            if (value - displayed).abs() > f32::EPSILON {
                spec.commit(state, FieldValue::F32(Some(value)));
            }
            if resp.drag_stopped() || resp.lost_focus() {
                intent = FieldIntent::EditCommitted;
            }
            if resp.secondary_clicked() {
                spec.commit(state, FieldValue::F32(None));
                intent = FieldIntent::EditAtomic;
            }
            if is_unset {
                ui.label(egui::RichText::new("default").weak().italics().small())
                    .on_hover_text("Engine default. Edit to override.");
            }
        });
    });
    intent
}

fn render_u32_opt<S>(
    ui: &mut egui::Ui,
    spec: &FieldSpec<S>,
    state: &mut S,
    hard: (u32, u32),
    unit: &str,
) -> FieldIntent
where
    S: 'static,
{
    let current = match (spec.get)(state) {
        FieldValue::U32(v) => v,
        _ => return FieldIntent::None,
    };
    let default = default_u32(spec);
    let is_unset = current.is_none();
    let displayed = current.unwrap_or(default);
    let mut value = displayed;
    let mut intent = FieldIntent::None;

    ui.horizontal(|ui| {
        draw_label(ui, spec.label, spec.description);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !unit.is_empty() {
                ui.label(egui::RichText::new(unit).weak());
            }
            let resp = ui.add(egui::DragValue::new(&mut value).range(hard.0..=hard.1));
            if resp.drag_started() || resp.gained_focus() {
                intent = FieldIntent::EditStarted;
            }
            if value != displayed {
                spec.commit(state, FieldValue::U32(Some(value)));
            }
            if resp.drag_stopped() || resp.lost_focus() {
                intent = FieldIntent::EditCommitted;
            }
            if resp.secondary_clicked() {
                spec.commit(state, FieldValue::U32(None));
                intent = FieldIntent::EditAtomic;
            }
            if is_unset {
                ui.label(egui::RichText::new("default").weak().italics().small())
                    .on_hover_text("Engine default. Edit to override.");
            }
        });
    });
    intent
}

fn render_bool_opt<S>(ui: &mut egui::Ui, spec: &FieldSpec<S>, state: &mut S) -> FieldIntent
where
    S: 'static,
{
    let current = match (spec.get)(state) {
        FieldValue::Bool(v) => v,
        _ => return FieldIntent::None,
    };
    let default = default_bool(spec);
    let mut intent = FieldIntent::None;

    ui.horizontal(|ui| {
        draw_label(ui, spec.label, spec.description);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let display = match current {
                Some(true) => egui::RichText::new("true"),
                Some(false) => egui::RichText::new("false"),
                None => egui::RichText::new(default.to_string()).weak().italics(),
            };
            let mut local = current;
            egui::ComboBox::from_id_salt(("field_bool", spec.id))
                .selected_text(display)
                .width(120.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_value(
                            &mut local,
                            None,
                            egui::RichText::new(default.to_string()).weak().italics(),
                        )
                        .clicked()
                    {
                        spec.commit(state, FieldValue::Bool(None));
                        intent = FieldIntent::EditAtomic;
                    }
                    if ui
                        .selectable_value(&mut local, Some(true), "true")
                        .clicked()
                    {
                        spec.commit(state, FieldValue::Bool(Some(true)));
                        intent = FieldIntent::EditAtomic;
                    }
                    if ui
                        .selectable_value(&mut local, Some(false), "false")
                        .clicked()
                    {
                        spec.commit(state, FieldValue::Bool(Some(false)));
                        intent = FieldIntent::EditAtomic;
                    }
                });
        });
    });
    intent
}

fn render_color_opt<S>(ui: &mut egui::Ui, spec: &FieldSpec<S>, state: &mut S) -> FieldIntent
where
    S: 'static,
{
    let current = match (spec.get)(state) {
        FieldValue::Color(v) => v,
        _ => return FieldIntent::None,
    };
    let default = default_color(spec);
    let is_unset = current.is_none();
    let displayed = current.unwrap_or(default);
    let mut intent = FieldIntent::None;

    ui.horizontal(|ui| {
        draw_label(ui, spec.label, spec.description);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Always-editable color swatch. When the recipe value is
            // None, the swatch shows the engine default and the first
            // change promotes the recipe to `Some(value)`. Right-click
            // reverts to None.
            let mut linear = bar_render::color::srgb_to_linear_rgb(displayed);
            let resp = ui.color_edit_button_rgb(&mut linear);
            if resp.changed() {
                let srgb = bar_render::color::linear_to_srgb_rgb(linear);
                spec.commit(state, FieldValue::Color(Some(srgb)));
                intent = FieldIntent::EditAtomic;
            }
            if resp.secondary_clicked() {
                spec.commit(state, FieldValue::Color(None));
                intent = FieldIntent::EditAtomic;
            }
            if is_unset {
                ui.label(egui::RichText::new("default").weak().italics().small())
                    .on_hover_text("Engine default. Edit to override.");
            }
        });
    });
    intent
}

fn render_vec_opt<S, const N: usize>(
    ui: &mut egui::Ui,
    spec: &FieldSpec<S>,
    state: &mut S,
    hard: (f32, f32),
) -> FieldIntent
where
    S: 'static,
{
    // Pulls the current array out of FieldValue::Vec3/Vec4 depending
    // on N. Renders N drag values in a horizontal strip with one
    // Override toggle in front. Commit semantics match F32: any
    // channel drag-started flips intent to EditStarted; any
    // channel-changed writes back; any channel drag-stopped /
    // lost-focus flips to EditCommitted.
    let (current_array, default_array): (Option<[f32; N]>, [f32; N]) = match N {
        3 => match (spec.get)(state) {
            FieldValue::Vec3(v) => (
                v.map(|a| {
                    let mut out = [0.0; N];
                    out[..3].copy_from_slice(&a);
                    out
                }),
                {
                    let d = default_vec3(spec);
                    let mut out = [0.0; N];
                    out[..3].copy_from_slice(&d);
                    out
                },
            ),
            _ => return FieldIntent::None,
        },
        4 => match (spec.get)(state) {
            FieldValue::Vec4(v) => (
                v.map(|a| {
                    let mut out = [0.0; N];
                    out[..4].copy_from_slice(&a);
                    out
                }),
                {
                    let d = default_vec4(spec);
                    let mut out = [0.0; N];
                    out[..4].copy_from_slice(&d);
                    out
                },
            ),
            _ => return FieldIntent::None,
        },
        _ => return FieldIntent::None,
    };

    let is_unset = current_array.is_none();
    let mut arr = current_array.unwrap_or(default_array);
    let mut intent = FieldIntent::None;
    let mut any_changed = false;
    let mut started = false;
    let mut stopped = false;
    let mut secondary = false;

    ui.horizontal(|ui| {
        draw_label(ui, spec.label, spec.description);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Always-editable channel drags. When the recipe value is
            // None, displayed numbers are the engine default and the
            // first change promotes the field. Right-click any
            // channel reverts the whole vector to None.
            for slot in arr.iter_mut().take(N) {
                let resp = ui.add(
                    egui::DragValue::new(slot)
                        .range(hard.0..=hard.1)
                        .speed((hard.1 - hard.0) / 1000.0),
                );
                if resp.drag_started() || resp.gained_focus() {
                    started = true;
                }
                if resp.drag_stopped() || resp.lost_focus() {
                    stopped = true;
                }
                if resp.changed() {
                    any_changed = true;
                }
                if resp.secondary_clicked() {
                    secondary = true;
                }
            }
            if is_unset {
                ui.label(egui::RichText::new("default").weak().italics().small())
                    .on_hover_text("Engine default. Edit to override.");
            }
        });
    });

    if any_changed {
        let new_value = if N == 3 {
            FieldValue::Vec3(Some([arr[0], arr[1], arr[2]]))
        } else {
            FieldValue::Vec4(Some([arr[0], arr[1], arr[2], arr[3]]))
        };
        spec.commit(state, new_value);
    }
    if started {
        intent = FieldIntent::EditStarted;
    }
    if stopped {
        intent = FieldIntent::EditCommitted;
    }
    if secondary {
        let none_value = if N == 3 {
            FieldValue::Vec3(None)
        } else {
            FieldValue::Vec4(None)
        };
        spec.commit(state, none_value);
        intent = FieldIntent::EditAtomic;
    }
    intent
}

fn render_text<S>(
    ui: &mut egui::Ui,
    spec: &FieldSpec<S>,
    state: &mut S,
    max_len: Option<usize>,
) -> FieldIntent
where
    S: 'static,
{
    let current = match (spec.get)(state) {
        FieldValue::Text(v) => v,
        _ => return FieldIntent::None,
    };
    let mut intent = FieldIntent::None;

    ui.horizontal(|ui| {
        draw_label(ui, spec.label, spec.description);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut buf = current.clone();
            let mut edit = egui::TextEdit::singleline(&mut buf).desired_width(220.0);
            if let Some(n) = max_len {
                edit = edit.char_limit(n);
            }
            let resp = ui.add(edit);
            if resp.gained_focus() {
                intent = FieldIntent::EditStarted;
            }
            if resp.changed() && buf != current {
                spec.commit(state, FieldValue::Text(buf.clone()));
            }
            if resp.lost_focus() {
                intent = FieldIntent::EditCommitted;
            }
        });
    });
    intent
}

fn render_option_text<S>(
    ui: &mut egui::Ui,
    spec: &FieldSpec<S>,
    state: &mut S,
    max_len: Option<usize>,
) -> FieldIntent
where
    S: 'static,
{
    let current = match (spec.get)(state) {
        FieldValue::OptionText(v) => v,
        _ => return FieldIntent::None,
    };
    let mut intent = FieldIntent::None;

    ui.horizontal(|ui| {
        draw_label(ui, spec.label, spec.description);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut buf = current.clone().unwrap_or_default();
            let mut edit = egui::TextEdit::singleline(&mut buf)
                .desired_width(220.0)
                .hint_text("(unset)");
            if let Some(n) = max_len {
                edit = edit.char_limit(n);
            }
            let resp = ui.add(edit);
            if resp.gained_focus() {
                intent = FieldIntent::EditStarted;
            }
            if resp.changed() {
                let new_value = if buf.is_empty() {
                    None
                } else {
                    Some(buf.clone())
                };
                if new_value != current {
                    spec.commit(state, FieldValue::OptionText(new_value));
                }
            }
            if resp.lost_focus() {
                intent = FieldIntent::EditCommitted;
            }
        });
    });
    intent
}

fn render_passthrough_texture<S>(
    ui: &mut egui::Ui,
    spec: &FieldSpec<S>,
    state: &mut S,
    _extensions: &[&str],
) -> FieldIntent
where
    S: 'static,
{
    // Schema-driven simple path: same TextEdit + "(unset)" hint as
    // OptionText. The richer file-picker + preview UX from the
    // existing `FilePickerField` / `MapEdgeEditor` lives in those
    // modules; integrating it via the schema is straightforward but
    // not necessary for the first conversion -- the modal-specific
    // panels can still wrap the schema-driven row with a Browse
    // button alongside it.
    render_option_text(ui, spec, state, None)
}

// Unused import warning suppression for categories -- the module
// path is re-exported through `bar_project::field_schema::categories`
// and used by callers, but this file doesn't reference it directly.
#[allow(dead_code)]
fn _categories_reachable() -> &'static str {
    categories::IDENTITY
}

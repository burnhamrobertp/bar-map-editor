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
            // Prefer the in-flight pre-edit snapshot (saved by a prior
            // EditStarted) so undo restores the correct pre-commit state.
            // Clears the slot regardless to prevent it being pushed again
            // by a subsequent EditCommitted on an unrelated field.
            let snap = if let Some(pre) = app.dialog.field_edit_in_progress.take() {
                pre
            } else {
                app.snapshot(&format!("Edit {}", label))
            };
            app.history.push(snap);
            app.mark_dirty();
        }
    }
}

/// Outline the field row matching the validation severity AND
/// attach a hover tooltip explaining the finding. Tooltip text is
/// driven entirely by severity (the finding's per-field message is
/// intentionally generic at this surface; users open the validation
/// sidebar for the full text).
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
    let tooltip = match sev {
        Severity::Error => "Provided value is invalid",
        Severity::Warning => "Provided value is outside reference range",
        // Info findings don't currently outline fields, but keep the
        // arm exhaustive in case we widen the contract later.
        Severity::Info => "",
    };
    let result = egui::Frame::group(ui.style())
        .stroke(egui::Stroke::new(1.5, colour))
        .corner_radius(2.0)
        .inner_margin(egui::Margin::symmetric(2, 1))
        .show(ui, body);
    if !tooltip.is_empty() {
        // `interact` upgrades the frame's hover-only response so the
        // tooltip fires anywhere inside the bordered area, not just
        // on the individual widgets within.
        let resp = ui.interact(
            result.response.rect,
            result.response.id.with("severity_tooltip"),
            egui::Sense::hover(),
        );
        resp.on_hover_text(tooltip);
    }
    result.inner
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

/// Top-tier section heading inside an action-bar modal. Uses
/// egui's heading font size *and* the strong (full-brightness) text
/// colour so it dominates the visual hierarchy. Without
/// `.strong()`, the dark theme paints `heading()` text at the
/// faded body colour, which ended up DIMMER than the group
/// sub-heading below (rendered with `.strong()` at normal size).
/// Calling this helper keeps the three tiers progressively less
/// prominent: section heading > group sub-heading > field label.
pub fn section_heading(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(
        egui::RichText::new(text).heading().strong(),
    ));
}

/// Draw a section heading row with an info icon at the right
/// carrying `tooltip` as hover text. Replaces the
/// `heading + paragraph` pattern that used to clutter the modals.
pub fn heading_with_info(ui: &mut egui::Ui, heading: &str, tooltip: &str) {
    ui.horizontal(|ui| {
        section_heading(ui, heading);
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
        FieldKind::F32 { hard, soft, unit } => render_f32_opt(ui, spec, state, *hard, *soft, unit),
        FieldKind::U32 { hard, soft, unit } => render_u32_opt(ui, spec, state, *hard, *soft, unit),
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

/// Reset-to-default control. Reserves a fixed slot even when hidden so
/// rows stay vertically aligned whether or not a field is set. Visible
/// only when `set` (a field holding a value is what you can clear back
/// to the engine default). Returns true when clicked.
fn revert_button(ui: &mut egui::Ui, set: bool) -> bool {
    let size = egui::vec2(18.0, 18.0);
    if set {
        ui.add_sized(size, egui::Button::new("\u{21ba}").frame(false))
            .on_hover_text("Reset to default")
            .clicked()
    } else {
        ui.allocate_space(size);
        false
    }
}

/// Slider whose track carries a faint tick at the engine-default
/// position, so a set field still shows where "unset" would sit.
fn slider_with_default_tick(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: (f32, f32),
    default: f32,
) -> egui::Response {
    // Gap from the value box (the slider's right neighbour in this RTL row)
    // plus a left margin via the reduced width, so the handle at either
    // extreme never tucks under the neighbouring widgets.
    ui.add_space(8.0);
    let avail = ui.available_width();
    if avail > 24.0 {
        ui.spacing_mut().slider_width = avail - 16.0;
    }
    let resp = ui.add(
        egui::Slider::new(value, range.0..=range.1)
            .show_value(false)
            .clamping(egui::SliderClamping::Never),
    );
    if range.1 > range.0 {
        let frac = ((default - range.0) / (range.1 - range.0)).clamp(0.0, 1.0);
        let r = resp.rect;
        let x = r.left() + frac * r.width();
        ui.painter().vline(
            x,
            (r.top() + 3.0)..=(r.bottom() - 3.0),
            egui::Stroke::new(1.5, ui.visuals().weak_text_color()),
        );
    }
    resp
}

/// Show `desc` as a hover tooltip anywhere within a field row. A bare
/// `ui.horizontal` response only reports hover in the gaps between its
/// child widgets, so re-interact the row's full rect with a hover sense
/// (the trick `outline_severity` uses) so it covers the controls too.
fn row_tooltip(ui: &mut egui::Ui, row: &egui::Response, desc: Option<&str>) {
    if let Some(desc) = desc {
        ui.interact(row.rect, row.id.with("row_tip"), egui::Sense::hover())
            .on_hover_text(desc);
    }
}

fn render_f32_opt<S>(
    ui: &mut egui::Ui,
    spec: &FieldSpec<S>,
    state: &mut S,
    hard: (f32, f32),
    soft: Option<(f32, f32)>,
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
    // "Cleared" flag persisted in egui's context data across frames.
    // Stack-local Cell<bool> would be lost between the frame where
    // custom_parser fires (user presses Delete) and the frame where
    // lost_focus() fires (user presses Tab). Keyed by spec.id so each
    // field tracks independently.
    let cleared_key = egui::Id::new(("field_cleared", spec.id));
    let ctx = ui.ctx().clone();
    let range = soft.unwrap_or(hard);

    let row = ui.horizontal(|ui| {
        ui.add(egui::Label::new(spec.label).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if revert_button(ui, !is_unset) {
                spec.commit(state, FieldValue::F32(None));
                intent = FieldIntent::EditAtomic;
            }
            if !unit.is_empty() {
                ui.label(egui::RichText::new(unit).weak());
            }
            let dv = egui::DragValue::new(&mut value)
                .range(hard.0..=hard.1)
                .speed((hard.1 - hard.0) / 1000.0)
                .custom_parser({
                    let ctx = ctx.clone();
                    move |s| {
                        let trimmed = s.trim();
                        if trimmed.is_empty() {
                            ctx.data_mut(|d| d.insert_temp::<bool>(cleared_key, true));
                            None
                        } else {
                            ctx.data_mut(|d| d.insert_temp::<bool>(cleared_key, false));
                            trimmed.parse::<f64>().ok()
                        }
                    }
                });
            // Unset value reads as a muted placeholder so it's visually
            // distinct from a value the user actually entered.
            let resp = if is_unset {
                ui.scope(|ui| {
                    let weak = ui.visuals().weak_text_color();
                    ui.visuals_mut().override_text_color = Some(weak);
                    ui.add(dv)
                })
                .inner
            } else {
                ui.add(dv)
            };
            if resp.drag_started() || resp.gained_focus() {
                if resp.gained_focus() {
                    ctx.data_mut(|d| d.insert_temp::<bool>(cleared_key, false));
                }
                intent = FieldIntent::EditStarted;
            }
            let cleared = ctx.data(|d| d.get_temp::<bool>(cleared_key).unwrap_or(false));
            if resp.lost_focus() && cleared {
                ctx.data_mut(|d| d.insert_temp::<bool>(cleared_key, false));
                spec.commit(state, FieldValue::F32(None));
                intent = FieldIntent::EditAtomic;
            } else if (value - displayed).abs() > f32::EPSILON {
                spec.commit(state, FieldValue::F32(Some(value)));
            }
            if intent != FieldIntent::EditAtomic && (resp.drag_stopped() || resp.lost_focus()) {
                intent = FieldIntent::EditCommitted;
            }
            let s = slider_with_default_tick(ui, &mut value, range, default);
            if s.drag_started() {
                intent = FieldIntent::EditStarted;
            }
            if s.changed() && (value - displayed).abs() > f32::EPSILON {
                spec.commit(state, FieldValue::F32(Some(value)));
            }
            if s.drag_stopped() {
                intent = FieldIntent::EditCommitted;
            }
        });
    });
    row_tooltip(ui, &row.response, spec.description);
    intent
}

fn render_u32_opt<S>(
    ui: &mut egui::Ui,
    spec: &FieldSpec<S>,
    state: &mut S,
    hard: (u32, u32),
    soft: Option<(u32, u32)>,
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
    let cleared_key = egui::Id::new(("field_cleared", spec.id));
    let ctx = ui.ctx().clone();
    let range = soft.unwrap_or(hard);

    let row = ui.horizontal(|ui| {
        ui.add(egui::Label::new(spec.label).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if revert_button(ui, !is_unset) {
                spec.commit(state, FieldValue::U32(None));
                intent = FieldIntent::EditAtomic;
            }
            if !unit.is_empty() {
                ui.label(egui::RichText::new(unit).weak());
            }
            let dv = egui::DragValue::new(&mut value)
                .range(hard.0..=hard.1)
                .custom_parser({
                    let ctx = ctx.clone();
                    move |s| {
                        let trimmed = s.trim();
                        if trimmed.is_empty() {
                            ctx.data_mut(|d| d.insert_temp::<bool>(cleared_key, true));
                            None
                        } else {
                            ctx.data_mut(|d| d.insert_temp::<bool>(cleared_key, false));
                            trimmed.parse::<f64>().ok()
                        }
                    }
                });
            let resp = if is_unset {
                ui.scope(|ui| {
                    let weak = ui.visuals().weak_text_color();
                    ui.visuals_mut().override_text_color = Some(weak);
                    ui.add(dv)
                })
                .inner
            } else {
                ui.add(dv)
            };
            if resp.drag_started() || resp.gained_focus() {
                if resp.gained_focus() {
                    ctx.data_mut(|d| d.insert_temp::<bool>(cleared_key, false));
                }
                intent = FieldIntent::EditStarted;
            }
            let cleared = ctx.data(|d| d.get_temp::<bool>(cleared_key).unwrap_or(false));
            if resp.lost_focus() && cleared {
                ctx.data_mut(|d| d.insert_temp::<bool>(cleared_key, false));
                spec.commit(state, FieldValue::U32(None));
                intent = FieldIntent::EditAtomic;
            } else if value != displayed {
                spec.commit(state, FieldValue::U32(Some(value)));
            }
            if intent != FieldIntent::EditAtomic && (resp.drag_stopped() || resp.lost_focus()) {
                intent = FieldIntent::EditCommitted;
            }
            let mut fval = value as f32;
            let s = slider_with_default_tick(
                ui,
                &mut fval,
                (range.0 as f32, range.1 as f32),
                default as f32,
            );
            if s.drag_started() {
                intent = FieldIntent::EditStarted;
            }
            if s.changed() {
                let nv = fval.round().clamp(hard.0 as f32, hard.1 as f32) as u32;
                if nv != displayed {
                    spec.commit(state, FieldValue::U32(Some(nv)));
                }
            }
            if s.drag_stopped() {
                intent = FieldIntent::EditCommitted;
            }
        });
    });
    row_tooltip(ui, &row.response, spec.description);
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

    let row = ui.horizontal(|ui| {
        ui.add(egui::Label::new(spec.label).truncate());
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
    row_tooltip(ui, &row.response, spec.description);
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

    let row = ui.horizontal(|ui| {
        ui.add(egui::Label::new(spec.label).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if revert_button(ui, !is_unset) {
                spec.commit(state, FieldValue::Color(None));
                intent = FieldIntent::EditAtomic;
            }
            // Always-editable swatch. When the recipe value is None the
            // swatch shows the engine default; the first change promotes
            // it to Some(value).
            let mut linear = bar_render::color::srgb_to_linear_rgb(displayed);
            let resp = ui.color_edit_button_rgb(&mut linear);
            if resp.changed() {
                let srgb = bar_render::color::linear_to_srgb_rgb(linear);
                spec.commit(state, FieldValue::Color(Some(srgb)));
                intent = FieldIntent::EditAtomic;
            }
        });
    });
    row_tooltip(ui, &row.response, spec.description);
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
    // Any channel being cleared + blurred reverts the whole vector to
    // None. Keyed by spec.id so channels in different fields don't
    // share a flag. Persisted in egui context data across frames.
    let cleared_key = egui::Id::new(("field_cleared", spec.id));
    let ctx = ui.ctx().clone();

    let row = ui.horizontal(|ui| {
        ui.add(egui::Label::new(spec.label).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if revert_button(ui, !is_unset) {
                secondary = true;
            }
            for slot in arr.iter_mut().take(N) {
                let resp = ui.add(
                    egui::DragValue::new(slot)
                        .range(hard.0..=hard.1)
                        .speed((hard.1 - hard.0) / 1000.0)
                        .custom_parser({
                            let ctx = ctx.clone();
                            move |s| {
                                let trimmed = s.trim();
                                if trimmed.is_empty() {
                                    ctx.data_mut(|d| d.insert_temp::<bool>(cleared_key, true));
                                    None
                                } else {
                                    trimmed.parse::<f64>().ok()
                                }
                            }
                        }),
                );
                if resp.drag_started() || resp.gained_focus() {
                    if resp.gained_focus() {
                        ctx.data_mut(|d| d.insert_temp::<bool>(cleared_key, false));
                    }
                    started = true;
                }
                if resp.drag_stopped() || resp.lost_focus() {
                    stopped = true;
                }
                if resp.changed() {
                    any_changed = true;
                }
            }
        });
    });
    row_tooltip(ui, &row.response, spec.description);

    let cleared = ctx.data(|d| d.get_temp::<bool>(cleared_key).unwrap_or(false));
    let reverting = stopped && cleared;
    if reverting || secondary {
        ctx.data_mut(|d| d.insert_temp::<bool>(cleared_key, false));
        let none_value = if N == 3 {
            FieldValue::Vec3(None)
        } else {
            FieldValue::Vec4(None)
        };
        spec.commit(state, none_value);
        intent = FieldIntent::EditAtomic;
    } else if any_changed {
        let new_value = if N == 3 {
            FieldValue::Vec3(Some([arr[0], arr[1], arr[2]]))
        } else {
            FieldValue::Vec4(Some([arr[0], arr[1], arr[2], arr[3]]))
        };
        spec.commit(state, new_value);
    }
    if intent == FieldIntent::None {
        if started {
            intent = FieldIntent::EditStarted;
        }
        if stopped {
            intent = FieldIntent::EditCommitted;
        }
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

    let row = ui.horizontal(|ui| {
        ui.add(egui::Label::new(spec.label).truncate());
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
    row_tooltip(ui, &row.response, spec.description);
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

    let row = ui.horizontal(|ui| {
        ui.add(egui::Label::new(spec.label).truncate());
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
    row_tooltip(ui, &row.response, spec.description);
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

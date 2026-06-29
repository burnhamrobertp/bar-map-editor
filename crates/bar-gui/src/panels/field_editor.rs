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

use bar_project::field_schema::{categories, DefaultValue, FieldKind, FieldSpec, FieldValue};
use bar_project::recipe::MapSettings;
use bar_project::Severity;
use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::widgets::{field_row, revert_button};

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

/// How a field looks: label, optional tooltip, kind (which drives the
/// widget and clamp range), and the default it reverts to / ticks at.
/// The value itself and where it's written live behind a [`FieldModel`],
/// so the same renderer drives settings (`MapSettings`) and node params.
#[derive(Clone)]
pub struct FieldDesc<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub description: Option<&'a str>,
    pub kind: FieldKind,
    pub default: DefaultValue,
}

/// Where a field's value lives and when its side effects fire. The
/// renderer reads [`value`](Self::value), writes [`set_value`](Self::set_value)
/// on every changed frame (cheap -- no graph eval), and calls
/// [`commit`](Self::commit) at the end of an edit session, which owns
/// undo (and, for node params, the re-eval `mark_dirty`). One renderer +
/// this trait means commit / revert / undo are written once; a caller
/// cannot forget them.
pub trait FieldModel {
    fn value(&self) -> FieldValue;
    /// Write a (renderer-supplied) value through, clamped against the
    /// field's own kind. Lazily snapshots the pre-edit state into the
    /// undo slot on the first write of an edit session, so a multi-frame
    /// drag captures exactly one pre-edit snapshot and a
    /// focus-without-change captures none. No history push, no eval here.
    fn set_value(&mut self, v: FieldValue);
    /// End of an edit session (drag stop / lost focus / atomic click):
    /// push the pending pre-edit snapshot to history and mark dirty.
    /// Node-param impls also re-evaluate the graph here. A no-op when no
    /// `set_value` ran this session (so idle focus leaves no undo entry).
    fn commit(&mut self);
}

/// [`FieldModel`] for a settings field: reads/writes `MapSettings`
/// through the spec's `get`/`set`, undo through [`process_intent`].
pub(crate) struct SettingsField<'a> {
    app: &'a mut BarEditorApp,
    spec: &'a FieldSpec<MapSettings>,
}

impl<'a> SettingsField<'a> {
    pub(crate) fn new(app: &'a mut BarEditorApp, spec: &'a FieldSpec<MapSettings>) -> Self {
        Self { app, spec }
    }
}

impl FieldModel for SettingsField<'_> {
    fn value(&self) -> FieldValue {
        (self.spec.get)(self.app.map_settings())
    }
    fn set_value(&mut self, v: FieldValue) {
        if self.app.dialog.field_edit_in_progress.is_none() {
            let snap = self.app.snapshot(&format!("Edit {}", self.spec.label));
            self.app.dialog.field_edit_in_progress = Some(snap);
        }
        // `commit` hard-clamps against the spec's kind before writing.
        self.spec.commit(self.app.map_settings_mut(), v);
    }
    fn commit(&mut self) {
        if let Some(snap) = self.app.dialog.field_edit_in_progress.take() {
            self.app.history.push(snap);
        }
        self.app.mark_dirty();
    }
}

/// Borrow a [`FieldDesc`] view of a settings spec.
pub(crate) fn desc_of<S>(spec: &FieldSpec<S>) -> FieldDesc<'_> {
    FieldDesc {
        id: spec.id,
        label: spec.label,
        description: spec.description,
        kind: spec.kind.clone(),
        default: spec.default,
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
pub fn render_field(
    ui: &mut egui::Ui,
    desc: &FieldDesc,
    model: &mut dyn FieldModel,
    finding_severity: Option<Severity>,
) -> FieldIntent {
    let intent = outline_severity(ui, finding_severity, |ui| match &desc.kind {
        FieldKind::F32 { hard, soft, unit } => render_f32_opt(ui, desc, model, *hard, *soft, unit),
        FieldKind::U32 { hard, soft, unit } => render_u32_opt(ui, desc, model, *hard, *soft, unit),
        FieldKind::Bool => render_bool_opt(ui, desc, model),
        FieldKind::Color => render_color_opt(ui, desc, model),
        FieldKind::Vec3 { hard, soft: _ } => render_vec_opt::<3>(ui, desc, model, *hard),
        FieldKind::Vec4 { hard, soft: _ } => render_vec_opt::<4>(ui, desc, model, *hard),
        FieldKind::Text { max_len } => render_text(ui, desc, model, *max_len),
        FieldKind::OptionText { max_len } => render_option_text(ui, desc, model, *max_len),
        FieldKind::PassthroughTexture { extensions } => {
            render_passthrough_texture(ui, desc, model, extensions)
        }
        FieldKind::FloatFree => render_float_free(ui, desc, model),
        FieldKind::UIntFree => render_uint_free(ui, desc, model),
        FieldKind::IntFree => render_int_free(ui, desc, model),
        FieldKind::Choices(opts) => render_choices(ui, desc, model, opts),
    });
    // Single place the edit session is closed out, so no call site can
    // forget undo/dirty (and, for node params, re-eval). set_value did the
    // lazy pre-edit snapshot; commit pushes it. Start-only frames (drag
    // begun, nothing changed yet) fall through and commit nothing.
    if matches!(intent, FieldIntent::EditCommitted | FieldIntent::EditAtomic) {
        model.commit();
    }
    intent
}

// ──────────────────────────────────────────────────────────────────
// Per-kind renderers
// ──────────────────────────────────────────────────────────────────

/// Read the engine default for kind that returns an f32-shaped value,
/// or 0.0 if the spec carries something unexpected.
fn default_f32(d: DefaultValue) -> f32 {
    match d {
        DefaultValue::F32(v) => v,
        DefaultValue::I32(v) => v as f32,
        DefaultValue::U32(v) => v as f32,
        _ => 0.0,
    }
}

fn default_u32(d: DefaultValue) -> u32 {
    match d {
        DefaultValue::U32(v) => v,
        _ => 0,
    }
}

fn default_i32(d: DefaultValue) -> i32 {
    match d {
        DefaultValue::I32(v) => v,
        DefaultValue::U32(v) => v as i32,
        _ => 0,
    }
}

fn default_color(d: DefaultValue) -> [f32; 3] {
    match d {
        DefaultValue::Color(v) | DefaultValue::Vec3(v) => v,
        _ => [0.5, 0.5, 0.5],
    }
}

fn default_vec3(d: DefaultValue) -> [f32; 3] {
    match d {
        DefaultValue::Vec3(v) | DefaultValue::Color(v) => v,
        _ => [0.0; 3],
    }
}

fn default_vec4(d: DefaultValue) -> [f32; 4] {
    match d {
        DefaultValue::Vec4(v) => v,
        _ => [0.0; 4],
    }
}

fn default_bool(d: DefaultValue) -> bool {
    matches!(d, DefaultValue::Bool(true))
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

fn render_f32_opt(
    ui: &mut egui::Ui,
    desc: &FieldDesc,
    model: &mut dyn FieldModel,
    hard: (f32, f32),
    soft: Option<(f32, f32)>,
    unit: &str,
) -> FieldIntent {
    let current = match model.value() {
        FieldValue::F32(v) => v,
        _ => return FieldIntent::None,
    };
    let default = default_f32(desc.default);
    let is_unset = current.is_none();
    let displayed = current.unwrap_or(default);
    let mut value = displayed;
    let mut intent = FieldIntent::None;
    // "Cleared" flag persisted in egui's context data across frames.
    // Stack-local Cell<bool> would be lost between the frame where
    // custom_parser fires (user presses Delete) and the frame where
    // lost_focus() fires (user presses Tab). Keyed by desc.id so each
    // field tracks independently.
    let cleared_key = egui::Id::new(("field_cleared", desc.id));
    let ctx = ui.ctx().clone();
    let range = soft.unwrap_or(hard);

    field_row(ui, desc.label, desc.description, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if revert_button(ui, !is_unset) {
                model.set_value(FieldValue::F32(None));
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
                model.set_value(FieldValue::F32(None));
                intent = FieldIntent::EditAtomic;
            } else if (value - displayed).abs() > f32::EPSILON {
                model.set_value(FieldValue::F32(Some(value)));
            }
            if intent != FieldIntent::EditAtomic && (resp.drag_stopped() || resp.lost_focus()) {
                intent = FieldIntent::EditCommitted;
            }
            let s = slider_with_default_tick(ui, &mut value, range, default);
            if s.drag_started() {
                intent = FieldIntent::EditStarted;
            }
            if s.changed() && (value - displayed).abs() > f32::EPSILON {
                model.set_value(FieldValue::F32(Some(value)));
            }
            if s.drag_stopped() {
                intent = FieldIntent::EditCommitted;
            }
        });
    });
    intent
}

fn render_u32_opt(
    ui: &mut egui::Ui,
    desc: &FieldDesc,
    model: &mut dyn FieldModel,
    hard: (u32, u32),
    soft: Option<(u32, u32)>,
    unit: &str,
) -> FieldIntent {
    let current = match model.value() {
        FieldValue::U32(v) => v,
        _ => return FieldIntent::None,
    };
    let default = default_u32(desc.default);
    let is_unset = current.is_none();
    let displayed = current.unwrap_or(default);
    let mut value = displayed;
    let mut intent = FieldIntent::None;
    let cleared_key = egui::Id::new(("field_cleared", desc.id));
    let ctx = ui.ctx().clone();
    let range = soft.unwrap_or(hard);

    field_row(ui, desc.label, desc.description, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if revert_button(ui, !is_unset) {
                model.set_value(FieldValue::U32(None));
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
                model.set_value(FieldValue::U32(None));
                intent = FieldIntent::EditAtomic;
            } else if value != displayed {
                model.set_value(FieldValue::U32(Some(value)));
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
                    model.set_value(FieldValue::U32(Some(nv)));
                }
            }
            if s.drag_stopped() {
                intent = FieldIntent::EditCommitted;
            }
        });
    });
    intent
}

fn render_bool_opt(ui: &mut egui::Ui, desc: &FieldDesc, model: &mut dyn FieldModel) -> FieldIntent {
    let current = match model.value() {
        FieldValue::Bool(v) => v,
        _ => return FieldIntent::None,
    };
    let default = default_bool(desc.default);
    let mut intent = FieldIntent::None;

    field_row(ui, desc.label, desc.description, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let display = match current {
                Some(true) => egui::RichText::new("true"),
                Some(false) => egui::RichText::new("false"),
                None => egui::RichText::new(default.to_string()).weak().italics(),
            };
            let mut local = current;
            egui::ComboBox::from_id_salt(("field_bool", desc.id))
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
                        model.set_value(FieldValue::Bool(None));
                        intent = FieldIntent::EditAtomic;
                    }
                    if ui
                        .selectable_value(&mut local, Some(true), "true")
                        .clicked()
                    {
                        model.set_value(FieldValue::Bool(Some(true)));
                        intent = FieldIntent::EditAtomic;
                    }
                    if ui
                        .selectable_value(&mut local, Some(false), "false")
                        .clicked()
                    {
                        model.set_value(FieldValue::Bool(Some(false)));
                        intent = FieldIntent::EditAtomic;
                    }
                });
        });
    });
    intent
}

fn render_color_opt(
    ui: &mut egui::Ui,
    desc: &FieldDesc,
    model: &mut dyn FieldModel,
) -> FieldIntent {
    let current = match model.value() {
        FieldValue::Color(v) => v,
        _ => return FieldIntent::None,
    };
    let default = default_color(desc.default);
    let is_unset = current.is_none();
    let displayed = current.unwrap_or(default);
    let mut intent = FieldIntent::None;

    field_row(ui, desc.label, desc.description, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if revert_button(ui, !is_unset) {
                model.set_value(FieldValue::Color(None));
                intent = FieldIntent::EditAtomic;
            }
            // Always-editable swatch. When the recipe value is None the
            // swatch shows the engine default; the first change promotes
            // it to Some(value).
            let mut linear = bar_render::color::srgb_to_linear_rgb(displayed);
            let resp = ui.color_edit_button_rgb(&mut linear);
            if resp.changed() {
                let srgb = bar_render::color::linear_to_srgb_rgb(linear);
                model.set_value(FieldValue::Color(Some(srgb)));
                intent = FieldIntent::EditAtomic;
            }
        });
    });
    intent
}

fn render_vec_opt<const N: usize>(
    ui: &mut egui::Ui,
    desc: &FieldDesc,
    model: &mut dyn FieldModel,
    hard: (f32, f32),
) -> FieldIntent {
    // Pulls the current array out of FieldValue::Vec3/Vec4 depending
    // on N. Renders N drag values in a horizontal strip with one
    // Override toggle in front. Commit semantics match F32: any
    // channel drag-started flips intent to EditStarted; any
    // channel-changed writes back; any channel drag-stopped /
    // lost-focus flips to EditCommitted.
    let (current_array, default_array): (Option<[f32; N]>, [f32; N]) = match N {
        3 => match model.value() {
            FieldValue::Vec3(v) => (
                v.map(|a| {
                    let mut out = [0.0; N];
                    out[..3].copy_from_slice(&a);
                    out
                }),
                {
                    let d = default_vec3(desc.default);
                    let mut out = [0.0; N];
                    out[..3].copy_from_slice(&d);
                    out
                },
            ),
            _ => return FieldIntent::None,
        },
        4 => match model.value() {
            FieldValue::Vec4(v) => (
                v.map(|a| {
                    let mut out = [0.0; N];
                    out[..4].copy_from_slice(&a);
                    out
                }),
                {
                    let d = default_vec4(desc.default);
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
    let cleared_key = egui::Id::new(("field_cleared", desc.id));
    let ctx = ui.ctx().clone();

    field_row(ui, desc.label, desc.description, |ui| {
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
    let cleared = ctx.data(|d| d.get_temp::<bool>(cleared_key).unwrap_or(false));
    let reverting = stopped && cleared;
    if reverting || secondary {
        ctx.data_mut(|d| d.insert_temp::<bool>(cleared_key, false));
        let none_value = if N == 3 {
            FieldValue::Vec3(None)
        } else {
            FieldValue::Vec4(None)
        };
        model.set_value(none_value);
        intent = FieldIntent::EditAtomic;
    } else if any_changed {
        let new_value = if N == 3 {
            FieldValue::Vec3(Some([arr[0], arr[1], arr[2]]))
        } else {
            FieldValue::Vec4(Some([arr[0], arr[1], arr[2], arr[3]]))
        };
        model.set_value(new_value);
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

fn render_text(
    ui: &mut egui::Ui,
    desc: &FieldDesc,
    model: &mut dyn FieldModel,
    max_len: Option<usize>,
) -> FieldIntent {
    let current = match model.value() {
        FieldValue::Text(v) => v,
        _ => return FieldIntent::None,
    };
    let mut intent = FieldIntent::None;

    field_row(ui, desc.label, desc.description, |ui| {
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
                model.set_value(FieldValue::Text(buf.clone()));
            }
            if resp.lost_focus() {
                intent = FieldIntent::EditCommitted;
            }
        });
    });
    intent
}

fn render_option_text(
    ui: &mut egui::Ui,
    desc: &FieldDesc,
    model: &mut dyn FieldModel,
    max_len: Option<usize>,
) -> FieldIntent {
    let current = match model.value() {
        FieldValue::OptionText(v) => v,
        _ => return FieldIntent::None,
    };
    let mut intent = FieldIntent::None;

    field_row(ui, desc.label, desc.description, |ui| {
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
                    model.set_value(FieldValue::OptionText(new_value));
                }
            }
            if resp.lost_focus() {
                intent = FieldIntent::EditCommitted;
            }
        });
    });
    intent
}

fn render_passthrough_texture(
    ui: &mut egui::Ui,
    desc: &FieldDesc,
    model: &mut dyn FieldModel,
    _extensions: &[&str],
) -> FieldIntent {
    // Schema-driven simple path: same TextEdit + "(unset)" hint as
    // OptionText. The richer file-picker + preview UX from the
    // existing `FilePickerField` / `MapEdgeEditor` lives in those
    // modules; integrating it via the schema is straightforward but
    // not necessary for the first conversion -- the modal-specific
    // panels can still wrap the schema-driven row with a Browse
    // button alongside it.
    render_option_text(ui, desc, model, None)
}

// ── New-kind renderers (node params; settings don't use these yet) ──

fn render_float_free(
    ui: &mut egui::Ui,
    desc: &FieldDesc,
    model: &mut dyn FieldModel,
) -> FieldIntent {
    let mut value = match model.value() {
        FieldValue::F32(v) => v.unwrap_or_else(|| default_f32(desc.default)),
        _ => return FieldIntent::None,
    };
    let mut intent = FieldIntent::None;
    field_row(ui, desc.label, desc.description, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let resp = ui.add(egui::DragValue::new(&mut value).speed(0.01));
            if resp.drag_started() || resp.gained_focus() {
                intent = FieldIntent::EditStarted;
            }
            if resp.changed() {
                model.set_value(FieldValue::F32(Some(value)));
            }
            if resp.drag_stopped() || resp.lost_focus() {
                intent = FieldIntent::EditCommitted;
            }
        });
    });
    intent
}

fn render_uint_free(
    ui: &mut egui::Ui,
    desc: &FieldDesc,
    model: &mut dyn FieldModel,
) -> FieldIntent {
    let mut value = match model.value() {
        FieldValue::U32(v) => v.unwrap_or_else(|| default_u32(desc.default)),
        _ => return FieldIntent::None,
    };
    let mut intent = FieldIntent::None;
    field_row(ui, desc.label, desc.description, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let resp = ui.add(egui::DragValue::new(&mut value));
            if resp.drag_started() || resp.gained_focus() {
                intent = FieldIntent::EditStarted;
            }
            if resp.changed() {
                model.set_value(FieldValue::U32(Some(value)));
            }
            if resp.drag_stopped() || resp.lost_focus() {
                intent = FieldIntent::EditCommitted;
            }
        });
    });
    intent
}

fn render_int_free(ui: &mut egui::Ui, desc: &FieldDesc, model: &mut dyn FieldModel) -> FieldIntent {
    let mut value = match model.value() {
        FieldValue::I32(v) => v.unwrap_or_else(|| default_i32(desc.default)),
        _ => return FieldIntent::None,
    };
    let mut intent = FieldIntent::None;
    field_row(ui, desc.label, desc.description, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let resp = ui.add(egui::DragValue::new(&mut value));
            if resp.drag_started() || resp.gained_focus() {
                intent = FieldIntent::EditStarted;
            }
            if resp.changed() {
                model.set_value(FieldValue::I32(Some(value)));
            }
            if resp.drag_stopped() || resp.lost_focus() {
                intent = FieldIntent::EditCommitted;
            }
        });
    });
    intent
}

fn render_choices(
    ui: &mut egui::Ui,
    desc: &FieldDesc,
    model: &mut dyn FieldModel,
    opts: &[&str],
) -> FieldIntent {
    let current = match model.value() {
        FieldValue::Text(v) => v,
        _ => return FieldIntent::None,
    };
    let mut intent = FieldIntent::None;
    field_row(ui, desc.label, desc.description, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            egui::ComboBox::from_id_salt(("field_choices", desc.id))
                .selected_text(&current)
                .show_ui(ui, |ui| {
                    for opt in opts {
                        if ui.selectable_label(current == *opt, *opt).clicked() && current != *opt {
                            model.set_value(FieldValue::Text((*opt).to_string()));
                            intent = FieldIntent::EditAtomic;
                        }
                    }
                });
        });
    });
    intent
}

// Unused import warning suppression for categories -- the module
// path is re-exported through `bar_project::field_schema::categories`
// and used by callers, but this file doesn't reference it directly.
#[allow(dead_code)]
fn _categories_reachable() -> &'static str {
    categories::IDENTITY
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_project::recipe::MapSettings;

    // A real `MapSettings` field (`gravity: Option<f32>`) so the model
    // exercises the live recipe path, not a stand-in struct.
    fn gravity_spec() -> FieldSpec<MapSettings> {
        FieldSpec {
            id: "test.gravity",
            label: "Gravity",
            description: None,
            kind: FieldKind::F32 {
                hard: (10.0, 500.0),
                soft: None,
                unit: "",
            },
            default: DefaultValue::F32(130.0),
            get: |s| FieldValue::F32(s.gravity),
            set: |s, v| {
                if let FieldValue::F32(x) = v {
                    s.gravity = x;
                }
            },
            category: "physics",
            group: "",
            blocks_export: false,
        }
    }

    // A drag = many set_value frames then one commit = exactly one undo
    // entry, and undo restores the true pre-edit value (no first-frame
    // slop, because set_value snapshots before its first write).
    #[test]
    fn settings_field_commit_is_one_entry_and_undo_round_trips() {
        let mut app = BarEditorApp::default();
        app.map_settings_mut().gravity = Some(100.0);
        let depth = app.history.undo_depth();
        let spec = gravity_spec();
        {
            let mut m = SettingsField::new(&mut app, &spec);
            m.set_value(FieldValue::F32(Some(180.0)));
            m.set_value(FieldValue::F32(Some(250.0)));
            m.commit();
        }
        assert_eq!(app.map_settings().gravity, Some(250.0));
        assert_eq!(
            app.history.undo_depth(),
            depth + 1,
            "one entry per edit session"
        );
        app.undo();
        assert_eq!(
            app.map_settings().gravity,
            Some(100.0),
            "undo restores the pre-edit value"
        );
        app.redo();
        assert_eq!(app.map_settings().gravity, Some(250.0), "redo re-applies");
    }

    // A discrete edit (combo / color / revert -> set_value + commit on the
    // same frame) is undoable -- the pre-fix flow snapshotted post-write,
    // making such undos a no-op.
    #[test]
    fn settings_field_atomic_edit_is_undoable() {
        let mut app = BarEditorApp::default();
        app.map_settings_mut().gravity = Some(100.0);
        let spec = gravity_spec();
        {
            let mut m = SettingsField::new(&mut app, &spec);
            m.set_value(FieldValue::F32(Some(300.0)));
            m.commit();
        }
        assert_eq!(app.map_settings().gravity, Some(300.0));
        app.undo();
        assert_eq!(app.map_settings().gravity, Some(100.0));
    }

    // Focus in / focus out without changing anything must leave no undo
    // entry behind.
    #[test]
    fn settings_field_no_write_no_entry() {
        let mut app = BarEditorApp::default();
        let depth = app.history.undo_depth();
        let spec = gravity_spec();
        {
            let mut m = SettingsField::new(&mut app, &spec);
            m.commit();
        }
        assert_eq!(
            app.history.undo_depth(),
            depth,
            "no set_value -> no undo entry"
        );
    }

    #[test]
    fn settings_field_clamps_to_hard_range() {
        let mut app = BarEditorApp::default();
        let spec = gravity_spec();
        let mut m = SettingsField::new(&mut app, &spec);
        m.set_value(FieldValue::F32(Some(99_999.0)));
        assert_eq!(
            app.map_settings().gravity,
            Some(500.0),
            "clamped to the kind's hard max"
        );
    }
}

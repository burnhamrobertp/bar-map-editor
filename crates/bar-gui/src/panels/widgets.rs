//! Reusable GUI widgets for the editor.

use eframe::egui;

const INPUT_H: f32 = 18.0;
const BAR_H: f32 = 8.0;
const HANDLE_W: f32 = 8.0;
const INPUT_W_DEFAULT: f32 = 52.0;
const BAR_INPUT_GAP: f32 = 4.0;

/// Select all text in a `TextEdit` when it first gains focus.
pub(crate) fn select_all_on_focus(ui: &mut egui::Ui, resp: &egui::Response, text: &str) {
    if resp.gained_focus() {
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), resp.id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(text.chars().count()),
                )));
            egui::TextEdit::store_state(ui.ctx(), resp.id, state);
        }
    }
}

/// Fixed width of the aligned label column. Constant (not a fraction of
/// available width) so every row's control column starts at the same x and
/// the column can't drift as an auto-sizing panel settles frame-to-frame.
pub(crate) const FIELD_LABEL_W: f32 = 150.0;

/// Fixed-width, left-aligned, truncating label cell -- the left column of
/// a settings/property row. The width is a constant ([`FIELD_LABEL_W`]) so
/// controls line up in a column across every row and every surface, and the
/// column doesn't grow row-by-row as the panel sizes itself. Long (e.g.
/// localized) labels truncate, with the full text still reachable via the
/// whole-row hover tooltip.
pub(crate) fn field_label(ui: &mut egui::Ui, text: &str) {
    let w = FIELD_LABEL_W;
    let h = ui.spacing().interact_size.y;
    // `add_sized` centers its widget; we want flush-left, so lay it out
    // left-to-right inside the fixed-size cell.
    ui.allocate_ui_with_layout(
        egui::vec2(w, h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add(egui::Label::new(text).truncate());
        },
    );
}

/// Reset-to-default control. Reserves a fixed slot even when hidden so
/// rows stay vertically aligned whether or not a field is set. Visible
/// only when `set`. Returns true when clicked.
pub(crate) fn revert_button(ui: &mut egui::Ui, set: bool) -> bool {
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

/// Show `desc` as a hover tooltip anywhere within a field row. A bare
/// `ui.horizontal` response only reports hover in the gaps between its
/// child widgets, so re-interact the row's full rect with a hover sense
/// so the tooltip covers the controls too.
pub(crate) fn row_tooltip(ui: &mut egui::Ui, row: &egui::Response, desc: Option<&str>) {
    if let Some(desc) = desc {
        ui.interact(row.rect, row.id.with("row_tip"), egui::Sense::hover())
            .on_hover_text(desc);
    }
}

/// The single row layout every editable field uses across the app --
/// settings modals, node properties, layout properties: an aligned label
/// column, then the caller's control(s), with `description` as a
/// whole-row hover tooltip. One definition means every surface stays
/// aligned identically and can't drift apart.
pub(crate) fn field_row<R>(
    ui: &mut egui::Ui,
    label: &str,
    description: Option<&str>,
    add_control: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let inner = ui.horizontal(|ui| {
        field_label(ui, label);
        add_control(ui)
    });
    row_tooltip(ui, &inner.response, description);
    inner.inner
}

fn fmt_val(v: f32, integer: bool, precision: usize) -> String {
    if integer {
        format!("{:.0}", v)
    } else {
        format!("{:.prec$}", v, prec = precision)
    }
}

fn snap(v: f32, min: f32, max: f32, integer: bool) -> f32 {
    let c = v.clamp(min, max);
    if integer {
        c.round()
    } else {
        c
    }
}

// ── ParamSlider ───────────────────────────────────────────────────────────────

/// Numeric parameter slider: single-row layout.
///
///   [optional label]  [======bar======[handle]=======]  [text input]
///
/// The bar and text input share the row height (`INPUT_H`). The bar fills
/// whatever space is left after the optional label and the text input.
/// Label and input width are independently configurable via the builder.
/// Outcome of a [`ParamSlider::show`] frame. `changed_live` is true on
/// every frame the value moved (drag, click-bump, or text commit), so a
/// caller writes the model each frame and the handle tracks; `begin` and
/// `commit` bracket the edit for undo gating. Replaces the bare `Response`
/// whose deferred `changed()` was easy to misread as "nothing changed
/// during the drag" -- the source of the dead-slider bug.
pub(crate) struct SliderEdit {
    pub value: f32,
    pub begin: bool,
    pub changed_live: bool,
    pub commit: bool,
}

pub(crate) struct ParamSlider<'a> {
    value: &'a mut f32,
    min: f32,
    max: f32,
    precision: usize,
    integer: bool,
    label: Option<&'a str>,
    input_width: f32,
}

impl<'a> ParamSlider<'a> {
    pub fn new(value: &'a mut f32, min: f32, max: f32) -> Self {
        Self {
            value,
            min,
            max,
            precision: 3,
            integer: false,
            label: None,
            input_width: INPUT_W_DEFAULT,
        }
    }

    pub fn integer(mut self) -> Self {
        self.integer = true;
        self.precision = 0;
        self
    }

    #[allow(dead_code)]
    pub fn label(mut self, text: &'a str) -> Self {
        self.label = Some(text);
        self
    }

    #[allow(dead_code)]
    pub fn input_width(mut self, w: f32) -> Self {
        self.input_width = w;
        self
    }

    #[allow(dead_code)]
    pub fn precision(mut self, p: usize) -> Self {
        self.precision = p;
        self
    }
}

impl ParamSlider<'_> {
    /// Render the slider and report what the user did this frame. The only
    /// way to use a `ParamSlider` -- there is no `Widget` impl, so a caller
    /// cannot read a deferred `.changed()` and silently drop the drag.
    pub fn show(self, ui: &mut egui::Ui) -> SliderEdit {
        let entry = *self.value;
        let id = ui.next_auto_id();
        let avail_w = ui.available_width().max(60.0);

        let (outer_rect, _) =
            ui.allocate_exact_size(egui::vec2(avail_w, INPUT_H), egui::Sense::hover());

        // Label portion -- capped at 40% of available width so bar always gets space.
        let label_w = if self.label.is_some() {
            (avail_w * 0.40)
                .min(avail_w - self.input_width - BAR_INPUT_GAP - 30.0)
                .max(0.0)
        } else {
            0.0
        };
        let input_w = self
            .input_width
            .min(avail_w - label_w - BAR_INPUT_GAP)
            .max(20.0);
        let bar_w = (avail_w - label_w - input_w - BAR_INPUT_GAP).max(10.0);

        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(
                outer_rect.left() + label_w,
                outer_rect.center().y - BAR_H / 2.0,
            ),
            egui::vec2(bar_w, BAR_H),
        );
        let te_rect = egui::Rect::from_min_size(
            egui::pos2(outer_rect.right() - input_w, outer_rect.top()),
            egui::vec2(input_w, INPUT_H),
        );

        // ---- Optional label -----------------------------------------------
        if let Some(text) = self.label {
            let lc = ui.visuals().text_color();
            ui.painter_at(outer_rect).text(
                egui::pos2(outer_rect.left(), outer_rect.center().y),
                egui::Align2::LEFT_CENTER,
                text,
                egui::FontId::proportional(13.0),
                lc,
            );
        }

        // ---- Text input ---------------------------------------------------
        let te_id = id.with("te");
        let buf_id = id.with("buf");

        // Keep the buffer in sync with the live value while not editing.
        if !ui.memory(|m| m.has_focus(te_id)) {
            ui.data_mut(|d| {
                d.insert_temp::<String>(buf_id, fmt_val(*self.value, self.integer, self.precision))
            });
        }
        let mut buf: String = ui
            .data(|d| d.get_temp::<String>(buf_id))
            .unwrap_or_else(|| fmt_val(*self.value, self.integer, self.precision));

        let te_resp = ui.put(te_rect, egui::TextEdit::singleline(&mut buf).id(te_id));
        select_all_on_focus(ui, &te_resp, &buf);

        // Border painted after the TextEdit so it sits on top of egui's
        // own widget frame. Derived from visuals for light/dark correctness.
        let border_col = {
            let vis = ui.visuals();
            let towards = if vis.dark_mode {
                egui::Color32::WHITE
            } else {
                egui::Color32::BLACK
            };
            vis.window_fill().lerp_to_gamma(towards, 0.35)
        };
        ui.painter_at(te_rect).rect_stroke(
            te_rect,
            egui::CornerRadius::same(2),
            egui::Stroke::new(1.0, border_col),
            egui::StrokeKind::Inside,
        );
        ui.data_mut(|d| d.insert_temp::<String>(buf_id, buf.clone()));

        let mut changed = false;
        if te_resp.lost_focus() {
            if let Ok(parsed) = buf.trim().parse::<f32>() {
                *self.value = snap(parsed, self.min, self.max, self.integer);
                changed = true;
            }
            // Reformat to canonical representation after commit.
            ui.data_mut(|d| {
                d.insert_temp::<String>(buf_id, fmt_val(*self.value, self.integer, self.precision))
            });
        }

        // ---- Slider bar ---------------------------------------------------
        let range = self.max - self.min;
        let t = if range > 1e-9 {
            ((*self.value - self.min) / range).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Handle center; clamped so the handle never clips outside the bar.
        let handle_cx = (bar_rect.left() + t * bar_rect.width()).clamp(
            bar_rect.left() + HANDLE_W * 0.5,
            bar_rect.right() - HANDLE_W * 0.5,
        );
        let handle_rect = egui::Rect::from_center_size(
            egui::pos2(handle_cx, bar_rect.center().y),
            egui::vec2(HANDLE_W, BAR_H),
        );

        if ui.is_rect_visible(bar_rect) {
            let painter = ui.painter_at(bar_rect);
            let rounding = egui::CornerRadius::same(3);
            let vis = ui.visuals();
            let slider_bg = vis.widgets.inactive.bg_fill;
            let slider_fill = vis.selection.bg_fill;
            // Handle: dark blue at rest, full selection blue on hover.
            let handle_base = slider_fill.lerp_to_gamma(egui::Color32::BLACK, 0.30);
            let handle_hot = slider_fill;
            let is_handle_hovered = ui.rect_contains_pointer(handle_rect);

            painter.rect_filled(bar_rect, rounding, slider_bg);
            if t > 0.001 {
                let fill =
                    egui::Rect::from_min_max(bar_rect.min, egui::pos2(handle_cx, bar_rect.max.y));
                painter.rect_filled(fill, rounding, slider_fill);
            }
            let handle_col = if is_handle_hovered {
                handle_hot
            } else {
                handle_base
            };
            painter.rect_filled(handle_rect, egui::CornerRadius::same(2), handle_col);
            if is_handle_hovered {
                painter.rect_stroke(
                    handle_rect,
                    egui::CornerRadius::same(2),
                    egui::Stroke::new(1.0, handle_hot.lerp_to_gamma(egui::Color32::WHITE, 0.4)),
                    egui::StrokeKind::Outside,
                );
            }
        }

        let bar_resp = ui.interact(bar_rect, id.with("bar"), egui::Sense::click_and_drag());
        let drag_id = id.with("dh"); // whether drag started on the handle
        let was_dragging_id = id.with("wd"); // true on the frame drag ends

        if bar_resp.drag_started() {
            let origin = ui
                .input(|i| i.pointer.press_origin())
                .unwrap_or(bar_rect.center());
            ui.data_mut(|d| d.insert_temp::<bool>(drag_id, handle_rect.contains(origin)));
        }

        let drag_on_handle: bool = ui.data(|d| d.get_temp(drag_id)).unwrap_or(false);
        if bar_resp.dragged() && drag_on_handle {
            // Update the value every frame so the handle tracks the pointer,
            // but defer the `changed()` signal until the drag ends so callers
            // (e.g. graph re-evaluation) don't fire on every pixel of movement.
            let dx = bar_resp.drag_delta().x;
            *self.value = snap(
                *self.value + dx * range / bar_rect.width(),
                self.min,
                self.max,
                self.integer,
            );
            ui.data_mut(|d| d.insert_temp::<bool>(was_dragging_id, true));
        } else {
            let was_dragging: bool = ui.data(|d| d.get_temp(was_dragging_id)).unwrap_or(false);
            if was_dragging {
                changed = true;
                ui.data_mut(|d| d.insert_temp::<bool>(was_dragging_id, false));
            }
        }

        if bar_resp.clicked() {
            if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                let bump = if self.integer { 1.0 } else { range * 0.01 };
                if p.x < handle_rect.left() {
                    *self.value = snap(*self.value - bump, self.min, self.max, self.integer);
                    changed = true;
                } else if p.x > handle_rect.right() {
                    *self.value = snap(*self.value + bump, self.min, self.max, self.integer);
                    changed = true;
                }
            }
        }

        SliderEdit {
            value: *self.value,
            begin: bar_resp.drag_started() || te_resp.gained_focus(),
            // True every frame the value moved, so callers commit live and
            // the handle tracks; `changed` adds the drag-stop / text-commit edge.
            changed_live: changed || *self.value != entry,
            // End of an edit: drag stop, text blur, or a discrete click-bump
            // (`changed` while not dragging). Not mid-drag, so callers can
            // gate heavy work (undo, re-eval, port resize) on it once.
            commit: te_resp.lost_focus() || (changed && !bar_resp.dragged()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run<R>(body: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(400.0, 300.0),
        ));
        let mut body = Some(body);
        let mut out = None;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(b) = body.take() {
                    out = Some(b(ui));
                }
            });
        });
        out.unwrap()
    }

    // Headless smoke test: `show()` renders without panic and, with no
    // pointer/keyboard input, reports an inert frame (value unchanged, no
    // begin/changed/commit). Guards the render path; the commit/undo
    // behaviour is covered at the model layer (field_editor / node_field).
    #[test]
    fn param_slider_show_is_inert_without_input() {
        let e = run(|ui| {
            let mut v = 0.5_f32;
            ParamSlider::new(&mut v, 0.0, 1.0).show(ui)
        });
        assert_eq!(e.value, 0.5);
        assert!(!e.begin);
        assert!(!e.changed_live);
        assert!(!e.commit);
    }
}

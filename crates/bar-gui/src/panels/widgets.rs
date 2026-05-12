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

impl<'a> egui::Widget for ParamSlider<'a> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
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

        let mut resp = te_resp | bar_resp;
        if changed {
            resp.mark_changed();
        }
        resp
    }
}

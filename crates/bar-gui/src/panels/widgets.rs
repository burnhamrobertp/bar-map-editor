//! Reusable GUI widgets for the editor.

use eframe::egui;

use super::tokens;

const INPUT_H: f32 = 18.0;
const INPUT_W: f32 = 52.0;
const BAR_H: f32 = 8.0;
const V_GAP: f32 = 2.0;
const HANDLE_W: f32 = 8.0;

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

/// Numeric parameter slider: text input (right) + drag bar (below).
///
///   [          label spacing   | [text input] ]   <- INPUT_H
///   [=== filled ===[ handle ]==============]      <- BAR_H
pub(crate) struct ParamSlider<'a> {
    value: &'a mut f32,
    min: f32,
    max: f32,
    precision: usize,
    integer: bool,
}

impl<'a> ParamSlider<'a> {
    pub fn new(value: &'a mut f32, min: f32, max: f32) -> Self {
        Self {
            value,
            min,
            max,
            precision: 3,
            integer: false,
        }
    }

    pub fn integer(mut self) -> Self {
        self.integer = true;
        self.precision = 0;
        self
    }
}

impl<'a> egui::Widget for ParamSlider<'a> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let id = ui.next_auto_id();
        let avail_w = ui.available_width().max(60.0);
        let total_h = INPUT_H + V_GAP + BAR_H;

        // Reserve the full widget rect so the cursor advances past it.
        let (outer_rect, _) =
            ui.allocate_exact_size(egui::vec2(avail_w, total_h), egui::Sense::hover());

        // Sub-rects.
        let te_rect = egui::Rect::from_min_size(
            egui::pos2(outer_rect.right() - INPUT_W, outer_rect.top()),
            egui::vec2(INPUT_W, INPUT_H),
        );
        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(outer_rect.left(), outer_rect.top() + INPUT_H + V_GAP),
            egui::vec2(avail_w, BAR_H),
        );

        // ---- Text input -----------------------------------------------

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

        // ---- Slider bar -----------------------------------------------

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
            painter.rect_filled(bar_rect, rounding, tokens::SLIDER_BG);
            if t > 0.001 {
                let fill =
                    egui::Rect::from_min_max(bar_rect.min, egui::pos2(handle_cx, bar_rect.max.y));
                painter.rect_filled(fill, rounding, tokens::SLIDER_FILL);
            }
            let handle_col = if ui.rect_contains_pointer(handle_rect) {
                tokens::SLIDER_HANDLE_HOT
            } else {
                tokens::SLIDER_HANDLE
            };
            painter.rect_filled(handle_rect, egui::CornerRadius::same(2), handle_col);
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

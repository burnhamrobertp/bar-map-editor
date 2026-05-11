//! Reusable GUI widgets for the editor.

use eframe::egui;

use super::tokens;

/// Transient per-widget state stored in egui temp memory.
#[derive(Clone, Default)]
struct SliderState {
    /// `Some(s)` while the widget is in inline text-edit mode.
    edit_buf: Option<String>,
}

/// A styled horizontal slider for numeric node parameters.
///
/// Shows a filled bar with a handle; double-click enters inline text
/// edit mode. Drag anywhere on the bar to set the value.
///
/// Usage: `ui.add(ParamSlider::new(&mut val, 0.0, 1.0))`
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

    /// Snap to integer steps and display without decimals.
    pub fn integer(mut self) -> Self {
        self.integer = true;
        self.precision = 0;
        self
    }
}

impl<'a> egui::Widget for ParamSlider<'a> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let id = ui.next_auto_id();
        let height = ui.spacing().interact_size.y;
        let width = ui.available_width().max(40.0);

        // Check if we are in text-edit mode.
        let state: SliderState = ui.data(|d| d.get_temp(id)).unwrap_or_default();

        if let Some(mut buf) = state.edit_buf.clone() {
            // --- Text edit mode ---
            let text_id = id.with("edit");
            // Allocate space first so the widget occupies the right slot
            // in any surrounding Grid/Layout.
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
            let resp = ui.put(rect, egui::TextEdit::singleline(&mut buf).id(text_id));
            // Request focus on the first frame we enter edit mode.
            if state.edit_buf.as_deref() != Some(buf.as_str()) || !resp.has_focus() {
                ui.memory_mut(|m| m.request_focus(text_id));
            }

            let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
            let committed =
                !cancelled && (ui.input(|i| i.key_pressed(egui::Key::Enter)) || resp.lost_focus());

            if cancelled {
                ui.data_mut(|d| d.remove_temp::<SliderState>(id));
            } else if committed {
                if let Ok(v) = buf.trim().parse::<f32>() {
                    let clamped = v.clamp(self.min, self.max);
                    *self.value = if self.integer {
                        clamped.round()
                    } else {
                        clamped
                    };
                }
                ui.data_mut(|d| d.remove_temp::<SliderState>(id));
            } else {
                ui.data_mut(|d| {
                    d.insert_temp(
                        id,
                        SliderState {
                            edit_buf: Some(buf),
                        },
                    )
                });
            }
            resp
        } else {
            // --- Slider display mode ---
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());

            if ui.is_rect_visible(rect) {
                let painter = ui.painter_at(rect);
                let rounding = egui::CornerRadius::same(3);

                // Background
                painter.rect_filled(rect, rounding, tokens::SLIDER_BG);

                // Filled portion
                let range = self.max - self.min;
                let t = if range > 1e-9 {
                    ((*self.value - self.min) / range).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                if t > 0.001 {
                    let fill_rect = egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(rect.width() * t, rect.height()),
                    );
                    painter.rect_filled(fill_rect, rounding, tokens::SLIDER_FILL);
                }

                // Handle -- small vertical bar at the fill edge
                let handle_x = rect.left() + rect.width() * t;
                let handle_w = 3.0_f32;
                let handle_rect = egui::Rect::from_min_size(
                    egui::pos2(
                        (handle_x - handle_w * 0.5).clamp(rect.left(), rect.right() - handle_w),
                        rect.top() + 2.0,
                    ),
                    egui::vec2(handle_w, rect.height() - 4.0),
                );
                painter.rect_filled(
                    handle_rect,
                    egui::CornerRadius::same(1),
                    tokens::SLIDER_HANDLE,
                );

                // Value text centered on the bar
                let label = if self.integer {
                    format!("{:.0}", *self.value)
                } else {
                    format!("{:.prec$}", *self.value, prec = self.precision)
                };
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &label,
                    egui::FontId::proportional(11.0),
                    tokens::SLIDER_TEXT,
                );
            }

            // Drag -- move value proportionally to horizontal drag delta.
            if resp.dragged() {
                let delta = resp.drag_delta().x;
                let range = self.max - self.min;
                let step = range / rect.width();
                let new_val = (*self.value + delta * step).clamp(self.min, self.max);
                *self.value = if self.integer {
                    new_val.round()
                } else {
                    new_val
                };
            }

            // Double-click -> enter text edit mode.
            if resp.double_clicked() {
                let buf = if self.integer {
                    format!("{:.0}", *self.value)
                } else {
                    format!("{:.prec$}", *self.value, prec = self.precision)
                };
                ui.data_mut(|d| {
                    d.insert_temp(
                        id,
                        SliderState {
                            edit_buf: Some(buf),
                        },
                    )
                });
            }

            resp
        }
    }
}

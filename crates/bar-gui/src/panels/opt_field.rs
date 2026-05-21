//! `Option<T>`-aware field widgets used by the Map Settings / grass
//! editors.
//!
//! Every modelled mapinfo field is stored as `Option<T>` on the recipe
//! (`None` = "not in source mapinfo / fall through to engine default";
//! `Some(v)` = "user-explicit or source-declared value"). These widgets
//! preserve that distinction in the UI: a field whose value is `None`
//! renders as an empty input with the engine default surfaced as a
//! placeholder hint. Typing flips the field to `Some(parsed)`; clearing
//! the field flips it back to `None`. There is no slider for these
//! Option-aware widgets -- a slider unavoidably draws a thumb at SOME
//! position, which the user can't visually distinguish from "user set
//! to this value." Numeric text input keeps the distinction sharp.
//!
//! The bool widget renders as a three-way `ComboBox` (default / true /
//! false). The colour widget keeps a colour picker but hides the
//! swatch behind an Override checkbox while the value is `None`.

use eframe::egui;

use crate::panels::widgets::select_all_on_focus;

/// Format an `f32` the way mapinfo expects (no trailing zeros, no
/// scientific notation): mirrors `lua_table::fmt_f32` but lives in the
/// GUI crate so the editor doesn't depend on bar-engine internals.
fn fmt_f32(v: f32) -> String {
    let s = format!("{:.4}", v);
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// `Option<f32>` text-input row: label on the left, value on the right.
/// Empty input box when `value` is `None`; default surfaced as
/// placeholder text. Typing + losing focus commits to `Some(parsed)`;
/// clearing + losing focus commits to `None`. Returns true exactly when
/// the underlying value changed.
pub fn opt_f32_row(ui: &mut egui::Ui, label: &str, value: &mut Option<f32>, default: f32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let id = ui.id().with(("opt_f32", label));
            // Seed the per-frame text buffer from egui's temp store
            // when the user has been typing in this field; otherwise
            // mirror the current `value` (so external changes -- e.g.
            // a fresh project load -- propagate into the visible text
            // until the next time the user focuses the field).
            let stored: Option<String> = ui.data(|d| d.get_temp::<String>(id));
            let mut text = stored.unwrap_or_else(|| value.map(fmt_f32).unwrap_or_default());
            let hint = fmt_f32(default);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .hint_text(hint)
                    .desired_width(80.0)
                    .id(id),
            );
            select_all_on_focus(ui, &resp, &text);
            ui.data_mut(|d| d.insert_temp(id, text.clone()));
            if resp.lost_focus() {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    if value.is_some() {
                        *value = None;
                        changed = true;
                    }
                    ui.data_mut(|d| d.remove::<String>(id));
                } else if let Ok(parsed) = trimmed.parse::<f32>() {
                    if *value != Some(parsed) {
                        *value = Some(parsed);
                        changed = true;
                    }
                    // Canonicalise the visible text so future renders
                    // mirror the parsed form (e.g. "2." -> "2").
                    ui.data_mut(|d| d.insert_temp(id, fmt_f32(parsed)));
                }
                // Unparseable input: leave the recipe alone. The user's
                // text stays visible so they can fix it.
            }
        });
    });
    changed
}

/// `Option<u32>` companion -- same UX as [`opt_f32_row`].
pub fn opt_u32_row(ui: &mut egui::Ui, label: &str, value: &mut Option<u32>, default: u32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let id = ui.id().with(("opt_u32", label));
            let stored: Option<String> = ui.data(|d| d.get_temp::<String>(id));
            let mut text =
                stored.unwrap_or_else(|| value.map(|v| v.to_string()).unwrap_or_default());
            let hint = default.to_string();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .hint_text(hint)
                    .desired_width(80.0)
                    .id(id),
            );
            select_all_on_focus(ui, &resp, &text);
            ui.data_mut(|d| d.insert_temp(id, text.clone()));
            if resp.lost_focus() {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    if value.is_some() {
                        *value = None;
                        changed = true;
                    }
                    ui.data_mut(|d| d.remove::<String>(id));
                } else if let Ok(parsed) = trimmed.parse::<u32>() {
                    if *value != Some(parsed) {
                        *value = Some(parsed);
                        changed = true;
                    }
                    ui.data_mut(|d| d.insert_temp(id, parsed.to_string()));
                }
            }
        });
    });
    changed
}

/// `Option<[f32; 3]>` RGB colour picker row. While the value is `None`
/// the picker is hidden behind an Override checkbox; clicking
/// "Override" promotes the field to `Some(default)` and reveals the
/// picker (the user is then editing an explicit value). Unchecking
/// reverts to `None`.
pub fn opt_color_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<[f32; 3]>,
    default: [f32; 3],
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // The Override checkbox is the explicit/implicit toggle.
            // Toggling it on seeds the value from the engine default
            // so the swatch starts at a sensible position rather than
            // jumping from "no colour" to "black".
            let mut overridden = value.is_some();
            if ui
                .checkbox(&mut overridden, "")
                .on_hover_text("Override the engine default for this field.")
                .changed()
            {
                if overridden {
                    *value = Some(default);
                } else {
                    *value = None;
                }
                changed = true;
            }
            if let Some(c) = value {
                // Mapinfo colour triples are stored as sRGB-perceptual
                // values; egui's `color_edit_button_rgb` expects linear
                // RGB and sRGB-encodes the swatch on render. Decode for
                // the picker so the swatch matches the rendered colour;
                // re-encode after edit so the recipe keeps perceptual
                // values that round-trip cleanly.
                let mut linear = bar_render::color::srgb_to_linear_rgb(*c);
                if ui.color_edit_button_rgb(&mut linear).changed() {
                    *c = bar_render::color::linear_to_srgb_rgb(linear);
                    changed = true;
                }
            } else {
                ui.label(
                    egui::RichText::new(format!(
                        "{:.2}, {:.2}, {:.2}",
                        default[0], default[1], default[2]
                    ))
                    .weak()
                    .italics(),
                );
            }
        });
    });
    changed
}

/// `Option<bool>` tri-state row. Renders as a ComboBox with three
/// options: "(default <true|false>)" surfaces the engine default at the
/// top, then explicit "true" and "false". Picking the first sets
/// `None`, the others set `Some(true)` / `Some(false)`.
pub fn opt_bool_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<bool>,
    default: bool,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let display = match value {
                Some(true) => egui::RichText::new("true"),
                Some(false) => egui::RichText::new("false"),
                None => egui::RichText::new(default.to_string()).weak().italics(),
            };
            egui::ComboBox::from_id_salt(("opt_bool", label))
                .selected_text(display)
                .width(140.0)
                .show_ui(ui, |ui| {
                    let mut local = *value;
                    if ui
                        .selectable_value(
                            &mut local,
                            None,
                            egui::RichText::new(default.to_string()).weak().italics(),
                        )
                        .changed()
                    {
                        *value = None;
                        changed = true;
                    }
                    if ui
                        .selectable_value(&mut local, Some(true), "true")
                        .changed()
                    {
                        *value = Some(true);
                        changed = true;
                    }
                    if ui
                        .selectable_value(&mut local, Some(false), "false")
                        .changed()
                    {
                        *value = Some(false);
                        changed = true;
                    }
                });
        });
    });
    changed
}

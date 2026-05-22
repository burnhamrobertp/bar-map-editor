//! Water / Lava modal -- the engine treats `mapinfo.water.damage > 0`
//! as lava (no separate flag), so the user picks the mode here via a
//! title-bar toggle and BME tracks the choice explicitly through
//! `WaterSettings::is_lava`. The exporter forces `damage = 0` in
//! water mode regardless of stored value; the lava form requires
//! damage >= 1 and shows a curated subset of fields that still make
//! sense on lava (drops the water-only Fresnel / wave-normal /
//! shore-foam / refraction / caustics / specular / plane groups).

use bar_project::recipe_fields::{LAVA_SPECS, WATER_SPECS};
use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::{render_specs, FieldFindings};
use crate::panels::field_editor::scrollbar_clearance;

/// Lava-mode minimum damage. Anything below this rounds back to
/// water at runtime, so a fresh switch to lava with damage == 0
/// would put the user in a confusing "lava mode but exports as
/// water" state. Pre-snap to keep the form valid.
const LAVA_MIN_DAMAGE: f32 = 1.0;

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_water_editor {
        return;
    }
    egui::Window::new("Water / Lava")
        .id(egui::Id::new("water_editor_modal"))
        .title_bar(false)
        .resizable(true)
        .collapsible(false)
        .default_size([460.0, 520.0])
        .show(ctx, |ui| {
            draw_title_row(app, ui);
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    scrollbar_clearance(ui, |ui| {
                        let findings = FieldFindings::from(app.validation.findings());
                        let is_lava = app.map_settings().water.is_lava.unwrap_or(false);
                        if is_lava {
                            render_specs(ui, app, LAVA_SPECS, &findings);
                        } else {
                            render_specs(ui, app, WATER_SPECS, &findings);
                        }
                    });
                });
        });
}

/// Custom title bar: "Water [switch] Lava" with the inactive label
/// faded out. Clicking either label or the switch flips the mode.
/// Includes the close (X) button so the user can still dismiss the
/// modal without the default egui title bar.
fn draw_title_row(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    let is_lava = app.map_settings().water.is_lava.unwrap_or(false);
    ui.horizontal(|ui| {
        let water_resp = title_label(ui, "Water", !is_lava);
        let switch_resp = title_switch(ui, is_lava);
        let lava_resp = title_label(ui, "Lava", is_lava);

        let want_water = is_lava && (water_resp.clicked() || switch_resp.clicked());
        let want_lava = !is_lava && (lava_resp.clicked() || switch_resp.clicked());

        if want_water || want_lava {
            let snap = app.snapshot(if want_lava {
                "Switch to Lava"
            } else {
                "Switch to Water"
            });
            app.history.push(snap);
            let settings = app.map_settings_mut();
            settings.water.is_lava = Some(want_lava);
            if want_lava {
                // Snap damage up to the lava minimum so the form
                // lands on a valid configuration. Preserves higher
                // stored values.
                let current = settings.water.damage.unwrap_or(0.0);
                if current < LAVA_MIN_DAMAGE {
                    settings.water.damage = Some(LAVA_MIN_DAMAGE);
                }
            }
            app.mark_dirty();
        }

        ui.add_space(ui.available_width() - 22.0);
        if ui
            .add(egui::Button::new("X").small().frame(false))
            .clicked()
        {
            app.dialog.show_water_editor = false;
        }
    });
}

fn title_label(ui: &mut egui::Ui, text: &str, active: bool) -> egui::Response {
    let color = if active {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let rich = egui::RichText::new(text).color(color).size(15.0).strong();
    ui.add(egui::Label::new(rich).sense(egui::Sense::click()))
}

/// A simple two-state switch: a rounded rectangle with a knob that
/// slides between two positions. Knob on the left = water, knob on
/// the right = lava. Returns the response so the caller can detect
/// clicks.
fn title_switch(ui: &mut egui::Ui, is_lava: bool) -> egui::Response {
    let size = egui::vec2(34.0, 18.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let visuals = ui.visuals();
    let track = if is_lava {
        egui::Color32::from_rgb(168, 64, 40)
    } else {
        egui::Color32::from_rgb(50, 96, 140)
    };
    ui.painter().rect_filled(rect, size.y * 0.5, track);
    ui.painter().rect_stroke(
        rect,
        size.y * 0.5,
        egui::Stroke::new(1.0, visuals.weak_text_color()),
        egui::StrokeKind::Inside,
    );
    let knob_r = size.y * 0.5 - 2.0;
    let knob_x = if is_lava {
        rect.right() - knob_r - 2.0
    } else {
        rect.left() + knob_r + 2.0
    };
    let knob_centre = egui::pos2(knob_x, rect.center().y);
    ui.painter()
        .circle_filled(knob_centre, knob_r, egui::Color32::from_rgb(240, 240, 240));
    resp
}

//! Structured Map Info editor — the floating modal opened from
//! the toolbar's gear button. Six tabs (Identity, Dimensions,
//! Physics, Atmosphere, Lighting, Water) bind directly to the
//! corresponding fields on `RecipeMeta` / `MapSettings` /
//! `BarEditorApp`'s map dimensions and height range. Validation
//! findings are surfaced inline: tabs gain a coloured dot when
//! their section has any issues, individual fields get an
//! outlined border at the worst severity touching them.

use eframe::egui;
use std::collections::HashMap;

use crate::app::{BarEditorApp, MapInfoTab};
use crate::panels::tokens;
use crate::t;

/// Map-Settings validation findings keyed by (tab_id, field_id).
/// `tab_id` matches the lowercase form of `MapInfoTab` variants; `field_id`
/// matches the names tagged onto findings in `bar-project::validation`.
pub(crate) struct FieldFindings {
    by_field: HashMap<(String, String), bar_project::Severity>,
    by_tab: HashMap<String, bar_project::Severity>,
}

impl FieldFindings {
    pub(crate) fn from(findings: &[bar_project::Finding]) -> Self {
        let mut by_field: HashMap<(String, String), bar_project::Severity> = HashMap::new();
        let mut by_tab: HashMap<String, bar_project::Severity> = HashMap::new();
        for f in findings {
            let cat = f.category.clone();
            by_tab
                .entry(cat.clone())
                .and_modify(|s| *s = worst_severity(*s, f.severity))
                .or_insert(f.severity);
            if let Some(field) = f.field.as_deref() {
                by_field
                    .entry((cat, field.to_string()))
                    .and_modify(|s| *s = worst_severity(*s, f.severity))
                    .or_insert(f.severity);
            }
        }
        Self { by_field, by_tab }
    }

    pub(crate) fn tab(&self, tab: &str) -> Option<bar_project::Severity> {
        self.by_tab.get(tab).copied()
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

pub(crate) fn severity_color(sev: bar_project::Severity) -> egui::Color32 {
    match sev {
        bar_project::Severity::Error => tokens::SEVERITY_ERROR,
        bar_project::Severity::Warning => tokens::SEVERITY_WARN,
        bar_project::Severity::Info => tokens::SEVERITY_INFO,
    }
}

/// Wrap a row in a thin coloured outline whose colour matches the
/// finding's severity. No-op when `sev` is `None`.
fn outline_finding<R>(
    ui: &mut egui::Ui,
    sev: Option<bar_project::Severity>,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    match sev {
        Some(s) => {
            let color = severity_color(s);
            egui::Frame::default()
                .stroke(egui::Stroke::new(1.0, color))
                .corner_radius(2.0)
                .inner_margin(egui::Margin::symmetric(2, 1))
                .show(ui, body)
                .inner
        }
        None => body(ui),
    }
}

fn drag_f32(ui: &mut egui::Ui, label: &str, value: &mut f32, lo: f32, hi: f32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        if ui
            .add(crate::panels::widgets::ParamSlider::new(value, lo, hi))
            .changed()
        {
            changed = true;
        }
    });
    changed
}

fn drag_u32(ui: &mut egui::Ui, label: &str, value: &mut u32, lo: u32, hi: u32) -> bool {
    let mut changed = false;
    let mut vf = *value as f32;
    ui.horizontal(|ui| {
        ui.label(label);
        if ui
            .add(crate::panels::widgets::ParamSlider::new(&mut vf, lo as f32, hi as f32).integer())
            .changed()
        {
            *value = vf as u32;
            changed = true;
        }
    });
    changed
}

/// Empty-string -> `None` so the bundler-side fallback kicks in. The
/// placeholder hint communicates what that fallback will be.
fn edit_optional_string(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<String>,
    placeholder: &str,
) -> bool {
    let mut changed = false;
    let mut buf = value.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let edit = egui::TextEdit::singleline(&mut buf)
                .desired_width(220.0)
                .hint_text(placeholder);
            let edit_resp = ui.add(edit);
            crate::panels::widgets::select_all_on_focus(ui, &edit_resp, &buf);
            if edit_resp.changed() {
                let trimmed = buf.trim();
                let new_value = if trimmed.is_empty() {
                    None
                } else {
                    Some(buf.clone())
                };
                if &new_value != value {
                    *value = new_value;
                    changed = true;
                }
            }
        });
    });
    changed
}

fn color_rgb(ui: &mut egui::Ui, label: &str, value: &mut [f32; 3]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.color_edit_button_rgb(value).changed() {
                changed = true;
            }
        });
    });
    changed
}

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_mapinfo_editor {
        return;
    }
    let mut open = app.dialog.show_mapinfo_editor;
    let mut dirty = false;
    egui::Window::new(t!("editor.map_settings.title"))
        .open(&mut open)
        .resizable(true)
        .collapsible(false)
        .default_size([460.0, 520.0])
        .show(ctx, |ui| {
            // Validation findings index — used to decorate tabs
            // and outline individual fields the validators
            // flagged.
            let findings_index = FieldFindings::from(app.validation.findings());

            // Tab strip.
            let accent = ui.visuals().selection.bg_fill;
            let bg_layer =
                egui::LayerId::new(egui::Order::Background, ui.id().with("mapinfo_tab_bg"));
            let mut active_tab = app.mapinfo_tab_now();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let mut tab = |ui: &mut egui::Ui, variant: MapInfoTab, text: &str, tab_id: &str| {
                    let active = active_tab == variant;
                    let color = if active {
                        ui.visuals().strong_text_color()
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    let sev = findings_index.tab(tab_id);
                    let label_text = match sev {
                        Some(_) => format!("\u{25CF} {text}"),
                        None => text.to_string(),
                    };
                    let mut rich = egui::RichText::new(label_text).strong().color(color);
                    if let Some(s) = sev {
                        rich = rich.color(severity_color(s));
                    }
                    let resp = ui.add(egui::Label::new(rich).sense(egui::Sense::click()));
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        if !active {
                            ui.ctx().layer_painter(bg_layer).rect_filled(
                                resp.rect.expand2(egui::vec2(4.0, 2.0)),
                                3.0,
                                ui.visuals().widgets.hovered.bg_fill,
                            );
                        }
                    }
                    if active {
                        let r = resp.rect;
                        ui.painter().line_segment(
                            [
                                egui::pos2(r.left() - 2.0, r.bottom() + 3.0),
                                egui::pos2(r.right() + 2.0, r.bottom() + 3.0),
                            ],
                            egui::Stroke::new(2.0, accent),
                        );
                    }
                    if resp.clicked() {
                        active_tab = variant;
                    }
                };
                let identity = t!("editor.map_settings.tab.identity");
                let dimensions = t!("editor.map_settings.tab.dimensions");
                let physics = t!("editor.map_settings.tab.physics");
                let atmosphere = t!("editor.map_settings.tab.atmosphere");
                let lighting = t!("editor.map_settings.tab.lighting");
                let water = t!("editor.map_settings.tab.water");
                tab(ui, MapInfoTab::Identity, &identity, "identity");
                tab(ui, MapInfoTab::Dimensions, &dimensions, "dimensions");
                tab(ui, MapInfoTab::Physics, &physics, "physics");
                tab(ui, MapInfoTab::Atmosphere, &atmosphere, "atmosphere");
                tab(ui, MapInfoTab::Lighting, &lighting, "lighting");
                tab(ui, MapInfoTab::Water, &water, "water");
            });
            app.set_mapinfo_tab(active_tab);
            ui.add_space(8.0);

            // Active section.
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| match app.mapinfo_tab_now() {
                    MapInfoTab::Identity => {
                        let meta = app.recipe_meta_mut();
                        dirty |= edit_optional_string(
                            ui,
                            &t!("common.shortname"),
                            &mut meta.shortname,
                            "",
                        );
                        ui.horizontal(|ui| {
                            ui.label(t!("common.description"));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let edit = egui::TextEdit::singleline(&mut meta.description)
                                        .desired_width(220.0);
                                    let edit_resp = ui.add(edit);
                                    crate::panels::widgets::select_all_on_focus(
                                        ui,
                                        &edit_resp,
                                        &meta.description,
                                    );
                                    if edit_resp.changed() {
                                        dirty = true;
                                    }
                                },
                            );
                        });
                        dirty |=
                            edit_optional_string(ui, &t!("common.author"), &mut meta.author, "");
                        dirty |=
                            edit_optional_string(ui, &t!("common.version"), &mut meta.version, "");
                    }
                    MapInfoTab::Dimensions => {
                        let dim_finding_w = findings_index.field("dimensions", "width");
                        let dim_finding_d = findings_index.field("dimensions", "depth");
                        let dim_finding_min = findings_index.field("dimensions", "min_height");
                        let dim_finding_max = findings_index.field("dimensions", "max_height");
                        {
                            let (w, h) = app.map_dimensions_mut();
                            let dim_finding = dim_finding_w.or(dim_finding_d);
                            outline_finding(ui, dim_finding, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(t!("editor.map_settings.map_size_label"));
                                    // egui does not store the text itself in
                                    // memory between frames -- only cursor state.
                                    // Store the in-progress string in temp data
                                    // so it survives frame boundaries while the
                                    // field is focused.
                                    let wid = egui::Id::new("map_dim_w");
                                    let hid = egui::Id::new("map_dim_h");
                                    let wv = (*w).saturating_sub(1) / 64;
                                    let hv = (*h).saturating_sub(1) / 64;
                                    let mut ws: String = ui
                                        .data(|d| d.get_temp::<String>(wid))
                                        .unwrap_or_else(|| wv.to_string());
                                    let wr = ui.add_sized(
                                        [30.0, 18.0],
                                        egui::TextEdit::singleline(&mut ws).id(wid),
                                    );
                                    crate::panels::widgets::select_all_on_focus(ui, &wr, &ws);
                                    ui.data_mut(|d| d.insert_temp(wid, ws.clone()));
                                    if wr.lost_focus() {
                                        let nv = ws.trim().parse::<u32>()
                                            .map(|v| v.clamp(1, 512))
                                            .unwrap_or(wv);
                                        *w = nv * 64 + 1;
                                        if nv != wv { dirty = true; }
                                        ui.data_mut(|d| d.insert_temp(wid, nv.to_string()));
                                    }
                                    ui.label("x");
                                    let mut hs: String = ui
                                        .data(|d| d.get_temp::<String>(hid))
                                        .unwrap_or_else(|| hv.to_string());
                                    let hr = ui.add_sized(
                                        [30.0, 18.0],
                                        egui::TextEdit::singleline(&mut hs).id(hid),
                                    );
                                    crate::panels::widgets::select_all_on_focus(ui, &hr, &hs);
                                    ui.data_mut(|d| d.insert_temp(hid, hs.clone()));
                                    if hr.lost_focus() {
                                        let nv = hs.trim().parse::<u32>()
                                            .map(|v| v.clamp(1, 512))
                                            .unwrap_or(hv);
                                        *h = nv * 64 + 1;
                                        if nv != hv { dirty = true; }
                                        ui.data_mut(|d| d.insert_temp(hid, nv.to_string()));
                                    }
                                });
                            });
                        }
                        let (mn, mx) = app.map_height_range_mut();
                        dirty |= outline_finding(ui, dim_finding_min, |ui| {
                            drag_f32(ui, "Min height", mn, -2000.0, 4000.0)
                        });
                        dirty |= outline_finding(ui, dim_finding_max, |ui| {
                            drag_f32(ui, "Max height", mx, -2000.0, 4000.0)
                        });
                    }
                    MapInfoTab::Physics => {
                        let f_grav = findings_index.field("physics", "gravity");
                        let f_hard = findings_index.field("physics", "map_hardness");
                        let f_tide = findings_index.field("physics", "tidal_strength");
                        let f_metal = findings_index.field("physics", "max_metal");
                        let f_extr = findings_index.field("physics", "extractor_radius");
                        let f_water = findings_index.field("physics", "water_damage");
                        let s = app.map_settings_mut();
                        dirty |= outline_finding(ui, f_grav, |ui| {
                            drag_f32(ui, "Gravity", &mut s.gravity, 0.0, 1000.0)
                        });
                        dirty |= outline_finding(ui, f_hard, |ui| {
                            drag_u32(ui, "Map hardness", &mut s.map_hardness, 0, 1000)
                        });
                        dirty |= outline_finding(ui, f_tide, |ui| {
                            drag_f32(ui, "Tidal strength", &mut s.tidal_strength, 0.0, 100.0)
                        });
                        dirty |= outline_finding(ui, f_metal, |ui| {
                            drag_f32(ui, "Max metal", &mut s.max_metal, 0.0, 10.0)
                        });
                        dirty |= outline_finding(ui, f_extr, |ui| {
                            drag_f32(ui, "Extractor radius", &mut s.extractor_radius, 0.0, 500.0)
                        });
                        dirty |= outline_finding(ui, f_water, |ui| {
                            drag_f32(ui, "Water damage / sec", &mut s.water_damage, 0.0, 1000.0)
                        });
                        if ui
                            .checkbox(&mut s.deformable, "Deformable terrain")
                            .changed()
                        {
                            dirty = true;
                        }
                        if ui.checkbox(&mut s.void_water, "Void water").changed() {
                            dirty = true;
                        }
                        if ui.checkbox(&mut s.void_ground, "Void ground").changed() {
                            dirty = true;
                        }
                    }
                    MapInfoTab::Atmosphere => {
                        let f_min = findings_index.field("atmosphere", "min_wind");
                        let f_max = findings_index.field("atmosphere", "max_wind");
                        let f_fs = findings_index.field("atmosphere", "fog_start");
                        let f_fe = findings_index.field("atmosphere", "fog_end");
                        let f_fc = findings_index.field("atmosphere", "fog_color");
                        let atm = &mut app.map_settings_mut().atmosphere;
                        dirty |= outline_finding(ui, f_min, |ui| {
                            drag_f32(ui, "Min wind", &mut atm.min_wind, 0.0, 200.0)
                        });
                        dirty |= outline_finding(ui, f_max, |ui| {
                            drag_f32(ui, "Max wind", &mut atm.max_wind, 0.0, 200.0)
                        });
                        dirty |= outline_finding(ui, f_fs, |ui| {
                            drag_f32(ui, "Fog start (0-1)", &mut atm.fog_start, 0.0, 1.0)
                        });
                        dirty |= outline_finding(ui, f_fe, |ui| {
                            drag_f32(ui, "Fog end (0-1)", &mut atm.fog_end, 0.0, 1.0)
                        });
                        dirty |= outline_finding(ui, f_fc, |ui| {
                            color_rgb(ui, "Fog colour", &mut atm.fog_color)
                        });
                    }
                    MapInfoTab::Lighting => {
                        let f_sun = findings_index.field("lighting", "sun_dir");
                        let f_amb = findings_index.field("lighting", "ground_ambient");
                        let f_diff = findings_index.field("lighting", "ground_diffuse");
                        let f_spec = findings_index.field("lighting", "ground_specular");
                        let f_se = findings_index.field("lighting", "spec_exponent");
                        let lit = &mut app.map_settings_mut().lighting;
                        dirty |= outline_finding(ui, f_sun, |ui| {
                            drag_f32(ui, "Sun X", &mut lit.sun_dir[0], -1.0, 1.0)
                        });
                        dirty |= drag_f32(ui, "Sun Y", &mut lit.sun_dir[1], -1.0, 1.0);
                        dirty |= drag_f32(ui, "Sun Z", &mut lit.sun_dir[2], -1.0, 1.0);
                        dirty |= outline_finding(ui, f_amb, |ui| {
                            color_rgb(ui, "Ground ambient", &mut lit.ground_ambient)
                        });
                        dirty |= outline_finding(ui, f_diff, |ui| {
                            color_rgb(ui, "Ground diffuse", &mut lit.ground_diffuse)
                        });
                        dirty |= outline_finding(ui, f_spec, |ui| {
                            color_rgb(ui, "Ground specular", &mut lit.ground_specular)
                        });
                        dirty |= outline_finding(ui, f_se, |ui| {
                            drag_f32(ui, "Specular exponent", &mut lit.spec_exponent, 1.0, 200.0)
                        });
                    }
                    MapInfoTab::Water => {
                        let f_dmg = findings_index.field("water", "damage");
                        let f_abs = findings_index.field("water", "absorb");
                        let w = &mut app.map_settings_mut().water;
                        dirty |= outline_finding(ui, f_dmg, |ui| {
                            drag_f32(ui, "Water damage / sec", &mut w.damage, 0.0, 1000.0)
                        });
                        dirty |= outline_finding(ui, f_abs, |ui| {
                            color_rgb(ui, "Absorb (per RGB)", &mut w.absorb)
                        });
                    }
                });
        });
    app.dialog.show_mapinfo_editor = open;
    if dirty {
        app.mark_dirty();
    }
}

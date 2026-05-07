//! Structured Map Info editor — the floating modal opened from
//! the toolbar's gear button. Six tabs (Identity, Dimensions,
//! Physics, Atmosphere, Lighting, Water) bind directly to the
//! corresponding fields on `RecipeMeta` / `MapSettings` /
//! `BarEditorApp`'s map dimensions and height range. Validation
//! findings are surfaced inline: tabs gain a coloured dot when
//! their section has any issues, individual fields get an
//! outlined border at the worst severity touching them.

use eframe::egui;

use crate::app::{
    color_rgb, drag_f32, drag_u32, edit_optional_string, outline_finding, severity_color,
    BarEditorApp, FieldFindings, MapInfoTab,
};
use crate::t;

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog_show_mapinfo_editor() {
        return;
    }
    let mut open = app.dialog_show_mapinfo_editor();
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
            let findings_index = FieldFindings::from(app.validation_findings());

            // Tab strip.
            let accent = ui.visuals().selection.bg_fill;
            let bg_layer = egui::LayerId::new(
                egui::Order::Background,
                ui.id().with("mapinfo_tab_bg"),
            );
            let mut active_tab = app.mapinfo_tab_now();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let mut tab = |ui: &mut egui::Ui,
                               variant: MapInfoTab,
                               text: &str,
                               tab_id: &str| {
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
            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| match app.mapinfo_tab_now() {
                MapInfoTab::Identity => {
                    let meta = app.recipe_meta_mut();
                    dirty |= edit_optional_string(ui, &t!("common.shortname"), &mut meta.shortname, "");
                    ui.horizontal(|ui| {
                        ui.label(t!("common.description"));
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let edit = egui::TextEdit::singleline(&mut meta.description)
                                    .desired_width(220.0);
                                if ui.add(edit).changed() {
                                    dirty = true;
                                }
                            },
                        );
                    });
                    dirty |= edit_optional_string(ui, &t!("common.author"), &mut meta.author, "");
                    dirty |= edit_optional_string(ui, &t!("common.version"), &mut meta.version, "");
                }
                MapInfoTab::Dimensions => {
                    let dim_finding_w = findings_index.field("dimensions", "width");
                    let dim_finding_d = findings_index.field("dimensions", "depth");
                    let dim_finding_min = findings_index.field("dimensions", "min_height");
                    let dim_finding_max = findings_index.field("dimensions", "max_height");
                    {
                        let (w, h) = app.map_dimensions_mut();
                        outline_finding(ui, dim_finding_w, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(t!("common.width"));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let mut wv = *w as i32;
                                        if ui
                                            .add(egui::DragValue::new(&mut wv).range(64..=16384).speed(64.0))
                                            .changed()
                                        {
                                            *w = wv as u32;
                                            dirty = true;
                                        }
                                    },
                                );
                            });
                        });
                        outline_finding(ui, dim_finding_d, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(t!("common.depth"));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let mut hv = *h as i32;
                                        if ui
                                            .add(egui::DragValue::new(&mut hv).range(64..=16384).speed(64.0))
                                            .changed()
                                        {
                                            *h = hv as u32;
                                            dirty = true;
                                        }
                                    },
                                );
                            });
                        });
                    }
                    let (mn, mx) = app.map_height_range_mut();
                    dirty |= outline_finding(ui, dim_finding_min, |ui| {
                        drag_f32(ui, "Min height (elmos)", mn, -2000.0, 4000.0, 1.0)
                    });
                    dirty |= outline_finding(ui, dim_finding_max, |ui| {
                        drag_f32(ui, "Max height (elmos)", mx, -2000.0, 4000.0, 1.0)
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
                        drag_f32(ui, "Gravity", &mut s.gravity, 0.0, 1000.0, 1.0)
                    });
                    dirty |= outline_finding(ui, f_hard, |ui| {
                        drag_u32(ui, "Map hardness", &mut s.map_hardness, 0, 1000)
                    });
                    dirty |= outline_finding(ui, f_tide, |ui| {
                        drag_f32(ui, "Tidal strength", &mut s.tidal_strength, 0.0, 100.0, 0.5)
                    });
                    dirty |= outline_finding(ui, f_metal, |ui| {
                        drag_f32(ui, "Max metal", &mut s.max_metal, 0.0, 10.0, 0.05)
                    });
                    dirty |= outline_finding(ui, f_extr, |ui| {
                        drag_f32(ui, "Extractor radius", &mut s.extractor_radius, 0.0, 500.0, 1.0)
                    });
                    dirty |= outline_finding(ui, f_water, |ui| {
                        drag_f32(ui, "Water damage / sec", &mut s.water_damage, 0.0, 1000.0, 1.0)
                    });
                    if ui.checkbox(&mut s.deformable, "Deformable terrain").changed() {
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
                        drag_f32(ui, "Min wind", &mut atm.min_wind, 0.0, 200.0, 0.5)
                    });
                    dirty |= outline_finding(ui, f_max, |ui| {
                        drag_f32(ui, "Max wind", &mut atm.max_wind, 0.0, 200.0, 0.5)
                    });
                    dirty |= outline_finding(ui, f_fs, |ui| {
                        drag_f32(ui, "Fog start (0-1)", &mut atm.fog_start, 0.0, 1.0, 0.01)
                    });
                    dirty |= outline_finding(ui, f_fe, |ui| {
                        drag_f32(ui, "Fog end (0-1)", &mut atm.fog_end, 0.0, 1.0, 0.01)
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
                        drag_f32(ui, "Sun X", &mut lit.sun_dir[0], -1.0, 1.0, 0.01)
                    });
                    dirty |= drag_f32(ui, "Sun Y", &mut lit.sun_dir[1], -1.0, 1.0, 0.01);
                    dirty |= drag_f32(ui, "Sun Z", &mut lit.sun_dir[2], -1.0, 1.0, 0.01);
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
                        drag_f32(ui, "Specular exponent", &mut lit.spec_exponent, 1.0, 200.0, 0.5)
                    });
                }
                MapInfoTab::Water => {
                    let f_dmg = findings_index.field("water", "damage");
                    let f_abs = findings_index.field("water", "absorb");
                    let w = &mut app.map_settings_mut().water;
                    dirty |= outline_finding(ui, f_dmg, |ui| {
                        drag_f32(ui, "Water damage / sec", &mut w.damage, 0.0, 1000.0, 1.0)
                    });
                    dirty |= outline_finding(ui, f_abs, |ui| {
                        color_rgb(ui, "Absorb (per RGB)", &mut w.absorb)
                    });
                }
            });
        });
    app.set_dialog_show_mapinfo_editor(open);
    if dirty {
        app.mark_dirty();
    }
}

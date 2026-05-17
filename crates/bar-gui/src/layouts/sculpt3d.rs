//! Sculpt3D layout -- left feature panel + central 3D viewport.
//!
//! Currently scoped to feature placement only. The brush / sculpt
//! flow (FC paint-layer composite, live-paint buffers, layer
//! selection) is on hold -- the Composition Layers section that
//! would expose it is commented out below, which makes the
//! conditional brush-tool palette unreachable. The dispatch wiring
//! and paint-state types stay in place so the on-hold subsystem can
//! be brought back without re-threading anything.
//!
//! The central panel is left unclaimed here -- bar-app draws the 3D
//! viewport there after this function returns.

use eframe::egui;
use eframe::egui::Color32;

use crate::app::{BarEditorApp, BrushTool};

/// Draw the Sculpt3D layout.
pub fn draw(app: &mut BarEditorApp, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    egui::SidePanel::left("sculpt3d_layers")
        .min_width(220.0)
        .max_width(300.0)
        .show(ctx, |ui| {
            draw_layer_panel(app, ui);
        });
    // Central panel is left unclaimed -- bar-app fills it with the 3D viewport.
}

fn draw_layer_panel(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    // Features pseudo-layer -- always present at the top of the
    // sculpt-side list. Selecting it switches the brush to Pointer
    // (no sculpting) and clears any FC layer selection.
    let features_selected = app.paint.brush.tool == BrushTool::Pointer;
    ui.add_space(4.0);
    if ui
        .horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), 4.0, Color32::from_rgb(255, 165, 60));
            ui.selectable_label(features_selected, "Features").clicked()
        })
        .inner
    {
        app.paint.brush.tool = BrushTool::Pointer;
        app.paint.selected_sculpt_layer = None;
        app.paint.selected_fc_layer = None;
    }

    // --- COMPOSITION LAYERS (on hold) ---
    // The FC paint-layer subsystem is paused; rendering this section
    // would expose entry points into the broken brush flow. The block
    // is commented out rather than deleted so the on-hold subsystem
    // can be reattached without re-threading the UI. With this block
    // gone, `paint.selected_fc_layer` stays None and `brush.tool`
    // stays Pointer, which short-circuits both the conditional
    // brush-tool palette below and the sculpt-dab path in
    // `viewport.rs::handle_camera_input`.
    /*
    ui.add_space(8.0);
    ui.strong("Layers");
    ui.separator();
    for kind in [
        crate::FCLayerKind::Heightmap,
        crate::FCLayerKind::Color,
        crate::FCLayerKind::Metalmap,
        crate::FCLayerKind::Typemap,
    ] {
        let selected = app.paint.selected_fc_layer == Some(kind);
        if ui.selectable_label(selected, kind.label()).clicked() {
            app.paint.selected_fc_layer = Some(kind);
            app.paint.selected_sculpt_layer = None;
            if kind == crate::FCLayerKind::Heightmap
                && matches!(app.paint.brush.tool, BrushTool::Pointer)
            {
                app.paint.brush.tool = BrushTool::Raise;
            }
            if kind != crate::FCLayerKind::Heightmap
                && matches!(app.paint.brush.tool, BrushTool::Pointer)
            {
                app.paint.brush.tool = BrushTool::Raise;
            }
        }
    }

    ui.add_space(8.0);
    ui.separator();
    */

    if app.paint.brush.tool == BrushTool::Pointer {
        draw_features_panel(app, ui);
    } else {
        // --- TOOLS section ---
        ui.strong("Tools");
        ui.add_space(4.0);

        let fc_kind = app.paint.selected_fc_layer;
        let has_terrain = app.paint.heightmap.is_some();

        match fc_kind {
            Some(crate::FCLayerKind::Heightmap) => {
                ui.horizontal_wrapped(|ui| {
                    let cur = app.paint.brush.tool;
                    for tool in [
                        BrushTool::Raise,
                        BrushTool::Lower,
                        BrushTool::Smooth,
                        BrushTool::Flatten,
                    ] {
                        if ui.selectable_label(cur == tool, tool.label()).clicked() {
                            app.paint.brush.tool = tool;
                        }
                    }
                });
            }
            Some(crate::FCLayerKind::Color) => {
                // Color layer: stamp the picked colour at brush
                // radius. No tool variants -- the only operation is
                // "paint this colour".
                ui.horizontal(|ui| {
                    ui.label("Colour");
                    let [r, g, b] = app.paint.brush.color_rgb;
                    let mut c = egui::Color32::from_rgb(r, g, b);
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        app.paint.brush.color_rgb = [c.r(), c.g(), c.b()];
                    }
                });
            }
            Some(kind @ (crate::FCLayerKind::Metalmap | crate::FCLayerKind::Typemap)) => {
                // Value-stamp kinds: pick a value in [0, 1], stamp
                // touched pixels with it. (A proper terrain-type-ID
                // picker for typemap is a UX follow-up.)
                let label = if kind == crate::FCLayerKind::Metalmap {
                    "Metal density"
                } else {
                    "Type ID (0..1)"
                };
                ui.horizontal(|ui| {
                    ui.label(label);
                    ui.add(egui::Slider::new(
                        &mut app.paint.brush.paint_value,
                        0.0..=1.0,
                    ));
                });
            }
            None => {
                ui.weak("Select a Composition layer to paint.");
            }
        }
        if fc_kind.is_some() && !has_terrain {
            draw_no_terrain_hint(app, ui);
        }

        ui.add_space(8.0);
        ui.separator();

        // --- BRUSH sliders ---
        ui.strong("Brush");
        ui.add_space(4.0);
        egui::Grid::new("sculpt_brush_params")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.label("Radius");
                ui.add(crate::panels::widgets::ParamSlider::new(
                    &mut app.paint.brush.radius_px,
                    0.5,
                    96.0,
                ));
                ui.end_row();
                ui.label("Strength");
                ui.add(crate::panels::widgets::ParamSlider::new(
                    &mut app.paint.brush.strength,
                    0.001,
                    0.2,
                ));
                ui.end_row();
                ui.label("Falloff");
                ui.add(crate::panels::widgets::ParamSlider::new(
                    &mut app.paint.brush.falloff,
                    0.5,
                    4.0,
                ));
                ui.end_row();
            });
    }
}

/// Full features panel: library (type picker) + selected feature info.
/// Only shown when the Features pseudo-layer is active.
fn draw_features_panel(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    // Selected feature info -- shown at the top so it's always visible.
    if let Some(idx) = app.map.selected_feature_idx {
        if let Some(f) = app.map.features.get(idx) {
            let ftype = f.feature_type.clone();
            let fx = f.x;
            let fy = f.y;
            let fz = f.z;
            let fangle = f.angle;
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(30, 30, 40))
                .corner_radius(4.0)
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong("Selected Feature");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button("Delete")
                                .on_hover_text("Remove this feature (Del)")
                                .clicked()
                            {
                                app.push_undo("Delete feature");
                                app.map.features.remove(idx);
                                app.map.selected_feature_idx = None;
                                app.map.features_placement_dirty = true;
                            }
                        });
                    });
                    egui::Grid::new("feature_info_grid")
                        .num_columns(2)
                        .spacing([8.0, 2.0])
                        .show(ui, |ui| {
                            ui.weak("Type");
                            ui.label(&ftype);
                            ui.end_row();
                            ui.weak("Position");
                            ui.label(format!("{:.1}, {:.1}, {:.1}", fx, fy, fz));
                            ui.end_row();
                            ui.weak("Angle");
                            ui.label(format!("{:.1} deg", fangle));
                            ui.end_row();
                            ui.weak("Index");
                            ui.label(format!("{} / {}", idx, app.map.features.len()));
                            ui.end_row();
                        });

                    // Per-feature in-engine light info. BAR's deferred
                    // rendering widget attaches point lights to certain
                    // feature defs (crystals etc.) -- BME shows a
                    // marker cube at each light position while this
                    // feature is selected and surfaces the same data
                    // textually here. The actual lighting effect is a
                    // game-runtime cosmetic, not rendered by BME.
                    let lights = bar_render::lights_for_feature_def(&ftype);
                    if !lights.is_empty() {
                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(4.0);
                        ui.strong("Lights");
                        ui.label(
                            "Coloured markers above the feature show where BAR will \
                             attach point lights at runtime. BME doesn't render the \
                             actual glow.",
                        );
                        ui.add_space(4.0);
                        for (li, light) in lights.iter().enumerate() {
                            if lights.len() > 1 {
                                ui.weak(format!("Light {}", li + 1));
                            }
                            egui::Grid::new(format!("feature_light_info_{}", li))
                                .num_columns(2)
                                .spacing([8.0, 2.0])
                                .show(ui, |ui| {
                                    ui.weak("Colour");
                                    ui.horizontal(|ui| {
                                        let swatch = egui::Color32::from_rgb(
                                            (light.color[0].clamp(0.0, 1.0) * 255.0) as u8,
                                            (light.color[1].clamp(0.0, 1.0) * 255.0) as u8,
                                            (light.color[2].clamp(0.0, 1.0) * 255.0) as u8,
                                        );
                                        let (rect, _) = ui.allocate_exact_size(
                                            egui::vec2(14.0, 14.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().rect_filled(rect, 2.0, swatch);
                                        ui.label(format!(
                                            "{:.2}, {:.2}, {:.2}",
                                            light.color[0], light.color[1], light.color[2]
                                        ));
                                    });
                                    ui.end_row();
                                    ui.weak("Radius");
                                    ui.label(format!("{:.0} elmos", light.radius));
                                    ui.end_row();
                                    ui.weak("Offset");
                                    ui.label(format!(
                                        "{:.0}, {:.0}, {:.0} elmos",
                                        light.offset[0], light.offset[1], light.offset[2]
                                    ));
                                    ui.end_row();
                                    ui.weak("Intensity");
                                    ui.label(format!("{:.2}", light.intensity));
                                    ui.end_row();
                                });
                            if li + 1 < lights.len() {
                                ui.add_space(4.0);
                            }
                        }
                    }
                });
            ui.add_space(4.0);
            ui.separator();
        }
    }

    // Feature library header.
    ui.horizontal(|ui| {
        ui.strong("Library");
        if app.selected_feature_type.is_some() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("x")
                    .on_hover_text("Cancel placement")
                    .clicked()
                {
                    app.selected_feature_type = None;
                }
            });
        }
    });
    ui.add_space(4.0);

    if app.feature_palette_names.is_empty() {
        ui.weak("No feature catalog loaded.");
        ui.weak("Set a game archive in Preferences.");
        return;
    }

    if let Some(ref sel) = app.selected_feature_type.clone() {
        ui.label(format!("Placing: {sel}"));
        ui.weak("Click terrain to place. Esc to cancel.");
        ui.add_space(4.0);
    } else {
        ui.weak("Click terrain to select. Del to remove.");
        ui.add_space(4.0);
    }

    egui::ScrollArea::vertical()
        .id_salt("feature_palette_scroll")
        .max_height(200.0)
        .show(ui, |ui| {
            let names = app.feature_palette_names.clone();
            for name in &names {
                let selected = app.selected_feature_type.as_deref() == Some(name.as_str());
                if ui.selectable_label(selected, name).clicked() {
                    app.selected_feature_type = if selected { None } else { Some(name.clone()) };
                }
            }
        });
}

fn draw_no_terrain_hint(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.weak("No terrain loaded --");
        if ui.small_button("Run preview").clicked() {
            app.preview.run_requested = true;
        }
    });
}

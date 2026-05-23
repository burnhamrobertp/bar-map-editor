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
use crate::t;

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
            ui.selectable_label(features_selected, t!("editor.sculpt3d.features_layer"))
                .clicked()
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
        crate::panels::feature_library::draw(app, ui);
    } else {
        // --- TOOLS section ---
        ui.strong(t!("editor.sculpt3d.tools_heading"));
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
                    ui.label(t!("common.colour"));
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
                    t!("editor.sculpt3d.metal_density")
                } else {
                    t!("editor.sculpt3d.type_id")
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
                ui.weak(t!("editor.sculpt3d.select_layer_hint"));
            }
        }
        if fc_kind.is_some() && !has_terrain {
            draw_no_terrain_hint(app, ui);
        }

        ui.add_space(8.0);
        ui.separator();

        // --- BRUSH sliders ---
        ui.strong(t!("editor.sculpt3d.brush_heading"));
        ui.add_space(4.0);
        egui::Grid::new("sculpt_brush_params")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.label(t!("editor.inspector.brush_radius"));
                ui.add(crate::panels::widgets::ParamSlider::new(
                    &mut app.paint.brush.radius_px,
                    0.5,
                    96.0,
                ));
                ui.end_row();
                ui.label(t!("editor.inspector.brush_strength"));
                ui.add(crate::panels::widgets::ParamSlider::new(
                    &mut app.paint.brush.strength,
                    0.001,
                    0.2,
                ));
                ui.end_row();
                ui.label(t!("editor.inspector.brush_falloff"));
                ui.add(crate::panels::widgets::ParamSlider::new(
                    &mut app.paint.brush.falloff,
                    0.5,
                    4.0,
                ));
                ui.end_row();
            });
    }
}

fn draw_no_terrain_hint(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.weak(t!("editor.sculpt3d.no_terrain"));
        if ui.small_button(t!("editor.sculpt3d.run_preview")).clicked() {
            app.preview.run_requested = true;
        }
    });
}

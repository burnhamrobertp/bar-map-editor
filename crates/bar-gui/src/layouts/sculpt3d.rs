//! Sculpt3D layout — full-width 3D viewport + brush controls.
//!
//! Brush strokes write directly to `app.paint.sculpt` (the project-level
//! `SculptState`). The export pipeline merges those layers onto graph output
//! at bundle time via `apply_sculpt_record` in `bar-engine/bundler.rs`.

use eframe::egui;

use crate::app::{BarEditorApp, BrushTarget, BrushTool};

/// Draw the Sculpt3D layout.
///
/// The shell (menu bar, status bar, action bar, modals) is drawn by
/// `dispatch::draw_active` before this is called. This function owns
/// only the brush-controls side panel and the viewport central panel.
pub fn draw(app: &mut BarEditorApp, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    // Right panel: brush controls + layer selector.
    egui::SidePanel::right("sculpt3d_controls")
        .min_width(200.0)
        .max_width(280.0)
        .show(ctx, |ui| {
            ui.add_space(4.0);
            ui.strong("Brush");
            ui.separator();

            // Layer selector.
            ui.label("Layer");
            ui.horizontal(|ui| {
                for target in [
                    BrushTarget::Heightmap,
                    BrushTarget::Color,
                    BrushTarget::Metalmap,
                    BrushTarget::Typemap,
                ] {
                    ui.selectable_value(
                        &mut app.paint.brush.target,
                        target,
                        target.label(),
                    );
                }
            });
            ui.add_space(4.0);

            // Tool selector.
            ui.label("Tool");
            ui.horizontal(|ui| {
                for tool in [
                    BrushTool::Raise,
                    BrushTool::Lower,
                    BrushTool::Smooth,
                    BrushTool::Flatten,
                ] {
                    ui.selectable_value(&mut app.paint.brush.tool, tool, tool.label());
                }
            });
            ui.add_space(4.0);

            // Brush sliders.
            ui.add(
                egui::Slider::new(&mut app.paint.brush.radius_px, 0.5..=96.0)
                    .text("Radius")
                    .logarithmic(true)
                    .clamping(egui::SliderClamping::Always),
            );
            ui.add(
                egui::Slider::new(&mut app.paint.brush.strength, 0.001..=0.2)
                    .text("Strength")
                    .clamping(egui::SliderClamping::Always)
                    .logarithmic(true),
            );
            ui.add(
                egui::Slider::new(&mut app.paint.brush.falloff, 0.5..=4.0)
                    .text("Falloff")
                    .clamping(egui::SliderClamping::Always),
            );

            // Color picker (shown only for Color target).
            if app.paint.brush.target == BrushTarget::Color {
                ui.add_space(4.0);
                ui.label("Colour");
                let [r, g, b] = app.paint.brush.color_rgb;
                let mut color32 = egui::Color32::from_rgb(r, g, b);
                if ui
                    .color_edit_button_srgba(&mut color32)
                    .changed()
                {
                    app.paint.brush.color_rgb = [
                        color32.r(),
                        color32.g(),
                        color32.b(),
                    ];
                }
            }

            // Value slider for metal/type.
            if matches!(
                app.paint.brush.target,
                BrushTarget::Metalmap | BrushTarget::Typemap
            ) {
                ui.add_space(4.0);
                ui.add(
                    egui::Slider::new(&mut app.paint.brush.paint_value, 0.0..=1.0)
                        .text("Value")
                        .clamping(egui::SliderClamping::Always),
                );
            }

            ui.add_space(8.0);
            ui.separator();

            // Sculpt layer status.
            ui.strong("Layers");
            let sculpt = &app.paint.sculpt;
            let present = |opt: bool| if opt { "filled" } else { "empty" };
            ui.weak(format!("Height: {}", present(sculpt.height_delta.is_some())));
            ui.weak(format!("Colour: {}", present(sculpt.texture_overlay.is_some())));
            ui.weak(format!("Metal:  {}", present(sculpt.metal_overlay.is_some())));
            ui.weak(format!("Type:   {}", present(sculpt.type_overlay.is_some())));

            ui.add_space(8.0);
            if ui
                .add_enabled(
                    app.paint.sculpt.height_delta.is_some()
                        || app.paint.sculpt.metal_overlay.is_some()
                        || app.paint.sculpt.type_overlay.is_some()
                        || app.paint.sculpt.texture_overlay.is_some(),
                    egui::Button::new("Reset sculpt"),
                )
                .clicked()
            {
                app.paint.sculpt = Default::default();
                app.mark_dirty();
            }
        });

    // Central panel is intentionally left unclaimed here. bar-app's
    // AppWrapper::update() claims it after self.app.update() returns and
    // draws the 3D viewport there, which is the only place GPU resources
    // (TerrainRenderer, render_state) are available.
}

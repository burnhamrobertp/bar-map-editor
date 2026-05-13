//! Sculpt3D layout -- left layer panel + central 3D viewport.
//!
//! The layer panel derives its contents from the live node graph via
//! `compute_sculpt_layers`. Selecting a layer sets
//! `paint.selected_sculpt_layer`; brush strokes then write into that
//! node's live buffer and flush to node params on stroke end.
//!
//! The central panel is left unclaimed here -- bar-app draws the 3D
//! viewport there after this function returns.

use eframe::egui;
use eframe::egui::Color32;

use crate::app::{BarEditorApp, BrushTool};
use crate::panels::canvas::sculpt_layers::{compute_sculpt_layers, SculptLayerEntry};

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
    let entries = compute_sculpt_layers(&app.graph);

    // --- LAYERS section ---
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.strong("Layers");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("+").clicked() {
                add_paint_layer(app);
            }
        });
    });
    ui.separator();

    if entries.is_empty() {
        ui.weak("Add a Bundler node to enable sculpting.");
    } else {
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .show(ui, |ui| {
                for entry in &entries {
                    draw_layer_row(app, ui, entry);
                }
            });
    }

    ui.add_space(8.0);
    ui.separator();

    // --- TOOLS section ---
    ui.strong("Tools");
    ui.add_space(4.0);

    let selected_kind = app
        .paint
        .selected_sculpt_layer
        .and_then(|id| app.graph.get_node(id))
        .map(|n| n.node_type.clone());

    let has_terrain = app.paint.heightmap.is_some();

    match selected_kind {
        Some(bar_graph::NodeType::PaintedHeightmap) | Some(bar_graph::NodeType::Sculpt) => {
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
            if !has_terrain {
                draw_no_terrain_hint(app, ui);
            }
        }
        Some(bar_graph::NodeType::PaintedTexture) => {
            ui.horizontal(|ui| {
                ui.label("Colour");
                let [r, g, b] = app.paint.brush.color_rgb;
                let mut c = egui::Color32::from_rgb(r, g, b);
                if ui.color_edit_button_srgba(&mut c).changed() {
                    app.paint.brush.color_rgb = [c.r(), c.g(), c.b()];
                }
            });
            if !has_terrain {
                draw_no_terrain_hint(app, ui);
            }
        }
        _ => {
            ui.weak("Select a paintable layer.");
        }
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

    ui.add_space(8.0);
    ui.separator();
    draw_feature_palette(app, ui);
}

fn draw_feature_palette(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.strong("Features");
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
        ui.weak("Click on terrain to place.");
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
                    if selected {
                        app.selected_feature_type = None;
                    } else {
                        app.selected_feature_type = Some(name.clone());
                        // Deselect sculpt layer so brush doesn't fire.
                        app.paint.selected_sculpt_layer = None;
                    }
                }
            }
        });
}

fn draw_layer_row(app: &mut BarEditorApp, ui: &mut egui::Ui, entry: &SculptLayerEntry) {
    let indent_px = entry.indent as f32 * 16.0;
    let is_selected =
        !entry.is_compositor && app.paint.selected_sculpt_layer == Some(entry.node_id);
    let node_id = entry.node_id;
    let is_compositor = entry.is_compositor;
    let is_paintable = entry.is_paintable;
    let label = entry.label.clone();
    let channel = entry.channel.clone();

    let clicked = ui
        .horizontal(|ui| {
            if indent_px > 0.0 {
                ui.add_space(indent_px);
            }
            paint_channel_dot(ui, &channel);
            if is_compositor {
                ui.weak(format!("> {}", label));
                false
            } else {
                let label_text = if is_paintable {
                    label
                } else {
                    format!("{} [locked]", label)
                };
                ui.selectable_label(is_selected, &label_text).clicked()
            }
        })
        .inner;

    if clicked {
        app.paint.selected_sculpt_layer = Some(node_id);
    }
}

fn paint_channel_dot(ui: &mut egui::Ui, channel: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 4.0, channel_color(channel));
}

fn channel_color(channel: &str) -> Color32 {
    match channel {
        "heightmap" => Color32::from_rgb(100, 200, 100),
        "texture" => Color32::from_rgb(180, 100, 220),
        "metalmap" => Color32::from_rgb(220, 120, 60),
        "typemap" => Color32::from_rgb(80, 140, 210),
        "grassmap" => Color32::from_rgb(80, 195, 140),
        "specular" => Color32::from_rgb(210, 195, 70),
        "normalmap" => Color32::from_rgb(130, 90, 210),
        _ => Color32::from_rgb(160, 160, 160),
    }
}

fn draw_no_terrain_hint(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.weak("No terrain loaded --");
        if ui.small_button("Run preview").clicked() {
            app.preview.run_requested = true;
        }
    });
}

/// Create a disconnected PaintedHeightmap node and notify the user to wire it.
fn add_paint_layer(app: &mut BarEditorApp) {
    use crate::state::NodeVisual;
    use bar_graph::{Node, NodeId, NodeType};

    let label = app.next_label_for("Painted Layer");
    let node = Node::new(NodeId(0), NodeType::PaintedHeightmap, &label);
    let id = app.graph.add_node(node);

    // Place it near the top-left of the visible canvas area.
    let pos = app.canvas.offset + egui::vec2(80.0, 80.0);
    app.visuals.node_visuals.insert(
        id,
        NodeVisual {
            position: egui::pos2(pos.x, pos.y),
            size: egui::vec2(150.0, 80.0),
        },
    );
    app.push_undo("Add paint layer");
    app.paint.selected_sculpt_layer = Some(id);
    app.mark_dirty();
    app.dialog.toast = Some((
        format!("Added '{}' -- wire it into the canvas.", label),
        std::time::Instant::now(),
    ));
}

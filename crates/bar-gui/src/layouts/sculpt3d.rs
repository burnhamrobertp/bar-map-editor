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

use crate::app::{BarEditorApp, BrushTool};
use crate::panels::canvas::sculpt_layers::{compute_sculpt_layers, SculptLayerGroup};

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
    let groups = compute_sculpt_layers(&app.graph);

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

    if groups.is_empty() {
        ui.weak("Add a Bundler node to enable sculpting.");
    } else {
        for group in &groups {
            draw_layer_group(app, ui, group);
        }
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
    ui.add(
        egui::Slider::new(&mut app.paint.brush.radius_px, 0.5..=96.0)
            .text("Radius")
            .logarithmic(true)
            .clamping(egui::SliderClamping::Always),
    );
    ui.add(
        egui::Slider::new(&mut app.paint.brush.strength, 0.001..=0.2)
            .text("Strength")
            .logarithmic(true)
            .clamping(egui::SliderClamping::Always),
    );
    ui.add(
        egui::Slider::new(&mut app.paint.brush.falloff, 0.5..=4.0)
            .text("Falloff")
            .clamping(egui::SliderClamping::Always),
    );
}

fn draw_layer_group(app: &mut BarEditorApp, ui: &mut egui::Ui, group: &SculptLayerGroup) {
    let channel_label = channel_display_name(&group.channel);
    ui.add_space(4.0);
    ui.label(egui::RichText::new(channel_label).small().strong());

    for entry in group.entries.iter().rev() {
        let is_selected = app.paint.selected_sculpt_layer == Some(entry.node_id);

        let icon = if !entry.is_connected {
            "!"
        } else if entry.is_paintable {
            "~"
        } else {
            "#"
        };

        let label_text = if entry.is_connected {
            format!("[{}] {}", icon, entry.label)
        } else {
            format!("[!] {} (disconnected)", entry.label)
        };

        let resp = ui.add_enabled(
            true,
            egui::SelectableLabel::new(
                is_selected,
                egui::RichText::new(&label_text).color(if entry.is_connected {
                    ui.visuals().text_color()
                } else {
                    ui.visuals().weak_text_color()
                }),
            ),
        );

        if !entry.is_connected {
            resp.on_hover_text("Layer is disconnected -- wire it in the canvas.");
        } else if resp.clicked() {
            app.paint.selected_sculpt_layer = Some(entry.node_id);
        }
    }
}

fn channel_display_name(channel: &str) -> &'static str {
    match channel {
        "heightmap" => "Heightmap",
        "texture" => "Texture",
        "metalmap" => "Metal",
        "typemap" => "Type",
        "grassmap" => "Grass",
        "specular" => "Specular",
        "normalmap" => "Normal",
        _ => "Other",
    }
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

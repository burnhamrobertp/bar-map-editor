//! Node palette — the collapsible category list on the left
//! sidebar. Drag an item onto the canvas to drop a node;
//! double-click to drop at a default position. The palette is a
//! pure renderer: drag state is published via
//! `BarEditorApp::set_palette_drag` and double-click drops route
//! through `add_node_at`. The canvas's drop handler (still in
//! `app.rs`) is what actually creates the node.

use bar_graph::NodeType;
use eframe::egui;

use crate::app::{BarEditorApp, PaletteDrag, PaletteKind};
use crate::t;

/// Render the palette into the surrounding `ui` (typically a
/// `SidePanel` body).
pub(crate) fn draw(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.add(egui::Label::new(
        egui::RichText::new(t!("editor.palette.heading"))
            .size(13.0)
            .strong(),
    ));
    ui.separator();
    ui.add_space(2.0);

    let generators = [
        ("Perlin Noise", NodeType::PerlinNoise),
        ("Simplex Noise", NodeType::SimplexNoise),
        ("Worley Noise", NodeType::WorleyNoise),
        ("Ridged Noise", NodeType::RidgedNoise),
        ("Constant", NodeType::Constant),
    ];

    let filters = [
        ("Hydraulic Erosion", NodeType::HydraulicErosion),
        ("Thermal Erosion", NodeType::ThermalErosion),
        ("Blur", NodeType::Blur),
        ("Sharpen", NodeType::Sharpen),
        ("Clamp", NodeType::Clamp),
        ("Invert", NodeType::Invert),
        ("2D Sculpt", NodeType::Sculpt),
        ("Preview", NodeType::Preview),
    ];

    let combiners = [
        ("Blend", NodeType::Blend),
        ("Add", NodeType::Add),
        ("Subtract", NodeType::Subtract),
        ("Multiply", NodeType::Multiply),
        ("Max", NodeType::Max),
        ("Min", NodeType::Min),
    ];

    let texture = [
        ("Slope Map", NodeType::SlopeMap),
        ("Height Select", NodeType::HeightSelect),
        ("Splat Map", NodeType::SplatMap),
        ("Auto Texture", NodeType::AutoTexture),
        ("Normal Map", NodeType::NormalMap),
        ("Grass Map", NodeType::GrassMap),
        ("Specular Map", NodeType::SpecularMap),
    ];

    let masks = [
        ("Painted Heightmap", NodeType::PaintedHeightmap),
        ("Painted Texture", NodeType::PaintedTexture),
        ("Mask Threshold", NodeType::MaskThreshold),
        ("Mask Invert", NodeType::MaskInvert),
        ("Mask Blur", NodeType::MaskBlur),
        ("Mask Apply", NodeType::MaskApply),
    ];

    let bundlers = [("Bundler", NodeType::Bundler)];

    let sources = [
        ("SMF Import", NodeType::SmfImport),
        ("SMT Import", NodeType::SmtImport),
        ("Pass-Through", NodeType::PassThrough),
        ("File Reference", NodeType::FileReference),
    ];

    let mut to_add: Option<(NodeType, String)> = None;
    let mut drag_start: Option<PaletteDrag> = None;

    macro_rules! palette_group {
        ($ui:expr, $title:expr, $items:expr) => {
            $ui.collapsing($title, |ui| {
                for (label, node_type) in &$items {
                    let resp = palette_item(ui, label, node_type);
                    if resp.drag_started() && drag_start.is_none() {
                        drag_start = Some(PaletteDrag {
                            kind: PaletteKind::Node(node_type.clone()),
                            label: label.to_string(),
                        });
                    }
                    if resp.double_clicked() {
                        to_add = Some((node_type.clone(), label.to_string()));
                    }
                }
            });
        };
    }

    // SubGraph IO nodes — only meaningful inside a subgraph view.
    // Pinned to the TOP of the palette (above Generators) since
    // they're how a subgraph's external interface is now defined.
    if app.is_in_subgraph_view() {
        let subgraph_io = [
            ("Subgraph Input", NodeType::SubgraphInput),
            ("Subgraph Output", NodeType::SubgraphOutput),
        ];
        palette_group!(ui, "SubGraph IO", subgraph_io);
        ui.add_space(8.0);
    }

    palette_group!(ui, "Generators", generators);
    palette_group!(ui, "Filters", filters);
    palette_group!(ui, "Combiners", combiners);
    palette_group!(ui, "Texture", texture);
    palette_group!(ui, "Masks", masks);
    palette_group!(ui, "Bundler", bundlers);
    palette_group!(ui, "Sources", sources);

    // Macros — pre-built SubGraphs that drop as a complete chunk
    // of graph wired up for a typical map archetype. Drop one,
    // wire its output to a Bundler, you're done.
    ui.collapsing("Macros", |ui| {
        for group in crate::macros::BUILTIN_MACRO_GROUPS {
            ui.collapsing(group.name, |ui| {
                for entry in group.entries {
                    let resp = ui.add(
                        egui::Label::new(entry.display_name)
                            .sense(egui::Sense::click_and_drag())
                            .selectable(false),
                    );
                    if resp.drag_started() && drag_start.is_none() {
                        drag_start = Some(PaletteDrag {
                            kind: PaletteKind::Macro {
                                name: entry.full_name.to_string(),
                            },
                            label: format!("{} - {}", group.name, entry.display_name),
                        });
                    }
                }
            });
        }
    });

    if let Some(pd) = drag_start {
        app.set_palette_drag(pd);
    }

    if let Some((node_type, label)) = to_add {
        // Place at a reasonable canvas-space position (centre of default viewport).
        let pos = egui::pos2(300.0, 200.0);
        app.add_node_at(node_type, &label, pos);
    }
}

/// Render a single palette item (drag-and-drop / double-click to
/// add a node). Returns a `Response` with `click_and_drag` sense
/// so the caller can detect `drag_started()` (start palette
/// drag) and `double_clicked()` (add at default pos).
fn palette_item(ui: &mut egui::Ui, label: &str, node_type: &NodeType) -> egui::Response {
    let desired_size = egui::vec2(ui.available_width(), 22.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());

    if ui.is_rect_visible(rect) {
        let bg = if response.is_pointer_button_down_on() {
            egui::Color32::from_rgb(55, 60, 80)
        } else if response.hovered() {
            egui::Color32::from_rgb(48, 53, 70)
        } else {
            egui::Color32::TRANSPARENT
        };
        if bg != egui::Color32::TRANSPARENT {
            ui.painter().rect_filled(rect, 3.0, bg);
        }

        // Type colour dot
        ui.painter().circle_filled(
            egui::pos2(rect.left() + 9.0, rect.center().y),
            3.5,
            crate::app::node_type_color(node_type),
        );

        // Label
        let text_col = if response.hovered() || response.is_pointer_button_down_on() {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_rgb(190, 190, 200)
        };
        ui.painter().text(
            egui::pos2(rect.left() + 19.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.0),
            text_col,
        );
    }
    response
}

//! Node palette -- the collapsible category list on the left
//! sidebar. Drag an item onto the canvas to drop a node;
//! double-click to drop at a default position. The palette is a
//! pure renderer: drag state is published via
//! `BarEditorApp::set_palette_drag` and double-click drops route
//! through `add_node_at`. The canvas's drop handler (still in
//! `app.rs`) is what actually creates the node.

use bar_graph::nodes::NodeCategory;
use bar_graph::NodeType;
use eframe::egui;

use crate::app::{BarEditorApp, PaletteDrag, PaletteKind};
use crate::t;

/// Palette section title for a node category. Drives both section
/// order (via `SECTION_ORDER`) and the collapsing-header label.
fn section_title(category: NodeCategory) -> &'static str {
    match category {
        NodeCategory::Generator => "Generators",
        NodeCategory::Filter => "Filters",
        NodeCategory::Combiner => "Combiners",
        NodeCategory::Colorizer => "Colorizers",
        NodeCategory::SplatMap => "Splat & Maps",
        NodeCategory::Mask => "Masks",
        NodeCategory::Source => "Sources",
        NodeCategory::Io => "I/O",
        NodeCategory::Terminal => "Output",
    }
}

/// Section render order, top to bottom.
const SECTION_ORDER: &[NodeCategory] = &[
    NodeCategory::Generator,
    NodeCategory::Filter,
    NodeCategory::Combiner,
    NodeCategory::Colorizer,
    NodeCategory::SplatMap,
    NodeCategory::Mask,
    NodeCategory::Source,
    NodeCategory::Io,
    NodeCategory::Terminal,
];

/// Placeable nodes in a category, sorted by label. Excludes
/// subgraph-only nodes -- those render only in the SubGraph IO block.
fn placeable_in(category: NodeCategory) -> Vec<(&'static str, NodeType)> {
    let mut v: Vec<(&'static str, NodeType)> = bar_graph::nodes::all_defs()
        // Terminal (FinalComposition) is an auto-created singleton, not
        // user-placeable; subgraph-IO nodes render only in the subgraph view.
        .filter(|d| d.category == category && !d.caps.is_subgraph_only && !d.caps.is_terminal)
        .map(|d| (d.label, d.node_type.clone()))
        .collect();
    v.sort_by(|a, b| a.0.cmp(b.0));
    v
}

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

    let mut to_add: Option<(NodeType, String)> = None;
    let mut drag_start: Option<PaletteDrag> = None;

    // Search box -- always visible at the top of the palette.
    let search_resp = ui.add(
        egui::TextEdit::singleline(&mut app.palette_filter)
            .hint_text("Search nodes...")
            .desired_width(f32::INFINITY),
    );
    crate::panels::widgets::select_all_on_focus(ui, &search_resp, &app.palette_filter);
    ui.add_space(4.0);

    let filter = app.palette_filter.to_lowercase();

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

    if !filter.is_empty() {
        // Flat filtered list -- categories are suppressed. Single source:
        // every placeable descriptor, regardless of section.
        let mut any = false;
        let mut matches: Vec<(&'static str, NodeType)> = bar_graph::nodes::all_defs()
            .filter(|d| {
                !d.caps.is_subgraph_only
                    && !d.caps.is_terminal
                    && d.label.to_lowercase().contains(&filter)
            })
            .map(|d| (d.label, d.node_type.clone()))
            .collect();
        matches.sort_by(|a, b| a.0.cmp(b.0));
        for (label, node_type) in &matches {
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
            any = true;
        }
        // Also search macros.
        for group in crate::macros::BUILTIN_MACRO_GROUPS {
            for entry in group.entries {
                let full = format!("{} {}", group.name, entry.display_name).to_lowercase();
                if full.contains(&filter) || entry.display_name.to_lowercase().contains(&filter) {
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
                    any = true;
                }
            }
        }
        if !any {
            ui.label(
                egui::RichText::new("No matches")
                    .color(ui.visuals().weak_text_color())
                    .italics(),
            );
        }
    } else {
        // Normal category tree.

        // SubGraph IO nodes -- only meaningful inside a subgraph view.
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

        for &category in SECTION_ORDER {
            let items = placeable_in(category);
            if items.is_empty() {
                continue;
            }
            palette_group!(ui, section_title(category), items);
        }

        // Macros -- pre-built SubGraphs that drop as a complete chunk
        // of graph wired up for a typical map archetype. Drop one and
        // wire it into Final Composition's inputs.
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
    }

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
        let (bg_hover, bg_press, text_col) = {
            let vis = ui.visuals();
            (
                vis.widgets.hovered.weak_bg_fill,
                vis.widgets.active.weak_bg_fill,
                vis.strong_text_color(),
            )
        };
        let bg = if response.is_pointer_button_down_on() {
            bg_press
        } else if response.hovered() {
            bg_hover
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

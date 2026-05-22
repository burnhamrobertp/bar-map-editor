//! Map Edge modal -- the mirrored area surrounding the playable map
//! (engine widget: `luaui/Widgets/map_edge_extension2.lua`).
//!
//! Holds the `grassShadingTex` picker / preview today. Future
//! map-edge knobs (curvature bend strength, mirror detail density)
//! land in this modal rather than crowding the main Map Info
//! group. The cached preview texture lives on `BarEditorApp.map_edge`
//! so flipping the modal open / closed doesn't re-decode the file.

use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::modal_frame;
use crate::panels::field_editor::heading_with_info;
use crate::panels::image_preview::PreviewCache;
use crate::t;

/// Per-session preview state for the modal. Lives on `BarEditorApp`
/// so it survives modal open / close cycles; reset on project switch.
#[derive(Default)]
pub struct MapEdgePanelState {
    pub(crate) preview: PreviewCache,
}

fn resolve_grass_shading_path(
    project_dir: &std::path::Path,
    filename: &str,
) -> Option<std::path::PathBuf> {
    if filename.is_empty() {
        let minimap = project_dir.join(bar_project::SMF_MINIMAP_SIDE_CAR);
        return minimap.is_file().then_some(minimap);
    }
    bar_project::find_file_in_dir(&project_dir.join("passthrough"), filename)
        .or_else(|| bar_project::find_file_in_dir(project_dir, filename))
}

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_map_edge_editor {
        return;
    }
    let mut open = app.dialog.show_map_edge_editor;
    let project_path_opt: Option<std::path::PathBuf> = app
        .project
        .path
        .clone()
        .or_else(|| app.project.pending_map_data_dir.clone());
    let parent_window = app.parent_window();

    modal_frame(
        ctx,
        &mut open,
        &t!("editor.modals.map_edge.title"),
        "map_edge_editor_modal",
        |ui| {
            heading_with_info(
                ui,
                &t!("editor.modals.map_edge.shading_heading"),
                &t!("editor.modals.map_edge.shading_info"),
            );

            let mut filename = app.map_settings().resources.grass_shading_tex.clone();
            let file_label = t!("editor.modals.map_edge.file_label");
            let dialog_title = t!("editor.modals.map_edge.dialog_title");
            let hint = t!("editor.modals.map_edge.hint");
            if crate::panels::file_picker::FilePickerField::new(&file_label, "passthrough")
                .extensions(&["dds", "png", "jpg", "jpeg", "tga", "bmp"])
                .title(&dialog_title)
                .allow_clear(true)
                .hint(&hint)
                .show(
                    ui,
                    &mut filename,
                    project_path_opt.as_deref(),
                    parent_window.as_ref(),
                )
            {
                app.push_undo(&t!("editor.modals.map_edge.undo_edit"));
                app.map_settings_mut().resources.grass_shading_tex = filename.clone();
                app.map_edge.preview.invalidate();
            }

            ui.add_space(12.0);
            ui.label(if filename.is_empty() {
                t!("editor.modals.map_edge.preview_smf")
            } else {
                t!("editor.modals.map_edge.preview_custom")
            });

            let active_filename = filename.clone();
            let project_dir_opt = project_path_opt.clone();
            let preview_cache = &mut app.map_edge.preview;
            let max_side = 384.0_f32;
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_size(egui::vec2(max_side + 8.0, max_side + 8.0));
                ui.with_layout(
                    egui::Layout::centered_and_justified(egui::Direction::TopDown),
                    |ui| {
                        let handle = project_dir_opt.as_deref().and_then(|p| {
                            preview_cache.ensure(
                                ctx,
                                p,
                                &active_filename,
                                max_side as u32,
                                "map_edge_grass_shading_preview",
                                resolve_grass_shading_path,
                            )
                        });
                        if let Some(tex) = handle {
                            let [w, h] = tex.size();
                            let aspect = w as f32 / h.max(1) as f32;
                            let display_size = if aspect >= 1.0 {
                                egui::vec2(max_side, max_side / aspect)
                            } else {
                                egui::vec2(max_side * aspect, max_side)
                            };
                            ui.image((tex.id(), display_size));
                        } else {
                            ui.label(t!("editor.modals.map_edge.preview_unavailable"));
                        }
                    },
                );
            });
        },
    );

    app.dialog.show_map_edge_editor = open;
}

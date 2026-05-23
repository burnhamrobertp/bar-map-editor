//! Grass modal -- `mapinfo.custom.grassConfig`. Texture file
//! pickers (richer than the schema's plain TextEdit) plus the
//! schema-driven numeric / blend / fade fields.
//!
//! All defaults match BAR widget
//! `bar-game/luaui/Widgets/map_grass_gl4.lua:87-110` verbatim.
//! Edits are live: the grass widget's `set_config` runs every
//! frame from `sync_map_grass`, so drag-value changes flow into
//! the GPU uniform on the next frame without a reimport.

use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::{modal_frame, render_specs, FieldFindings};
use crate::panels::field_editor::heading_with_info;
use crate::panels::file_picker::FilePickerField;
use crate::t;

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_grass_editor {
        return;
    }

    let project_path_opt: Option<std::path::PathBuf> = app
        .project
        .path
        .clone()
        .or_else(|| app.project.pending_map_data_dir.clone());
    let parent_window = app.parent_window();
    let mut open = app.dialog.show_grass_editor;
    let mut texture_dirty = false;

    modal_frame(
        ctx,
        &mut open,
        &t!("editor.modals.grass.title"),
        "grass_editor_modal",
        |ui| {
            let findings = FieldFindings::from(app.validation.findings());
            ui.add_space(12.0);
            heading_with_info(
                ui,
                &t!("editor.modals.grass.textures_heading"),
                &t!("editor.modals.grass.textures_info"),
            );

            let mut dist_tga = app
                .map_settings()
                .custom_grass
                .dist_tga
                .clone()
                .unwrap_or_default();
            let mut blade_color_tex = app
                .map_settings()
                .custom_grass
                .blade_color_tex
                .clone()
                .unwrap_or_default();
            let mut grass_shading_tex = app.map_settings().resources.grass_shading_tex.clone();

            let dist_dialog_title = t!("editor.modals.grass.dist_tga_dialog_title");
            if FilePickerField::new("grassDistTGA", "passthrough")
                .extensions(&["tga"])
                .title(&dist_dialog_title)
                .show(
                    ui,
                    &mut dist_tga,
                    project_path_opt.as_deref(),
                    parent_window.as_ref(),
                )
            {
                let new = if dist_tga.is_empty() {
                    None
                } else {
                    Some(dist_tga.clone())
                };
                app.map_settings_mut().custom_grass.dist_tga = new;
                texture_dirty = true;
            }
            let blade_dialog_title = t!("editor.modals.grass.blade_dialog_title");
            if FilePickerField::new("grassBladeColorTex", "passthrough")
                .extensions(&["dds", "png", "jpg", "jpeg", "tga"])
                .title(&blade_dialog_title)
                .show(
                    ui,
                    &mut blade_color_tex,
                    project_path_opt.as_deref(),
                    parent_window.as_ref(),
                )
            {
                let new = if blade_color_tex.is_empty() {
                    None
                } else {
                    Some(blade_color_tex.clone())
                };
                app.map_settings_mut().custom_grass.blade_color_tex = new;
                texture_dirty = true;
            }

            ui.add_space(12.0);
            heading_with_info(
                ui,
                &t!("editor.modals.grass.colours_heading"),
                &t!("editor.modals.grass.colours_info"),
            );
            let shading_dialog_title = t!("editor.modals.grass.shading_dialog_title");
            let shading_hint = t!("editor.modals.grass.shading_hint");
            if FilePickerField::new("grassShadingTex", "passthrough")
                .extensions(&["dds", "png", "jpg", "jpeg"])
                .title(&shading_dialog_title)
                .allow_clear(true)
                .hint(&shading_hint)
                .show(
                    ui,
                    &mut grass_shading_tex,
                    project_path_opt.as_deref(),
                    parent_window.as_ref(),
                )
            {
                app.map_settings_mut().resources.grass_shading_tex = grass_shading_tex.clone();
                texture_dirty = true;
            }

            ui.add_space(12.0);
            render_specs(ui, app, bar_project::recipe_fields::GRASS_SPECS, &findings);
        },
    );

    app.dialog.show_grass_editor = open;

    if texture_dirty {
        app.push_undo(&t!("editor.modals.grass.undo_edit_texture"));
    }
}

//! Optional extras page -- splat distribution + four detail normals;
//! specular / sky-reflect / detail-normal / light-emission masks;
//! minimap override; skybox cubemap. Every slot optional.

use eframe::egui;

use super::file_row;
use crate::app::BarEditorApp;

pub(super) fn draw(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    ui.label(
        "Optional textures. Use these to override engine defaults; \
         everything left empty falls through to the engine's behaviour.",
    );
    ui.add_space(8.0);

    let parent = app.parent_window();
    let parent = parent.as_ref();
    let picks = &mut app.assemble_map.picks;

    egui::CollapsingHeader::new("Splats")
        .default_open(false)
        .show(ui, |ui| {
            file_row(
                ui,
                "Distribution",
                &["png", "dds", "tga", "bmp"],
                &mut picks.splat_distribution_path,
                parent,
                "Select splat distribution (4-channel mask)",
            );
            file_row(
                ui,
                "Detail normal 1",
                &["png", "dds", "tga", "bmp"],
                &mut picks.splat_detail_normal_1_path,
                parent,
                "Select splat detail normal 1",
            );
            file_row(
                ui,
                "Detail normal 2",
                &["png", "dds", "tga", "bmp"],
                &mut picks.splat_detail_normal_2_path,
                parent,
                "Select splat detail normal 2",
            );
            file_row(
                ui,
                "Detail normal 3",
                &["png", "dds", "tga", "bmp"],
                &mut picks.splat_detail_normal_3_path,
                parent,
                "Select splat detail normal 3",
            );
            file_row(
                ui,
                "Detail normal 4",
                &["png", "dds", "tga", "bmp"],
                &mut picks.splat_detail_normal_4_path,
                parent,
                "Select splat detail normal 4",
            );
        });

    egui::CollapsingHeader::new("Per-pixel masks")
        .default_open(false)
        .show(ui, |ui| {
            file_row(
                ui,
                "Specular",
                &["png", "dds", "tga", "bmp"],
                &mut picks.specular_path,
                parent,
                "Select specular mask",
            );
            file_row(
                ui,
                "Sky reflect modulation",
                &["png", "dds", "tga", "bmp"],
                &mut picks.sky_reflect_mod_path,
                parent,
                "Select sky reflection modulation mask",
            );
            file_row(
                ui,
                "Detail normal",
                &["png", "dds", "tga", "bmp"],
                &mut picks.detail_normal_path,
                parent,
                "Select per-pixel detail normal",
            );
            file_row(
                ui,
                "Light emission",
                &["png", "dds", "tga", "bmp"],
                &mut picks.light_emission_path,
                parent,
                "Select light emission texture",
            );
        });

    egui::CollapsingHeader::new("Sky and minimap")
        .default_open(false)
        .show(ui, |ui| {
            file_row(
                ui,
                "Minimap",
                &["png", "dds", "jpg", "jpeg", "tga", "bmp"],
                &mut picks.minimap_path,
                parent,
                "Select minimap image (any size; encoded to 1024x1024 DXT1 at bundle)",
            );
            file_row(
                ui,
                "Skybox (cubemap)",
                &["dds"],
                &mut picks.skybox_path,
                parent,
                "Select skybox DDS cubemap",
            );
        });
}

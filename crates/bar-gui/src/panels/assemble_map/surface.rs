//! Surface layers page -- optional diffuse / metalmap / typemap /
//! grass-distribution pickers. Everything optional; skip = bundler
//! falls back to its built-in generator (terrain-derived texture,
//! empty metalmap, etc.).

use eframe::egui;

use super::file_row;
use crate::app::BarEditorApp;

pub(super) fn draw(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    ui.label(
        "Optional surface layers. Anything you skip is auto-generated \
         or left empty at bundle time.",
    );
    ui.add_space(8.0);

    let parent = app.parent_window();
    let parent = parent.as_ref();
    let picks = &mut app.assemble_map.picks;

    file_row(
        ui,
        "Diffuse texture",
        &["png", "dds", "jpg", "jpeg", "tga", "bmp"],
        &mut picks.diffuse_path,
        parent,
        "Select diffuse / surface texture",
    );
    file_row(
        ui,
        "Metalmap",
        &["png", "tga", "bmp"],
        &mut picks.metalmap_path,
        parent,
        "Select metalmap (grayscale)",
    );
    file_row(
        ui,
        "Typemap",
        &["png", "tga", "bmp"],
        &mut picks.typemap_path,
        parent,
        "Select typemap (grayscale terrain-type index)",
    );
    file_row(
        ui,
        "Grass distribution",
        &["tga", "png", "bmp"],
        &mut picks.grass_distribution_path,
        parent,
        "Select grass distribution mask (grayscale)",
    );
}

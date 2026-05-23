//! Identity page -- map name (required), author, description, version.

use eframe::egui;

use crate::app::BarEditorApp;

pub(super) fn draw(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    ui.label(
        "Name your map. The other fields are optional and can be \
         edited later via the Identity action-bar modal.",
    );
    ui.add_space(8.0);

    egui::Grid::new("assemble_map_identity_grid")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Name");
            ui.add(
                egui::TextEdit::singleline(&mut app.assemble_map.picks.name)
                    .hint_text("Required")
                    .desired_width(360.0),
            );
            ui.end_row();

            ui.label("Author");
            ui.add(
                egui::TextEdit::singleline(&mut app.assemble_map.picks.author).desired_width(360.0),
            );
            ui.end_row();

            ui.label("Description");
            ui.add(
                egui::TextEdit::multiline(&mut app.assemble_map.picks.description)
                    .desired_rows(3)
                    .desired_width(360.0),
            );
            ui.end_row();

            ui.label("Version");
            ui.add(
                egui::TextEdit::singleline(&mut app.assemble_map.picks.version)
                    .hint_text("e.g. 1.0")
                    .desired_width(120.0),
            );
            ui.end_row();
        });
}

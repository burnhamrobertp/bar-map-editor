//! Welcome panel -- replaces the blank canvas before any project is
//! loaded. Surfaces the four primary entry points (Blank, Open,
//! Import SD7, Assemble Map) plus a recent-files menu. Stateless:
//! every interaction routes through `BarEditorApp` methods so other
//! layouts can call `panels::welcome::draw` without coordination.

use eframe::egui;

use crate::app::BarEditorApp;
use crate::t;

/// Render the welcome panel into `ui`. Caller decides when to
/// display it (today: when `graph.nodes()` is empty AND no project
/// is loaded, in `BarEditorApp::draw_node_graph`).
pub(crate) fn draw(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    let available = ui.available_size();
    let (rect, _resp) = ui.allocate_exact_size(available, egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);

    let max_width: f32 = 720.0;
    let target_w = available.x.min(max_width);
    let panel_left = rect.left() + (rect.width() - target_w) * 0.5;
    let panel_top = rect.top() + 32.0;
    let panel_rect = egui::Rect::from_min_size(
        egui::pos2(panel_left, panel_top),
        egui::vec2(target_w, available.y - 64.0),
    );

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(panel_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );

    child.heading(t!("editor.welcome.heading"));
    child.add_space(4.0);
    child.weak(t!("editor.welcome.tagline"));
    child.add_space(24.0);

    // Four equal-weight entry points laid out as a single row. Each
    // is a large primary affordance -- the welcome screen's only job
    // is to land the user in one of these four flows.
    let btn_w = 160.0_f32;
    let btn_h = 48.0_f32;
    let gap = 14.0_f32;
    let pair_w = btn_w * 4.0 + gap * 3.0;
    let avail = child.available_width();
    let lpad = ((avail - pair_w) * 0.5).max(0.0);

    let mut clicked_blank = false;
    let mut clicked_open = false;
    let mut clicked_import = false;
    let mut clicked_assemble = false;
    child.horizontal(|ui| {
        ui.add_space(lpad);
        if ui
            .add_sized(
                [btn_w, btn_h],
                egui::Button::new(
                    egui::RichText::new(t!("editor.welcome.blank_project")).size(15.0),
                ),
            )
            .clicked()
        {
            clicked_blank = true;
        }
        ui.add_space(gap);
        if ui
            .add_sized(
                [btn_w, btn_h],
                egui::Button::new(
                    egui::RichText::new(t!("editor.welcome.open_project")).size(15.0),
                ),
            )
            .clicked()
        {
            clicked_open = true;
        }
        ui.add_space(gap);
        if ui
            .add_sized(
                [btn_w, btn_h],
                egui::Button::new(egui::RichText::new(t!("editor.welcome.import_sd7")).size(15.0)),
            )
            .clicked()
        {
            clicked_import = true;
        }
        ui.add_space(gap);
        if ui
            .add_sized(
                [btn_w, btn_h],
                egui::Button::new(
                    egui::RichText::new(t!("editor.welcome.assemble_map")).size(15.0),
                ),
            )
            .clicked()
        {
            clicked_assemble = true;
        }
    });

    if !app.settings().recent_files.is_empty() {
        child.add_space(16.0);
        child.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.menu_button(
                egui::RichText::new(t!("editor.welcome.recent"))
                    .size(12.0)
                    .weak(),
                |ui| {
                    let recents = app.settings().recent_files.clone();
                    for p in recents {
                        let label = p
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| p.display().to_string());
                        if ui.button(label).clicked() {
                            app.start_open_path_for_panel(p);
                            ui.close_menu();
                        }
                    }
                },
            );
        });
    }

    if clicked_blank {
        app.welcome_blank_project();
    }
    if clicked_open {
        app.welcome_open_dialog();
    }
    if clicked_import {
        app.import_sd7_dialog_async();
    }
    if clicked_assemble {
        app.start_assemble_map_dialog();
    }
}

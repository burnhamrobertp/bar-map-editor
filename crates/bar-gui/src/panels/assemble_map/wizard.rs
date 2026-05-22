//! Modal frame, step indicator, Back / Next / Finish footer, and
//! per-page dispatch for the Assemble Map wizard.

use eframe::egui;

use super::state::Page;
use crate::app::BarEditorApp;

pub fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_assemble_map {
        return;
    }

    let mut open = true;
    let mut cancel = false;
    let mut finish = false;

    let page = app.assemble_map.page;

    egui::Window::new("Assemble Map")
        .id(egui::Id::new("assemble_map_wizard"))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .default_width(560.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            draw_step_header(ui, page);
            ui.separator();
            ui.add_space(8.0);

            // Per-page content -- each module exposes a `draw` fn taking
            // (&mut BarEditorApp, &mut egui::Ui). Pages mutate
            // `app.assemble_map.picks` directly and decide what counts
            // as "ready to advance" via `super::is_page_ready`.
            match page {
                Page::Identity => super::identity::draw(app, ui),
                Page::Heightmap => super::heightmap::draw(app, ui),
                Page::Surface => super::surface::draw(app, ui),
                Page::Extras => super::extras::draw(app, ui),
            }

            ui.add_space(12.0);
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let is_last = page.next().is_none();
                    let ready = is_page_ready(app, page);
                    if is_last {
                        if ui.add_enabled(ready, egui::Button::new("Finish")).clicked() {
                            finish = true;
                        }
                    } else if ui.add_enabled(ready, egui::Button::new("Next")).clicked() {
                        if let Some(next) = page.next() {
                            app.assemble_map.page = next;
                        }
                    }
                    if let Some(prev) = page.prev() {
                        if ui.button("Back").clicked() {
                            app.assemble_map.page = prev;
                        }
                    }
                });
            });
        });

    if !open || cancel {
        app.dialog.show_assemble_map = false;
        app.assemble_map.reset();
        return;
    }
    if finish {
        app.dialog.show_assemble_map = false;
        app.finish_assemble_map();
    }
}

fn draw_step_header(ui: &mut egui::Ui, page: Page) {
    ui.horizontal(|ui| {
        for i in 0..Page::COUNT {
            let is_current = i == page.step_index();
            let label = match i {
                0 => Page::Identity.title(),
                1 => Page::Heightmap.title(),
                2 => Page::Surface.title(),
                _ => Page::Extras.title(),
            };
            let text = egui::RichText::new(format!("{}. {}", i + 1, label));
            let text = if is_current {
                text.strong()
            } else {
                text.weak()
            };
            ui.label(text);
            if i + 1 < Page::COUNT {
                ui.weak("›");
            }
        }
    });
}

/// Per-page gate for the Next / Finish button. Identity needs a name;
/// Heightmap needs a successfully decoded heightmap. Surface and
/// Extras are entirely optional -- the user can advance with nothing
/// picked.
fn is_page_ready(app: &BarEditorApp, page: Page) -> bool {
    let p = &app.assemble_map.picks;
    match page {
        Page::Identity => !p.name.trim().is_empty(),
        Page::Heightmap => {
            p.heightmap_path.is_some()
                && p.squares_x > 0
                && p.squares_z > 0
                && app.assemble_map.heightmap_error.is_none()
        }
        Page::Surface | Page::Extras => true,
    }
}

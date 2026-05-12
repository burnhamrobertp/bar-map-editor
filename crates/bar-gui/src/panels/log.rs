//! In-app log window: scrolling list of LogEntry values with level
//! badges and elapsed timestamps.

use eframe::egui;

use crate::app::BarEditorApp;
use crate::log::LogLevel;
use crate::t;

pub(crate) fn level_color(level: LogLevel) -> egui::Color32 {
    match level {
        LogLevel::Debug => egui::Color32::from_gray(130),
        LogLevel::Info => egui::Color32::WHITE,
        LogLevel::Warning => egui::Color32::YELLOW,
        LogLevel::Error => egui::Color32::from_rgb(255, 80, 80),
    }
}

impl BarEditorApp {
    pub(crate) fn draw_log_window(&mut self, ctx: &egui::Context) {
        if !self.dialog.show_log {
            return;
        }

        let mut open = true;
        egui::Window::new(t!("editor.log.title"))
            .open(&mut open)
            .default_size([640.0, 280.0])
            .min_height(120.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.small_button(t!("editor.log.clear")).clicked() {
                        self.dialog.log_buffer.clear();
                    }
                    ui.weak(format!("({} entries)", self.dialog.log_buffer.len()));
                });
                ui.separator();

                let needs_scroll = self.dialog.log_buffer.take_needs_scroll();
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        for entry in self.dialog.log_buffer.entries() {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("{:8.1}s", entry.elapsed_secs))
                                        .monospace()
                                        .color(egui::Color32::from_gray(90)),
                                );
                                ui.label(
                                    egui::RichText::new(entry.level.label())
                                        .monospace()
                                        .strong()
                                        .color(level_color(entry.level)),
                                );
                                ui.label(
                                    egui::RichText::new(&entry.message)
                                        .color(level_color(entry.level)),
                                );
                            });
                        }
                        if needs_scroll {
                            ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                        }
                    });
            });

        if !open {
            self.dialog.show_log = false;
        }
    }
}

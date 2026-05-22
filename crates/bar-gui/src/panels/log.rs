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
                    ui.weak(t!(
                        "editor.log.entries_count",
                        n = self.dialog.log_buffer.len()
                    ));
                    ui.separator();
                    // Per-level visibility toggles. Each button reads
                    // its current state from `log_levels_visible` and
                    // flips that level on click; multiple levels can
                    // be on / off independently.
                    for (label, level) in [
                        ("INF", LogLevel::Info),
                        ("WRN", LogLevel::Warning),
                        ("ERR", LogLevel::Error),
                        ("DBG", LogLevel::Debug),
                    ] {
                        let visible = self.dialog.log_levels_visible.is_visible(level);
                        let btn = egui::Button::new(
                            egui::RichText::new(label)
                                .monospace()
                                .color(level_color(level)),
                        )
                        .selected(visible);
                        if ui.add(btn).clicked() {
                            self.dialog.log_levels_visible.set(level, !visible);
                            self.dialog.log_buffer.mark_needs_scroll();
                        }
                    }
                    ui.separator();
                    // Text search box.
                    ui.label(t!("editor.log.filter_label"));
                    let search = ui.add(
                        egui::TextEdit::singleline(&mut self.dialog.log_search)
                            .desired_width(150.0),
                    );
                    if search.changed() {
                        // Reset scroll when filter changes.
                        self.dialog.log_buffer.mark_needs_scroll();
                    }
                    if !self.dialog.log_search.is_empty()
                        && ui
                            .small_button("x")
                            .on_hover_text(t!("editor.log.clear_filter_hint"))
                            .clicked()
                    {
                        self.dialog.log_search.clear();
                    }
                });
                ui.separator();

                let needs_scroll = self.dialog.log_buffer.take_needs_scroll();
                let levels_visible = self.dialog.log_levels_visible;
                let search_lower = self.dialog.log_search.to_lowercase();
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        for entry in self.dialog.log_buffer.entries() {
                            if !levels_visible.is_visible(entry.level) {
                                continue;
                            }
                            if !search_lower.is_empty()
                                && !entry.message.to_lowercase().contains(&search_lower)
                            {
                                continue;
                            }
                            ui.horizontal_wrapped(|ui| {
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
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&entry.message)
                                            .color(level_color(entry.level)),
                                    )
                                    .wrap(),
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

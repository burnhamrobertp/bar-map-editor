//! Modal dialogs that aren't part of the main panel layout --
//! Preferences, About, and (in the future) the file editor and
//! map-info file picker. Confirm-dialog and unsaved-changes
//! prompts stay in `app.rs` for now because they're tightly
//! coupled to the `PendingAction` flow.

use eframe::egui;

use crate::app::BarEditorApp;
use crate::settings::Settings;
use crate::t;

/// Render the Preferences modal when the user has opened it.
/// No-op when `dialog.show_settings` is false.
pub(crate) fn draw_settings(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_settings {
        return;
    }
    let mut open = app.dialog.show_settings;
    let mut changed = false;
    egui::Window::new(t!("editor.prefs.title"))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.heading(t!("editor.prefs.autosave.heading"));
            let mut autosave_enabled = app.settings().autosave_enabled;
            if ui
                .checkbox(&mut autosave_enabled, t!("editor.prefs.autosave.enabled"))
                .changed()
            {
                app.settings.autosave_enabled = autosave_enabled;
                changed = true;
            }
            ui.horizontal(|ui| {
                ui.label(t!("editor.prefs.autosave.interval_label"));
                let mut secs = app.settings().autosave_interval_secs as i64;
                if ui
                    .add(egui::Slider::new(&mut secs, 30..=600).suffix(" s"))
                    .changed()
                {
                    app.settings.autosave_interval_secs = secs.max(30) as u64;
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label(t!("editor.prefs.autosave.slots_label"));
                let mut slots = app.settings().autosave_slot_count as i32;
                if ui
                    .add(egui::Slider::new(&mut slots, 1..=10))
                    .on_hover_text(t!("editor.prefs.autosave.slots_hint"))
                    .changed()
                {
                    app.settings.autosave_slot_count = slots.max(1) as u32;
                    changed = true;
                }
            });

            ui.add_space(8.0);
            ui.heading(t!("editor.prefs.confirmations.heading"));
            let suppressed_count = app.settings().suppressed_confirmations.len();
            if suppressed_count == 0 {
                ui.weak(t!("editor.prefs.confirmations.none_suppressed"));
            } else {
                ui.label(t!(
                    "editor.prefs.confirmations.suppressed_count",
                    n = suppressed_count.to_string()
                ));
                let mut keys: Vec<String> = app
                    .settings()
                    .suppressed_confirmations
                    .iter()
                    .cloned()
                    .collect();
                keys.sort();
                for k in &keys {
                    ui.weak(format!(
                        "  \u{2022} {}",
                        crate::app::confirm_key_display_name(k)
                    ));
                }
                if ui.button(t!("editor.prefs.confirmations.clear")).clicked() {
                    app.settings.suppressed_confirmations.clear();
                    changed = true;
                }
            }

            ui.add_space(8.0);
            ui.heading(t!("editor.prefs.startup.heading"));
            let mut restore = app.settings().restore_last_project;
            if ui
                .checkbox(&mut restore, t!("editor.prefs.startup.restore_last"))
                .changed()
            {
                app.settings.restore_last_project = restore;
                changed = true;
            }

            ui.add_space(8.0);
            ui.heading(t!("editor.prefs.game.heading"));
            ui.weak(t!("editor.prefs.game.hint"));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(t!("editor.prefs.game.archive_label"));
                let display = app
                    .settings()
                    .selected_game_archive
                    .as_deref()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or(&t!("editor.prefs.game.none"))
                    .to_string();
                ui.weak(&display);
            });
            ui.horizontal(|ui| {
                if ui.button(t!("editor.prefs.game.browse")).clicked() {
                    let picked = rfd::FileDialog::new()
                        .add_filter("BAR game archive", &["sdz", "sd7", "sdd"])
                        .pick_file();
                    if let Some(path) = picked {
                        app.settings.selected_game_archive = Some(path);
                        changed = true;
                    }
                }
                if app.settings().selected_game_archive.is_some()
                    && ui.button(t!("editor.prefs.game.clear")).clicked()
                {
                    app.settings.selected_game_archive = None;
                    changed = true;
                }
            });

            ui.add_space(12.0);
            if let Some(p) = Settings::config_path() {
                ui.weak(t!("editor.prefs.saved_to", path = p.display().to_string()));
            }
        });
    app.dialog.show_settings = open;
    if changed {
        app.settings().save();
    }
}

/// Render the About modal when the user has opened it.
pub(crate) fn draw_about(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_about {
        return;
    }
    let mut open = app.dialog.show_about;
    egui::Window::new(t!("editor.app.about"))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.heading(t!("editor.app.title"));
            ui.label(t!("editor.app.version", v = env!("CARGO_PKG_VERSION")));
            ui.add_space(6.0);
            ui.label(t!("editor.app.tagline_about"));
            ui.add_space(6.0);
            ui.hyperlink_to(
                "github.com/burnhamrobertp/bar-editor",
                "https://github.com/burnhamrobertp/bar-editor",
            );
        });
    app.dialog.show_about = open;
}

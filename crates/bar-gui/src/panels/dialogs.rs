//! Modal dialogs that aren't part of the main panel layout —
//! Preferences, About, and (in the future) the file editor and
//! map-info file picker. Confirm-dialog and unsaved-changes
//! prompts stay in `app.rs` for now because they're tightly
//! coupled to the `PendingAction` flow.

use eframe::egui;

use crate::app::BarEditorApp;
use crate::settings::Settings;

/// Render the Preferences modal when the user has opened it.
/// No-op when `dialog.show_settings` is false.
pub(crate) fn draw_settings(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog_show_settings() {
        return;
    }
    let mut open = app.dialog_show_settings();
    let mut changed = false;
    egui::Window::new("Preferences")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.heading("Auto-save");
            let mut autosave_enabled = app.settings().autosave_enabled;
            if ui.checkbox(&mut autosave_enabled, "Enabled").changed() {
                app.settings_mut().autosave_enabled = autosave_enabled;
                changed = true;
            }
            ui.horizontal(|ui| {
                ui.label("Interval:");
                let mut secs = app.settings().autosave_interval_secs as i64;
                if ui
                    .add(egui::Slider::new(&mut secs, 30..=600).suffix(" s"))
                    .changed()
                {
                    app.settings_mut().autosave_interval_secs = secs.max(30) as u64;
                    changed = true;
                }
            });

            ui.add_space(8.0);
            ui.heading("Confirmations");
            let suppressed_count = app.settings().suppressed_confirmations.len();
            if suppressed_count == 0 {
                ui.weak("No confirmations are currently suppressed.");
            } else {
                ui.label(format!(
                    "{suppressed_count} confirmation type(s) suppressed via \"Don't ask again\":"
                ));
                let mut keys: Vec<String> = app
                    .settings()
                    .suppressed_confirmations
                    .iter()
                    .cloned()
                    .collect();
                keys.sort();
                for k in &keys {
                    ui.weak(format!("  • {}", crate::app::confirm_key_display_name(k)));
                }
                if ui.button("Clear suppressed confirmations").clicked() {
                    app.settings_mut().suppressed_confirmations.clear();
                    changed = true;
                }
            }

            ui.add_space(8.0);
            ui.heading("Startup");
            let mut restore = app.settings().restore_last_project;
            if ui
                .checkbox(&mut restore, "Reopen the last project on launch")
                .changed()
            {
                app.settings_mut().restore_last_project = restore;
                changed = true;
            }

            ui.add_space(12.0);
            if let Some(p) = Settings::config_path() {
                ui.weak(format!("Saved to: {}", p.display()));
            }
        });
    app.set_dialog_show_settings(open);
    if changed {
        app.settings().save();
    }
}

/// Render the About modal when the user has opened it.
pub(crate) fn draw_about(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog_show_about() {
        return;
    }
    let mut open = app.dialog_show_about();
    egui::Window::new("About BAR - Map Editor")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.heading("BAR - Map Editor");
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            ui.add_space(6.0);
            ui.label("Standalone map editor for Beyond All Reason.");
            ui.add_space(6.0);
            ui.hyperlink_to(
                "github.com/burnhamrobertp/bar-editor",
                "https://github.com/burnhamrobertp/bar-editor",
            );
        });
    app.set_dialog_show_about(open);
}

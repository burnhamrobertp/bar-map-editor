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
                    ui.weak(format!("  - {}", crate::app::confirm_key_display_name(k)));
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
            ui.label(t!("editor.prefs.game.install_label"));
            ui.horizontal(|ui| {
                let mut path_str = app
                    .settings()
                    .bar_install_path
                    .as_deref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut path_str)
                        .hint_text(t!("editor.prefs.game.install_none"))
                        .desired_width(320.0),
                );
                if response.lost_focus() {
                    let trimmed = path_str.trim();
                    let new_value = if trimmed.is_empty() {
                        None
                    } else {
                        Some(std::path::PathBuf::from(trimmed))
                    };
                    if new_value != app.settings().bar_install_path {
                        app.settings.bar_install_path = new_value;
                        changed = true;
                    }
                }
                if ui.button(t!("editor.prefs.game.browse")).clicked() {
                    let picked = rfd::FileDialog::new().pick_folder();
                    if let Some(path) = picked {
                        app.settings.bar_install_path = Some(path);
                        changed = true;
                    }
                }
                if app.settings().bar_install_path.is_some()
                    && ui.button(t!("editor.prefs.game.clear")).clicked()
                {
                    app.settings.bar_install_path = None;
                    changed = true;
                }
            });

            ui.add_space(4.0);
            ui.label(t!("editor.prefs.game.archive_label"));
            ui.horizontal(|ui| {
                let mut path_str = app
                    .settings()
                    .selected_game_archive
                    .as_deref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut path_str)
                        .hint_text(t!("editor.prefs.game.none"))
                        .desired_width(320.0),
                );
                if response.lost_focus() {
                    let trimmed = path_str.trim();
                    if trimmed.is_empty() {
                        app.settings.selected_game_archive = None;
                    } else {
                        app.settings.selected_game_archive =
                            Some(std::path::PathBuf::from(trimmed));
                    }
                    changed = true;
                }
                if ui.button(t!("editor.prefs.game.browse")).clicked() {
                    // pick_folder lets the user select .sdd directories (which
                    // pick_file would navigate into instead of selecting).
                    // For .sdz/.sd7 files, paste the path directly in the field.
                    let picked = rfd::FileDialog::new().pick_folder();
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

            ui.add_space(8.0);
            ui.heading(t!("editor.prefs.display.heading"));
            ui.weak(t!("editor.prefs.display.hint"));
            let mut grass = app.settings().display.grass;
            if ui
                .checkbox(&mut grass, t!("editor.prefs.display.grass"))
                .changed()
            {
                app.settings.display.grass = grass;
                changed = true;
            }
            // The "Advanced Map Shading" and "Advanced Model Shading"
            // toggles are hidden from the UI today: they were intended
            // as opt-in switches for additional fidelity beyond the
            // baseline, but neither has any rendering attached yet --
            // surfacing them would mislead users into thinking the
            // checkboxes do something. The `DisplayPrefs` fields are
            // kept (with `#[serde(default)]`) so any saved settings
            // referencing them still load cleanly; the renderer
            // plumbing (`TerrainRenderer::advanced_*_shading` +
            // `terrain_detail_params.zw`) stays for the eventual
            // implementations. See `docs/TODO.md` -- "Diagnose the
            // model-shading disparity vs in-engine cus_gl4-on" and
            // "Real `cus_gl4` port..." -- for the work that needs to
            // land before these are worth showing again.

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

/// Centered modal shown while a `.sd7` import is in flight. The
/// step string comes from `app.project.import_status`, which
/// `bar-app::runner` updates from the worker thread's progress
/// callback. Cleared (modal closes) when the worker reports success
/// or failure. Non-dismissable -- the import runs to completion
/// regardless of user input, so no close button.
pub(crate) fn draw_import_progress(app: &BarEditorApp, ctx: &egui::Context) {
    let Some(step) = app.project.import_status.as_deref() else {
        return;
    };
    egui::Window::new(t!("editor.import.title"))
        .resizable(false)
        .collapsible(false)
        .title_bar(false)
        .fixed_size(egui::vec2(340.0, 0.0))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.heading(t!("editor.import.title"));
                ui.add_space(8.0);
                ui.add(egui::Spinner::new().size(28.0));
                ui.add_space(8.0);
                // `Truncate` mode keeps the modal height stable as
                // labels change length; egui adds an ellipsis if the
                // step text overflows the fixed modal width.
                ui.add(
                    egui::Label::new(format!("{step}...")).wrap_mode(egui::TextWrapMode::Truncate),
                );
                ui.add_space(8.0);
            });
        });
    // Keep the GUI loop ticking so the spinner animates.
    ctx.request_repaint();
}

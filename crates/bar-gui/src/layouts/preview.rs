//! Preview layout -- read-only 3D viewport showing the compiled native-resolution
//! BC1 texture.
//!
//! This layout claims only a top info bar. The central panel is left unclaimed
//! so bar-app can fill it with the 3D viewport pointed at the BC1 texture.
//! When BC1 is unavailable (unsupported GPU or no compiled state), the central
//! panel is claimed to show an informative placeholder instead.

use eframe::egui;

use crate::app::BarEditorApp;

/// Draw the Preview layout.
pub fn draw(app: &mut BarEditorApp, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    let is_compiled = app
        .project
        .path
        .as_deref()
        .map(|p| {
            bar_project::PackageDir::open(p)
                .map(|pkg| pkg.is_compiled())
                .unwrap_or(false)
        })
        .unwrap_or(false);

    egui::TopBottomPanel::top("preview_info_bar").show(ctx, |ui| {
        draw_info_bar(app, ui, is_compiled);
    });

    let can_show_viewport = app.supports_bc && is_compiled;

    if !can_show_viewport {
        egui::CentralPanel::default().show(ctx, |ui| {
            draw_placeholder(app, ui, is_compiled);
        });
        return;
    }

    // Signal bar-app to load the compiled BC1 texture this frame.
    app.preview.bc_texture_requested = true;
}

fn draw_info_bar(app: &mut BarEditorApp, ui: &mut egui::Ui, is_compiled: bool) {
    ui.horizontal(|ui| {
        ui.strong("Preview");
        ui.separator();

        if !is_compiled {
            ui.label("Not yet compiled");
        } else if let Some(compiled_at) = app.project.compiled_at {
            let secs = compiled_at.elapsed().as_secs();
            let age = if secs < 60 {
                format!("Compiled {secs}s ago")
            } else if secs < 3600 {
                format!("Compiled {}m ago", secs / 60)
            } else {
                format!("Compiled {}h ago", secs / 3600)
            };
            if app.project.compile_dirty {
                ui.colored_label(egui::Color32::from_rgb(220, 160, 40), "Out of date");
                ui.separator();
                ui.weak(&age);
            } else {
                ui.label(age);
            }
        } else {
            // Compiled on disk but not in this session (no compiled_at timestamp).
            if app.project.compile_dirty {
                ui.colored_label(egui::Color32::from_rgb(220, 160, 40), "Out of date");
            } else {
                ui.label("Compiled");
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let running = app.preview.compile_running;
            let any_running = running || app.preview.export_status().is_running();

            if running {
                ui.weak("Compiling...");
            } else {
                let label = if app.project.compile_dirty || !is_compiled {
                    if is_compiled {
                        "Recompile"
                    } else {
                        "Compile"
                    }
                } else {
                    "Recompile"
                };
                if ui
                    .add_enabled(!any_running, egui::Button::new(label))
                    .clicked()
                {
                    app.preview.compile_requested = true;
                }
            }
        });
    });
}

fn draw_placeholder(app: &BarEditorApp, ui: &mut egui::Ui, is_compiled: bool) {
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            if !app.supports_bc {
                ui.heading("BC texture compression unavailable");
                ui.add_space(8.0);
                ui.label("Your GPU does not support BC1/DXT1 texture compression.");
                ui.label("The native-resolution Preview layout requires it.");
            } else if !is_compiled {
                ui.heading("Not yet compiled");
                ui.add_space(8.0);
                ui.label("Run Compile to generate the native-resolution texture.");
                ui.add_space(16.0);
                if !app.preview.compile_running {
                    ui.label("Use the Compile button above or in the toolbar.");
                } else {
                    ui.label("Compiling...");
                }
            }
        });
    });
}

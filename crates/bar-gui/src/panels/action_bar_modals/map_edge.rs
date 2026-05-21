//! Map Edge modal -- the mirrored area surrounding the playable map
//! (engine widget: `luaui/Widgets/map_edge_extension2.lua`).
//!
//! Holds the `grassShadingTex` picker / preview today. Future
//! map-edge knobs (curvature bend strength, mirror detail density)
//! land in this modal rather than crowding the main Map Info
//! group. The cached preview texture lives on `BarEditorApp.map_edge`
//! so flipping the modal open / closed doesn't re-decode the file.

use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::modal_frame;
use crate::panels::field_editor::heading_with_info;

/// Per-session preview state for the modal. Lives on `BarEditorApp`
/// so it survives modal open / close cycles; reset on project switch.
#[derive(Default)]
pub struct MapEdgePanelState {
    pub(crate) preview: PreviewCache,
}

/// Per-session cache of the decoded preview texture so we don't
/// re-load the file each frame. Keyed by `(project_dir, filename)`
/// so a project switch or a file swap invalidates the cache.
#[derive(Default)]
pub(crate) struct PreviewCache {
    key: Option<(std::path::PathBuf, String)>,
    texture: Option<egui::TextureHandle>,
}

impl PreviewCache {
    fn ensure(
        &mut self,
        ctx: &egui::Context,
        project_dir: &std::path::Path,
        filename: &str,
    ) -> Option<&egui::TextureHandle> {
        let key = (project_dir.to_path_buf(), filename.to_string());
        if self.key.as_ref() == Some(&key) {
            return self.texture.as_ref();
        }
        self.key = Some(key);
        self.texture = decode_preview(project_dir, filename).map(|(rgba, w, h)| {
            let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            ctx.load_texture(
                "map_edge_grass_shading_preview",
                image,
                egui::TextureOptions::LINEAR,
            )
        });
        self.texture.as_ref()
    }

    fn invalidate(&mut self) {
        self.key = None;
        self.texture = None;
    }
}

fn resolve_grass_shading_path(
    project_dir: &std::path::Path,
    filename: &str,
) -> Option<std::path::PathBuf> {
    if filename.is_empty() {
        let minimap = project_dir.join(bar_project::SMF_MINIMAP_SIDE_CAR);
        return minimap.is_file().then_some(minimap);
    }
    bar_project::find_file_in_dir(&project_dir.join("passthrough"), filename)
        .or_else(|| bar_project::find_file_in_dir(project_dir, filename))
}

fn decode_preview(project_dir: &std::path::Path, filename: &str) -> Option<(Vec<u8>, u32, u32)> {
    let path = resolve_grass_shading_path(project_dir, filename)?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if ext == "dds" {
        if let Ok((rgba, w, h)) = bar_data::load_dds_2d(&path) {
            return Some((rgba, w, h));
        }
    }
    let bytes = std::fs::read(&path).ok()?;
    let fmt = image::ImageFormat::from_extension(&ext)?;
    let img = image::load_from_memory_with_format(&bytes, fmt).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Some((rgba.into_raw(), w, h))
}

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_map_edge_editor {
        return;
    }
    let mut open = app.dialog.show_map_edge_editor;
    let project_path_opt: Option<std::path::PathBuf> = app
        .project
        .path
        .clone()
        .or_else(|| app.project.pending_map_data_dir.clone());

    modal_frame(ctx, &mut open, "Map Edge", "map_edge_editor_modal", |ui| {
        heading_with_info(
            ui,
            "Grass shading texture",
            "Sampled at the mirrored UV around the playable area. \
             Empty falls back to the SMF-embedded minimap. Example: \
             Onyx Cauldron uses this to show off-map rocks instead \
             of the mirrored metal spots.",
        );

        let mut filename = app.map_settings().resources.grass_shading_tex.clone();
        let mut text_started = false;
        let mut text_committed = false;
        let mut text_changed = false;
        let mut atomic_change = false;

        ui.horizontal(|ui| {
            ui.label("File");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut filename)
                        .hint_text("(empty = SMF minimap fallback)")
                        .desired_width(260.0),
                );
                if resp.gained_focus() {
                    text_started = true;
                }
                if resp.changed() {
                    text_changed = true;
                }
                if resp.lost_focus() {
                    text_committed = true;
                }
            });
        });

        ui.horizontal(|ui| {
            if ui.button("Browse...").clicked() {
                if let Some(picked) = rfd::FileDialog::new()
                    .set_title("Select grass shading texture")
                    .add_filter("Image", &["dds", "png", "jpg", "jpeg", "tga", "bmp"])
                    .pick_file()
                {
                    let picked_name = picked
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string());
                    if let (Some(name), Some(project_dir)) =
                        (picked_name, project_path_opt.as_deref())
                    {
                        let dst_dir = project_dir.join("passthrough");
                        let dst = dst_dir.join(&name);
                        let copy_result = std::fs::create_dir_all(&dst_dir)
                            .and_then(|_| std::fs::copy(&picked, &dst));
                        match copy_result {
                            Ok(_) => {
                                filename = name;
                                atomic_change = true;
                            }
                            Err(e) => {
                                tracing::warn!(err = %e, "Failed to copy grassShadingTex");
                            }
                        }
                    } else if let Some(name) = picked
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string())
                    {
                        filename = name;
                        atomic_change = true;
                    }
                }
            }
            if !filename.is_empty() && ui.button("Clear (use minimap)").clicked() {
                filename.clear();
                atomic_change = true;
            }
        });

        if text_started && app.dialog.field_edit_in_progress.is_none() {
            let snap = app.snapshot("Edit grassShadingTex");
            app.dialog.field_edit_in_progress = Some(snap);
        }
        if text_changed {
            app.map_settings_mut().resources.grass_shading_tex = filename.clone();
            app.map_edge.preview.invalidate();
        }
        if text_committed {
            if let Some(snap) = app.dialog.field_edit_in_progress.take() {
                app.history.push(snap);
            }
            app.mark_dirty();
        }
        if atomic_change {
            app.push_undo("Edit grassShadingTex");
            app.map_settings_mut().resources.grass_shading_tex = filename.clone();
            app.map_edge.preview.invalidate();
        }

        ui.add_space(12.0);
        ui.label(if filename.is_empty() {
            "Preview: SMF-embedded minimap (engine default)"
        } else {
            "Preview: custom grassShadingTex"
        });

        let active_filename = filename.clone();
        let project_dir_opt = project_path_opt.clone();
        let preview_cache = &mut app.map_edge.preview;
        let max_side = 384.0_f32;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_size(egui::vec2(max_side + 8.0, max_side + 8.0));
            ui.with_layout(
                egui::Layout::centered_and_justified(egui::Direction::TopDown),
                |ui| {
                    let handle = project_dir_opt
                        .as_deref()
                        .and_then(|p| preview_cache.ensure(ctx, p, &active_filename));
                    if let Some(tex) = handle {
                        let [w, h] = tex.size();
                        let aspect = w as f32 / h.max(1) as f32;
                        let display_size = if aspect >= 1.0 {
                            egui::vec2(max_side, max_side / aspect)
                        } else {
                            egui::vec2(max_side * aspect, max_side)
                        };
                        ui.image((tex.id(), display_size));
                    } else {
                        ui.label(
                            "No preview available. The SMF minimap sidecar is generated \
                             on .sd7 import and copied into the .barproj on save; for \
                             freshly-imported projects, save once to populate it.",
                        );
                    }
                },
            );
        });
    });

    app.dialog.show_map_edge_editor = open;
}

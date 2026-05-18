//! Map Edge editor — dedicated modal for the mirrored area surrounding
//! the playable map (engine widget: `luaui/Widgets/map_edge_extension2.lua`).
//!
//! Holds the `grassShadingTex` picker / preview today. Future map-edge
//! knobs (curvature bend strength, atmosphere fog tuning, mirror detail
//! density) will land in the same panel rather than cramming into the
//! main Map Settings modal. The session keeps a cached egui texture
//! handle for the current preview so flipping the modal open/closed
//! doesn't re-decode the file.

use eframe::egui;

use crate::app::BarEditorApp;

/// Per-panel session state held on `BarEditorApp` so the preview texture
/// survives between frames. Reset on project switch by `reset_project`.
#[derive(Default)]
pub struct MapEdgePanelState {
    pub(crate) preview: PreviewCache,
}

/// Per-session cache of the decoded preview texture so we don't re-load
/// the file each frame. Keyed by `(project_dir, filename)` so a project
/// switch or a file swap invalidates the cache.
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

/// Walk the project's known asset locations for the picker filename.
/// Mirrors the order `sync_grass_shading_tex` uses at the renderer side
/// (passthrough/ first, then project root) so the preview is sourced
/// from the same file the renderer will sample.
fn resolve_grass_shading_path(
    project_dir: &std::path::Path,
    filename: &str,
) -> Option<std::path::PathBuf> {
    if filename.is_empty() {
        let minimap = project_dir.join(bar_project::SMF_MINIMAP_SIDE_CAR);
        return minimap.is_file().then_some(minimap);
    }
    let candidates = [
        project_dir.join("passthrough").join(filename),
        project_dir.join(filename),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

/// Decode the preview image to RGBA8 + dimensions for egui upload.
/// Returns `None` when the file is missing or the format isn't one
/// we can decode.
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

    egui::Window::new("Map Edge")
        .open(&mut open)
        .resizable(true)
        .collapsible(false)
        .default_size([480.0, 520.0])
        .show(ctx, |ui| {
            ui.label(
                "Configure the mirrored area that surrounds the playable map. \
                 Engine widget: `luaui/Widgets/map_edge_extension2.lua`. \
                 The texture below is sampled at the mirrored UV; when unset \
                 the SMF-embedded minimap is used (engine default \
                 `MAP_BASE_GRASS_TEX`).",
            );

            ui.add_space(12.0);
            ui.heading("Grass shading texture");
            ui.label(
                "Mapinfo `resources.grassShadingTex`. Custom override for the \
                 off-map area -- e.g. Onyx Cauldron uses this to show off-map \
                 rocks instead of mirrored metal spots.",
            );

            let resources = &mut app.map_settings_mut().resources;
            let mut filename = resources.grass_shading_tex.clone();
            let mut changed = false;

            ui.horizontal(|ui| {
                ui.label("File");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut filename)
                            .hint_text("(empty = SMF minimap fallback)")
                            .desired_width(260.0),
                    );
                    if resp.changed() {
                        changed = true;
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
                                    changed = true;
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        err = %e,
                                        "Failed to copy grassShadingTex"
                                    );
                                }
                            }
                        } else if let Some(name) = picked
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(|s| s.to_string())
                        {
                            filename = name;
                            changed = true;
                        }
                    }
                }
                if !filename.is_empty() && ui.button("Clear (use minimap)").clicked() {
                    filename.clear();
                    changed = true;
                }
            });

            if changed {
                resources.grass_shading_tex = filename.clone();
                app.project.is_dirty = true;
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

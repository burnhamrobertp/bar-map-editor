//! Dimensions modal -- map width / height (in /64 increments, the
//! engine convention), min / max height in elmos, and the minimap
//! override picker + preview.
//!
//! Bespoke: width and height live on `MapState`, not `MapSettings`,
//! and they're exposed in quantised heightmap-square units rather
//! than the raw cell count. Min / max height drag through
//! `MapState::height_range_mut`.
//!
//! The minimap field stores a basename inside `passthrough/` (or the
//! SMF-sidecar `_bme_smf_minimap.png` when unset); the preview decodes
//! whichever file resolves and downscales to a fixed display size so
//! opening the modal stays snappy on large source images.

use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::{
    drive_drag_intent, drive_text_edit_intent, modal_frame,
};
use crate::panels::field_editor::section_heading;
use crate::panels::file_picker::FilePickerField;
use crate::panels::image_preview::PreviewCache;
use crate::t;

#[derive(Default)]
pub struct DimensionsPanelState {
    pub(crate) minimap_preview: PreviewCache,
}

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_dimensions_editor {
        return;
    }
    let mut open = app.dialog.show_dimensions_editor;
    let project_path_opt: Option<std::path::PathBuf> = app
        .project
        .path
        .clone()
        .or_else(|| app.project.pending_map_data_dir.clone());
    let parent_window = app.parent_window();
    modal_frame(
        ctx,
        &mut open,
        &t!("editor.modals.dimensions.title"),
        "dimensions_editor_modal",
        |ui| {
            draw_size(ui, app);
            draw_height_range(ui, app);
            ui.add_space(12.0);
            draw_minimap(
                ui,
                app,
                ctx,
                project_path_opt.as_deref(),
                parent_window.as_ref(),
            );
        },
    );
    app.dialog.show_dimensions_editor = open;
}

fn draw_size(ui: &mut egui::Ui, app: &mut BarEditorApp) {
    let (w_cur, h_cur) = (app.map.width, app.map.height);
    let wid = egui::Id::new("map_dim_w");
    let hid = egui::Id::new("map_dim_h");
    let wv = w_cur.saturating_sub(1) / 64;
    let hv = h_cur.saturating_sub(1) / 64;
    let mut ws: String = ui
        .data(|d| d.get_temp::<String>(wid))
        .unwrap_or_else(|| wv.to_string());
    let mut hs: String = ui
        .data(|d| d.get_temp::<String>(hid))
        .unwrap_or_else(|| hv.to_string());
    let mut wr_taken = None;
    let mut hr_taken = None;
    ui.horizontal(|ui| {
        ui.label(t!("editor.map_settings.map_size_label"));
        let wr = ui.add_sized([30.0, 18.0], egui::TextEdit::singleline(&mut ws).id(wid));
        crate::panels::widgets::select_all_on_focus(ui, &wr, &ws);
        wr_taken = Some(wr);
        ui.label(t!("editor.modals.dimensions.size_separator"));
        let hr = ui.add_sized([30.0, 18.0], egui::TextEdit::singleline(&mut hs).id(hid));
        crate::panels::widgets::select_all_on_focus(ui, &hr, &hs);
        hr_taken = Some(hr);
    });
    let wr = wr_taken.expect("width response captured above");
    let hr = hr_taken.expect("height response captured above");
    ui.data_mut(|d| d.insert_temp(wid, ws.clone()));
    ui.data_mut(|d| d.insert_temp(hid, hs.clone()));

    if wr.lost_focus() {
        let nv = ws
            .trim()
            .parse::<u32>()
            .map(|v| v.clamp(1, 512))
            .unwrap_or(wv);
        ui.data_mut(|d| d.insert_temp(wid, nv.to_string()));
        let (w, _) = app.map_dimensions_mut();
        *w = nv * 64 + 1;
    }
    if hr.lost_focus() {
        let nv = hs
            .trim()
            .parse::<u32>()
            .map(|v| v.clamp(1, 512))
            .unwrap_or(hv);
        ui.data_mut(|d| d.insert_temp(hid, nv.to_string()));
        let (_, h) = app.map_dimensions_mut();
        *h = nv * 64 + 1;
    }
    let width_label = t!("editor.modals.dimensions.undo_width");
    let height_label = t!("editor.modals.dimensions.undo_height");
    drive_text_edit_intent(app, &wr, &width_label, wr.changed());
    drive_text_edit_intent(app, &hr, &height_label, hr.changed());
}

fn draw_height_range(ui: &mut egui::Ui, app: &mut BarEditorApp) {
    let (mn, mx) = app.map_height_range_mut();
    let mut min_val = *mn;
    let mut max_val = *mx;
    let mut min_resp = None;
    let mut max_resp = None;
    let mut min_sl = None;
    let mut max_sl = None;
    ui.horizontal(|ui| {
        ui.label(t!("editor.modals.dimensions.field.min_height"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            min_resp = Some(
                ui.add(
                    egui::DragValue::new(&mut min_val)
                        .range(-2000.0..=4000.0)
                        .speed(1.0),
                ),
            );
            ui.spacing_mut().slider_width = (ui.available_width() - 8.0).max(40.0);
            min_sl = Some(
                ui.add(
                    egui::Slider::new(&mut min_val, -500.0..=2000.0)
                        .show_value(false)
                        .clamping(egui::SliderClamping::Never),
                ),
            );
        });
    });
    ui.horizontal(|ui| {
        ui.label(t!("editor.modals.dimensions.field.max_height"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            max_resp = Some(
                ui.add(
                    egui::DragValue::new(&mut max_val)
                        .range(-2000.0..=4000.0)
                        .speed(1.0),
                ),
            );
            ui.spacing_mut().slider_width = (ui.available_width() - 8.0).max(40.0);
            max_sl = Some(
                ui.add(
                    egui::Slider::new(&mut max_val, -500.0..=2000.0)
                        .show_value(false)
                        .clamping(egui::SliderClamping::Never),
                ),
            );
        });
    });
    let min_resp = min_resp.expect("min response captured above");
    let max_resp = max_resp.expect("max response captured above");
    let min_sl = min_sl.expect("min slider captured above");
    let max_sl = max_sl.expect("max slider captured above");
    let (mn_mut, mx_mut) = app.map_height_range_mut();
    if min_resp.changed() || min_sl.changed() {
        *mn_mut = min_val;
    }
    if max_resp.changed() || max_sl.changed() {
        *mx_mut = max_val;
    }
    let min_label = t!("editor.modals.dimensions.field.min_height");
    let max_label = t!("editor.modals.dimensions.field.max_height");
    drive_drag_intent(app, &min_resp, &min_label);
    drive_drag_intent(app, &min_sl, &min_label);
    drive_drag_intent(app, &max_resp, &max_label);
    drive_drag_intent(app, &max_sl, &max_label);
}

/// Minimap override: an optional source file (DDS / PNG / TGA / etc.)
/// that the bundler embeds into the SMF in place of the auto-generated
/// terrain-derived minimap. Empty preview falls back to the SMF
/// sidecar from import, when present.
fn draw_minimap(
    ui: &mut egui::Ui,
    app: &mut BarEditorApp,
    ctx: &egui::Context,
    project_dir: Option<&std::path::Path>,
    parent: Option<&crate::io::dialogs::ParentWindow>,
) {
    section_heading(ui, &t!("editor.modals.dimensions.minimap_heading"));

    let mut filename = app.map_settings().minimap.clone().unwrap_or_default();
    let file_label = t!("editor.modals.dimensions.minimap_file_label");
    let dialog_title = t!("editor.modals.dimensions.minimap_dialog_title");
    let hint = t!("editor.modals.dimensions.minimap_hint");
    let changed = FilePickerField::new(&file_label, "passthrough")
        .extensions(&["dds", "png", "jpg", "jpeg", "tga", "bmp"])
        .title(&dialog_title)
        .allow_clear(true)
        .hint(&hint)
        .show(ui, &mut filename, project_dir, parent);
    if changed {
        app.push_undo(&t!("editor.modals.dimensions.undo_minimap"));
        app.map_settings_mut().minimap = if filename.is_empty() {
            None
        } else {
            Some(filename.clone())
        };
        app.dimensions.minimap_preview.invalidate();
    }

    ui.add_space(8.0);
    ui.label(if filename.is_empty() {
        t!("editor.modals.dimensions.preview_smf")
    } else {
        t!("editor.modals.dimensions.preview_custom")
    });

    let max_side = 256.0_f32;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_size(egui::vec2(max_side + 8.0, max_side + 8.0));
        ui.with_layout(
            egui::Layout::centered_and_justified(egui::Direction::TopDown),
            |ui| {
                let handle = project_dir.and_then(|p| {
                    app.dimensions.minimap_preview.ensure(
                        ctx,
                        p,
                        &filename,
                        max_side as u32,
                        "dimensions_minimap_preview",
                        resolve_minimap_path,
                    )
                });
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
                    ui.label(t!("editor.modals.dimensions.preview_unavailable"));
                }
            },
        );
    });
}

fn resolve_minimap_path(
    project_dir: &std::path::Path,
    filename: &str,
) -> Option<std::path::PathBuf> {
    if filename.is_empty() {
        let sidecar = project_dir.join(bar_project::SMF_MINIMAP_SIDE_CAR);
        return sidecar.is_file().then_some(sidecar);
    }
    bar_project::find_file_in_dir(&project_dir.join("passthrough"), filename)
        .or_else(|| bar_project::find_file_in_dir(project_dir, filename))
}

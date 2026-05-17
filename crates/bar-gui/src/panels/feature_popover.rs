//! Floating popover for the selected feature.
//!
//! Anchors next to the projected screen-space position of the
//! feature in the 3D viewport (similar in spirit to node-property
//! popups on the graph canvas). Shows feature details + any
//! per-feature lights BAR's deferred-rendering widget would attach
//! in-engine.
//!
//! Projection math lives at the call site (it needs the renderer's
//! camera + map dims, which this crate doesn't model). Caller passes
//! the precomputed anchor; this module renders the panel at it.

use eframe::egui;
use glam::Vec4;

use crate::app::BarEditorApp;
use crate::panels::icons;
use crate::t;

/// Map-extent inputs needed to project a feature's elmo-space
/// position into render space. Caller derives these from the active
/// project / frame.
const POPOVER_WIDTH: f32 = 230.0;
const TYPE_COMBO_WIDTH: f32 = 150.0;

pub struct PopoverDims {
    pub map_w: u32,
    pub map_h: u32,
    pub min_height: f32,
    pub max_height: f32,
    pub x_extent: f32,
    pub z_extent: f32,
    pub height_scale: f32,
}

/// Draw the floating feature-info popover next to the selected
/// feature. No-op when no feature is selected.
pub fn draw(ctx: &egui::Context, app: &mut BarEditorApp, screen_anchor: egui::Pos2) {
    let Some(idx) = app.map.selected_feature_idx else {
        return;
    };
    let Some(feature) = app.map.features.get(idx).cloned() else {
        return;
    };

    // Offset to the upper-right of the projected point so the feature
    // itself isn't covered. egui's Area auto-bounds inside the screen
    // rect, which keeps the popover readable near viewport edges.
    let popover_pos = egui::pos2(screen_anchor.x + 18.0, screen_anchor.y - 8.0);

    egui::Area::new(egui::Id::new("feature_popover"))
        .fixed_pos(popover_pos)
        .order(egui::Order::Foreground)
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_unmultiplied(20, 22, 30, 235))
                .stroke(egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgba_unmultiplied(120, 120, 140, 220),
                ))
                .corner_radius(6.0)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_width(POPOVER_WIDTH);
                    draw_body(ui, app, idx, &feature);
                });
        });
}

fn draw_body(
    ui: &mut egui::Ui,
    app: &mut BarEditorApp,
    idx: usize,
    feature: &bar_project::recipe::PlacedFeature,
) {
    // Header row: trash button right-aligned in a single horizontal
    // strip. The body's first row (Type) carries the feature name so
    // we don't need a separate title.
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let trash_size = egui::vec2(16.0, 16.0);
            let (rect, resp) = ui.allocate_exact_size(trash_size, egui::Sense::click());
            let color = if resp.hovered() {
                egui::Color32::from_rgb(255, 110, 110)
            } else {
                egui::Color32::from_rgb(210, 80, 80)
            };
            icons::paint_trash_icon(ui.painter(), rect, color);
            let resp = resp.on_hover_text(t!("editor.common.delete"));
            if resp.clicked() {
                app.push_undo("Delete feature");
                app.map.features.remove(idx);
                app.map.selected_feature_idx = None;
                app.map.features_placement_dirty = true;
            }
        });
    });

    egui::Grid::new("feature_popover_info_grid")
        .num_columns(2)
        .spacing([8.0, 2.0])
        .show(ui, |ui| {
            ui.weak(t!("editor.feature_popover.field.type"));
            let mut current_type = feature.feature_type.clone();
            let prev_type = current_type.clone();
            let palette = app.feature_palette_names.clone();
            egui::ComboBox::from_id_salt("feature_popover_type_combo")
                .width(TYPE_COMBO_WIDTH)
                .selected_text(&current_type)
                .show_ui(ui, |ui| {
                    for name in &palette {
                        ui.selectable_value(&mut current_type, name.clone(), name);
                    }
                });
            if current_type != prev_type {
                app.push_undo("Change feature type");
                if let Some(f) = app.map.features.get_mut(idx) {
                    f.feature_type = current_type;
                }
                app.map.features_placement_dirty = true;
            }
            ui.end_row();
            ui.weak(t!("editor.feature_popover.field.position"));
            ui.label(format!(
                "{:.1}, {:.1}, {:.1}",
                feature.x, feature.y, feature.z
            ));
            ui.end_row();
            ui.weak(t!("editor.feature_popover.field.angle"));
            ui.label(format!(
                "{:.1} {}",
                feature.angle,
                t!("editor.feature_popover.field.angle_unit")
            ));
            ui.end_row();
        });

    let lights = bar_render::lights_for_feature_def(&feature.feature_type);
    if lights.is_empty() {
        return;
    }

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.strong(t!("editor.feature_popover.feature_lights.title"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let icon_size = egui::vec2(16.0, 16.0);
            let (rect, resp) = ui.allocate_exact_size(icon_size, egui::Sense::hover());
            let color = if resp.hovered() {
                egui::Color32::from_rgb(220, 220, 240)
            } else {
                egui::Color32::from_rgb(160, 160, 180)
            };
            icons::paint_info_icon(ui.painter(), rect, color);
            resp.on_hover_text(t!("editor.feature_popover.feature_lights.info_tooltip"));
        });
    });
    ui.add_space(4.0);

    for (li, light) in lights.iter().enumerate() {
        if lights.len() > 1 {
            ui.weak(t!(
                "editor.feature_popover.feature_lights.light_n",
                n = (li + 1)
            ));
        }
        egui::Grid::new(format!("feature_popover_light_{}", li))
            .num_columns(2)
            .spacing([8.0, 2.0])
            .show(ui, |ui| {
                ui.weak(t!("editor.feature_popover.feature_lights.field.colour"));
                ui.horizontal(|ui| {
                    let swatch = egui::Color32::from_rgb(
                        (light.color[0].clamp(0.0, 1.0) * 255.0) as u8,
                        (light.color[1].clamp(0.0, 1.0) * 255.0) as u8,
                        (light.color[2].clamp(0.0, 1.0) * 255.0) as u8,
                    );
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, swatch);
                    ui.label(format!(
                        "{:.2}, {:.2}, {:.2}",
                        light.color[0], light.color[1], light.color[2]
                    ));
                });
                ui.end_row();
                ui.weak(t!("editor.feature_popover.feature_lights.field.radius"));
                ui.label(format!(
                    "{:.0} {}",
                    light.radius,
                    t!("editor.feature_popover.feature_lights.field.radius_unit")
                ));
                ui.end_row();
                ui.weak(t!("editor.feature_popover.feature_lights.field.offset"));
                ui.label(format!(
                    "{:.0}, {:.0}, {:.0} {}",
                    light.offset[0],
                    light.offset[1],
                    light.offset[2],
                    t!("editor.feature_popover.feature_lights.field.offset_unit")
                ));
                ui.end_row();
                ui.weak(t!("editor.feature_popover.feature_lights.field.intensity"));
                ui.label(format!("{:.2}", light.intensity));
                ui.end_row();
            });
        if li + 1 < lights.len() {
            ui.add_space(4.0);
        }
    }
}

/// Convert a feature's elmo-space position into screen-space pixels.
/// Returns `None` when the projection lands outside the camera
/// frustum or behind the near plane.
pub fn project_feature_to_screen(
    feature: &bar_project::recipe::PlacedFeature,
    dims: &PopoverDims,
    heightmap: Option<&bar_data::Heightmap>,
    view_projection: glam::Mat4,
    viewport_rect: egui::Rect,
) -> Option<egui::Pos2> {
    let pw = (dims.map_w as f32 - 1.0).max(1.0);
    let ph = (dims.map_h as f32 - 1.0).max(1.0);
    let height_range = (dims.max_height - dims.min_height).abs().max(1.0);

    let rx = (feature.x / (pw * 8.0) - 0.5) * 2.0 * dims.x_extent;
    let rz = (feature.z / (ph * 8.0) - 0.5) * 2.0 * dims.z_extent;
    // Y: explicitly-authored feature.y wins; otherwise sample terrain
    // at this XZ -- same fallback the renderer uses for placement.
    let ry = if feature.y.abs() < 0.01 {
        heightmap
            .and_then(|hm| {
                bar_render::terrain_y_at_world_xz(
                    rx,
                    rz,
                    hm,
                    dims.x_extent,
                    dims.z_extent,
                    dims.height_scale,
                )
            })
            .unwrap_or(dims.height_scale * 0.5)
    } else {
        ((feature.y - dims.min_height) / height_range) * dims.height_scale
    };

    let clip = view_projection * Vec4::new(rx, ry, rz, 1.0);
    if clip.w <= 1e-4 {
        return None;
    }
    let ndc_x = clip.x / clip.w;
    let ndc_y = clip.y / clip.w;
    let ndc_z = clip.z / clip.w;
    if !(0.0..=1.0).contains(&ndc_z)
        || !(-1.0..=1.0).contains(&ndc_x)
        || !(-1.0..=1.0).contains(&ndc_y)
    {
        return None;
    }
    let sx = (ndc_x * 0.5 + 0.5) * viewport_rect.width() + viewport_rect.left();
    let sy = (1.0 - (ndc_y * 0.5 + 0.5)) * viewport_rect.height() + viewport_rect.top();
    Some(egui::pos2(sx, sy))
}

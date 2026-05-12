//! 2D Inspector window — heightmap backdrop + draggable spawn
//! markers (Spawns mode) or live brush sculpting (Sculpt mode).
//! The window is the cross-cutting workspace for everything you'd
//! want to do on a 2D map view: spawn placement, brush controls,
//! sculpt feedback, and "save heightmap as PNG" as the bridge
//! into the export path.

use eframe::egui;

use crate::app::{
    apply_brush_dab, heightmap_to_color_image, save_heightmap_as_png16, BarEditorApp, BrushTool,
    InspectorMode,
};

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_inspector {
        return;
    }

    // Refresh the cached egui texture if the heightmap has changed.
    if app.paint().heightmap_rev != app.paint().texture_rev {
        let img_and_rev = app.paint().heightmap.as_ref().map(|hm| {
            let (mn, mx) = app.map.height_range();
            (
                heightmap_to_color_image(hm, mn, mx),
                app.paint().heightmap_rev,
            )
        });
        if let Some((img, rev)) = img_and_rev {
            let tex = ctx.load_texture("inspector_heightmap", img, egui::TextureOptions::LINEAR);
            let p = app.paint_mut();
            p.texture = Some(tex);
            p.texture_rev = rev;
        }
    }

    let mut open = app.dialog.show_inspector;
    let mut to_remove: Option<usize> = None;
    let mut new_drag: Option<usize> = None;
    let mut spawn_to_add: Option<[u32; 2]> = None;
    let (map_w, map_h) = app.map.dimensions();
    // Elmo bounds (1 heightmap pixel = 8 elmos in X/Z).
    let world_w = (map_w.saturating_sub(1)) * 8;
    let world_h = (map_h.saturating_sub(1)) * 8;
    // Brush stroke aggregation: accumulate dab points across this
    // frame so we can apply them to the heightmap once after the
    // window closure has released its borrow on `app`.
    let mut sculpt_dabs: Vec<egui::Pos2> = Vec::new();
    let mut stroke_just_started = false;
    let mut stroke_just_ended = false;
    let mut hover_pos_in_rect: Option<egui::Pos2> = None;
    let mut last_rect: Option<egui::Rect> = None;
    let mut clicked_save_png = false;

    egui::Window::new("2D Inspector")
        .open(&mut open)
        .resizable(true)
        .default_size([540.0, 620.0])
        .show(ctx, |ui| {
            // Mode selector at the top of the window.
            ui.horizontal(|ui| {
                ui.label("Mode:");
                let p = app.paint_mut();
                ui.selectable_value(&mut p.inspector_mode, InspectorMode::Spawns, "Start positions");
                ui.selectable_value(&mut p.inspector_mode, InspectorMode::Sculpt, "Sculpt");
            });
            match app.paint().inspector_mode {
                InspectorMode::Spawns => {
                    ui.label("Click to add a start position; drag to move; right-click to delete.");
                }
                InspectorMode::Sculpt => {
                    ui.label("Drag-paint to sculpt the heightmap. Preview-only - doesn't yet write back into the graph.");
                }
            }
            ui.separator();

            // Brush controls (only shown in Sculpt mode).
            if app.paint().inspector_mode == InspectorMode::Sculpt {
                ui.horizontal(|ui| {
                    let current_tool = app.paint().brush.tool;
                    for tool in [BrushTool::Raise, BrushTool::Lower, BrushTool::Smooth, BrushTool::Flatten] {
                        let resp = ui.add(egui::SelectableLabel::new(current_tool == tool, tool.label()));
                        if resp.clicked() {
                            app.paint_mut().brush.tool = tool;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Radius");
                    ui.add(
                        egui::Slider::new(&mut app.paint_mut().brush.radius_px, 2.0..=128.0)
                            .clamping(egui::SliderClamping::Always),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Strength");
                    ui.add(
                        egui::Slider::new(&mut app.paint_mut().brush.strength, 0.001..=0.1)
                            .clamping(egui::SliderClamping::Always)
                            .logarithmic(true),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Falloff");
                    ui.add(
                        egui::Slider::new(&mut app.paint_mut().brush.falloff, 0.5..=4.0)
                            .clamping(egui::SliderClamping::Always),
                    );
                });
                ui.separator();
            }

            // Reserve a square-ish area for the map view.
            let avail = ui.available_size();
            let side = avail.x.min(avail.y - 30.0).max(64.0);
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(side, side),
                egui::Sense::click_and_drag(),
            );
            last_rect = Some(rect);
            let painter = ui.painter_at(rect);

            // Backdrop.
            if let Some(tex_id) = app.paint().texture.as_ref().map(|t| t.id()) {
                let img = egui::Image::from_texture(egui::load::SizedTexture::new(tex_id, rect.size()));
                img.paint_at(ui, rect);
            } else {
                painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(28, 35, 50));
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Open a project / preview to see the heightmap",
                    egui::FontId::proportional(13.0),
                    egui::Color32::from_rgb(150, 150, 170),
                );
            }

            // Convert elmo XZ <-> screen pixel inside `rect`.
            let to_screen = |elmo_x: u32, elmo_z: u32| -> egui::Pos2 {
                let u = if world_w > 0 { elmo_x as f32 / world_w as f32 } else { 0.5 };
                let v = if world_h > 0 { elmo_z as f32 / world_h as f32 } else { 0.5 };
                egui::pos2(rect.left() + u * rect.width(), rect.top() + v * rect.height())
            };
            let to_world = |pos: egui::Pos2| -> [u32; 2] {
                let u = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                let v = ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
                [(u * world_w as f32) as u32, (v * world_h as f32) as u32]
            };

            let pointer = ctx.pointer_latest_pos();

            match app.paint().inspector_mode {
                InspectorMode::Spawns => {
                    // Draw existing markers.
                    let marker_radius = 9.0;
                    let spawns: Vec<[u32; 2]> = app.map_settings_mut().start_positions.clone();
                    let dragging_idx = app.dragging_spawn();
                    for (i, [x, z]) in spawns.iter().enumerate() {
                        let p = to_screen(*x, *z);
                        let hit_dist = pointer.map_or(f32::INFINITY, |pp| pp.distance(p));
                        let hovered = hit_dist <= marker_radius + 2.0
                            && rect.contains(pointer.unwrap_or_default());
                        let fill = if Some(i) == dragging_idx {
                            egui::Color32::from_rgb(255, 200, 80)
                        } else if hovered {
                            egui::Color32::from_rgb(255, 130, 100)
                        } else {
                            egui::Color32::from_rgb(220, 80, 70)
                        };
                        painter.circle_filled(p, marker_radius, fill);
                        painter.circle_stroke(p, marker_radius, egui::Stroke::new(1.5, egui::Color32::WHITE));
                        painter.text(
                            p,
                            egui::Align2::CENTER_CENTER,
                            format!("{}", i + 1),
                            egui::FontId::proportional(11.0),
                            egui::Color32::WHITE,
                        );
                        if hovered && resp.secondary_clicked() {
                            to_remove = Some(i);
                        }
                        if hovered && resp.drag_started_by(egui::PointerButton::Primary) {
                            new_drag = Some(i);
                        }
                    }

                    // Update drag.
                    if let Some(idx) = dragging_idx {
                        if let Some(pos) = pointer {
                            if rect.contains(pos) {
                                let world = to_world(pos);
                                if let Some(slot) = app.map_settings_mut().start_positions.get_mut(idx) {
                                    *slot = world;
                                }
                            }
                        }
                        if !resp.dragged_by(egui::PointerButton::Primary) {
                            new_drag = Some(usize::MAX);
                        }
                    }

                    // Click empty area to add a new spawn.
                    if resp.clicked()
                        && dragging_idx.is_none()
                        && pointer.map(|p| rect.contains(p)).unwrap_or(false)
                    {
                        let pos = pointer.unwrap();
                        let on_marker = spawns
                            .iter()
                            .any(|[x, z]| pos.distance(to_screen(*x, *z)) <= marker_radius + 2.0);
                        if !on_marker {
                            spawn_to_add = Some(to_world(pos));
                        }
                    }
                }
                InspectorMode::Sculpt => {
                    // Brush footprint as a hover ring.
                    if let Some(p) = pointer {
                        if rect.contains(p) {
                            hover_pos_in_rect = Some(p);
                            let scale = rect.width() / map_w.max(1) as f32;
                            let r_screen = (app.paint().brush.radius_px * scale).max(2.0);
                            let stroking = ctx.input(|i| i.pointer.button_down(egui::PointerButton::Primary));
                            let stroke_color = if stroking {
                                egui::Color32::from_rgb(255, 220, 120)
                            } else {
                                egui::Color32::from_rgb(255, 255, 255)
                            };
                            painter.circle_stroke(p, r_screen, egui::Stroke::new(1.5, stroke_color));
                            painter.circle_stroke(
                                p,
                                r_screen * 0.5,
                                egui::Stroke::new(0.7, stroke_color.gamma_multiply(0.6)),
                            );
                        }
                    }

                    let primary_down = ctx.input(|i| i.pointer.button_down(egui::PointerButton::Primary));
                    let inside = pointer.map(|p| rect.contains(p)).unwrap_or(false);
                    if primary_down && inside {
                        if !app.paint().brush_stroking {
                            stroke_just_started = true;
                        }
                        if let Some(p) = pointer {
                            sculpt_dabs.push(p);
                        }
                    } else if app.paint().brush_stroking && !primary_down {
                        stroke_just_ended = true;
                    }
                }
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| match app.paint().inspector_mode {
                InspectorMode::Spawns => {
                    let n = app.map_settings_mut().start_positions.len();
                    ui.label(format!("{n} spawn(s)"));
                    if ui.button("Clear all").clicked() {
                        app.map_settings_mut().start_positions.clear();
                        app.mark_dirty();
                    }
                }
                InspectorMode::Sculpt => {
                    let h_label = app
                        .paint()
                        .heightmap
                        .as_ref()
                        .map(|hm| format!("{}x{} px", hm.width(), hm.height()))
                        .unwrap_or_else(|| "(no preview)".to_string());
                    ui.weak(format!("Heightmap: {h_label}"));
                    if ui.button("Reset to graph output").clicked() {
                        let p = app.paint_mut();
                        p.heightmap = None;
                        p.texture = None;
                        p.texture_rev = p.heightmap_rev;
                    }
                    let can_save = app.paint().heightmap.is_some();
                    ui.add_enabled_ui(can_save, |ui| {
                        if ui.button("Save heightmap as PNG...").clicked() {
                            clicked_save_png = true;
                        }
                    });
                }
            });
        });

    app.dialog.show_inspector = open;
    if let Some(idx) = to_remove {
        app.map_settings_mut().start_positions.remove(idx);
        if app.dragging_spawn() == Some(idx) {
            app.set_dragging_spawn(None);
        }
        app.mark_dirty();
    }
    match new_drag {
        Some(usize::MAX) => app.set_dragging_spawn(None),
        Some(i) => app.set_dragging_spawn(Some(i)),
        None => {}
    }
    if app.dragging_spawn().is_some() && !ctx.input(|i| i.pointer.primary_down()) {
        app.set_dragging_spawn(None);
    }
    if app.dragging_spawn().is_some() {
        app.mark_dirty();
    }
    if let Some(p) = spawn_to_add {
        app.map_settings_mut().start_positions.push(p);
        app.mark_dirty();
    }

    // Apply queued sculpt dabs.
    if let (Some(rect), true) = (last_rect, !sculpt_dabs.is_empty()) {
        // Capture the Flatten target on stroke start.
        if stroke_just_started && app.paint().brush.tool == BrushTool::Flatten {
            let target = {
                let p_ref = app.paint();
                if let (Some(hm), Some(p)) = (
                    p_ref.heightmap.as_ref(),
                    hover_pos_in_rect.or_else(|| sculpt_dabs.first().copied()),
                ) {
                    let u = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                    let v = ((p.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
                    let hx = (u * hm.width() as f32) as u32;
                    let hy = (v * hm.height() as f32) as u32;
                    hm.get(hx, hy)
                } else {
                    None
                }
            };
            app.paint_mut().brush.flatten_target = target;
        }
        if stroke_just_started {
            app.paint_mut().brush_stroking = true;
        }
        // Apply each queued dab to the inspector heightmap.
        let p = app.paint_mut();
        if let Some(hm) = p.heightmap.as_mut() {
            for sp in &sculpt_dabs {
                let u = ((sp.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                let v = ((sp.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
                let hx = u * hm.width() as f32;
                let hy = v * hm.height() as f32;
                apply_brush_dab(hm, hx, hy, &p.brush);
            }
            p.heightmap_rev = p.heightmap_rev.wrapping_add(1);
        }
        app.mark_dirty();
    }
    if stroke_just_ended {
        let p = app.paint_mut();
        p.brush_stroking = false;
        p.brush.flatten_target = None;
    }

    // "Save heightmap as PNG" button — handled outside the window
    // closure so the rfd modal doesn't interleave with the egui
    // borrow on `app`.
    if clicked_save_png {
        if let Some(path) = app
            .make_dialog()
            .set_title("Save sculpted heightmap")
            .add_filter("16-bit grayscale PNG", &["png"])
            .set_file_name("sculpted-heightmap.png")
            .save_file()
        {
            // Clone the heightmap so we don't hold a borrow on
            // `app` across the file write.
            let hm = app.paint().heightmap.clone();
            if let Some(hm) = hm {
                match save_heightmap_as_png16(&hm, &path) {
                    Ok(()) => {
                        app.set_status_message(format!(
                            "Sculpt saved to {}. Add a FileInput node pointing at it to bake into export.",
                            path.display(),
                        ));
                    }
                    Err(e) => {
                        app.set_status_message(format!("Save failed: {e}"));
                    }
                }
            }
        }
    }
}

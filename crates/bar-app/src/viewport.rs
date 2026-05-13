//! Viewport rendering types and functions.
//!
//! `ViewportCore` and `EvalState` are the per-slot state types owned by the
//! layout manager. The free functions here operate on those types and on the
//! egui `Ui` passed by the layout manager each frame.

use std::sync::mpsc;
use std::time::Instant;

use bar_compute::GpuContext;
use bar_engine::recipe::PlacedFeature;
use bar_graph::NodeExecutor;
use bar_render::{pick_terrain, Camera, FeatureInstance, TerrainRenderer, TerrainUpdateParams};
use eframe::egui;

// ── Result type returned by background eval threads ──────────────────────────

pub struct PreviewResult {
    pub heightmap: Option<bar_data::Heightmap>,
    pub texture: Option<bar_data::ColorBuffer>,
    pub cache_key: u64,
    pub session_id: u64,
    pub height_scale: f32,
    pub water_y: f32,
    pub water_color: [f32; 3],
    pub smf_lighting: bar_render::SmfLighting,
    pub is_low_res: bool,
    pub x_extent: f32,
    pub z_extent: f32,
    /// Texture resolution this result was evaluated at.
    pub tex_w: u32,
    pub tex_h: u32,
}

// ── Owned frame (renderer input, survives across frames) ─────────────────────

#[derive(Clone)]
pub struct OwnedFrame {
    pub height_scale: f32,
    pub x_extent: f32,
    pub z_extent: f32,
    pub water_y: f32,
    pub water_color: [f32; 3],
    pub quality_high: bool,
    pub smf_lighting: bar_render::SmfLighting,
    /// Texture resolution this frame was evaluated at.
    pub tex_w: u32,
    pub tex_h: u32,
}

impl OwnedFrame {
    pub fn as_frame(&self, time: f32) -> bar_render::PreviewFrame {
        bar_render::PreviewFrame {
            height_scale: self.height_scale,
            x_extent: self.x_extent,
            z_extent: self.z_extent,
            water_y: self.water_y,
            water_color: self.water_color,
            quality_high: self.quality_high,
            time,
            smf_lighting: self.smf_lighting,
        }
    }
}

// ── Per-slot state types ──────────────────────────────────────────────────────

/// Rendering state shared by Sculpt3D and Preview slots.
pub struct ViewportCore {
    pub camera: Camera,
    pub terrain_renderer: Option<TerrainRenderer>,
    pub viewport_texture_id: Option<egui::TextureId>,
    pub current_frame: Option<OwnedFrame>,
    pub last_water_y: f32,
    pub last_water_color: [f32; 3],
    pub session_id: u64,
    pub started_at: Instant,
}

impl ViewportCore {
    pub fn new(gpu_context: &Option<GpuContext>, session_id: u64) -> Self {
        let terrain_renderer = gpu_context.as_ref().map(|ctx| {
            let mut r =
                TerrainRenderer::new(&ctx.device, &ctx.queue, wgpu::TextureFormat::Rgba8UnormSrgb);
            r.resize(&ctx.device, 512, 512);
            r
        });
        Self {
            camera: Camera::default(),
            terrain_renderer,
            viewport_texture_id: None,
            current_frame: None,
            last_water_y: -1.0,
            last_water_color: [0.0, 0.4, 0.6],
            session_id,
            started_at: Instant::now(),
        }
    }
}

/// Progressive eval scheduling state for the Sculpt3D slot.
pub struct EvalState {
    pub preview_tx: mpsc::Sender<PreviewResult>,
    pub preview_rx: mpsc::Receiver<PreviewResult>,
    pub last_low_res_key: u64,
    pub last_high_res_key: u64,
    pub low_res_pending: bool,
    pub high_res_pending: bool,
    pub low_res_completed_at: Option<Instant>,
    pub force_refresh_requested: bool,
    pub features_dirty: bool,
    /// Texture dims of the fast-preview pass for this slot's map dims.
    pub low_tex_dims: Option<(u32, u32)>,
    /// Texture dims of the full-quality pass for this slot's map dims.
    pub high_tex_dims: Option<(u32, u32)>,
}

impl EvalState {
    pub fn new() -> Self {
        let (preview_tx, preview_rx) = mpsc::channel();
        Self {
            preview_tx,
            preview_rx,
            last_low_res_key: u64::MAX,
            last_high_res_key: u64::MAX,
            low_res_pending: false,
            high_res_pending: false,
            low_res_completed_at: None,
            force_refresh_requested: false,
            features_dirty: true,
            low_tex_dims: None,
            high_tex_dims: None,
        }
    }
}

/// Resolution info for the viewport status overlay.
pub struct ResolutionStatus {
    /// Texture dims of the frame currently displayed (None = no frame yet).
    pub current_tex_dims: Option<(u32, u32)>,
    /// Configured fast-preview dims for this slot.
    pub low_tex_dims: Option<(u32, u32)>,
    /// Configured full-quality dims for this slot.
    pub high_tex_dims: Option<(u32, u32)>,
    pub low_pending: bool,
    pub high_pending: bool,
}

// ── Helper: register / update the egui texture handle ────────────────────────

pub fn update_viewport_texture(
    viewport_texture_id: &mut Option<egui::TextureId>,
    terrain_renderer: &Option<TerrainRenderer>,
    render_state: &Option<eframe::egui_wgpu::RenderState>,
    ctx: &egui::Context,
) {
    let Some(ref renderer) = terrain_renderer else {
        return;
    };
    let Some(view) = renderer.output_view() else {
        return;
    };
    let Some(ref rs) = render_state else {
        return;
    };

    let mut egui_rend = rs.renderer.write();
    if let Some(tex_id) = *viewport_texture_id {
        egui_rend.update_egui_texture_from_wgpu_texture(
            &rs.device,
            view,
            wgpu::FilterMode::Linear,
            tex_id,
        );
    } else {
        let tex_id = egui_rend.register_native_texture(&rs.device, view, wgpu::FilterMode::Linear);
        *viewport_texture_id = Some(tex_id);
    }
    ctx.request_repaint();
}

// ── Sculpt3D viewport drawing ─────────────────────────────────────────────────

pub fn draw_sculpt_viewport(
    core: &mut ViewportCore,
    res: &ResolutionStatus,
    gpu_context: &Option<GpuContext>,
    render_state: &Option<eframe::egui_wgpu::RenderState>,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    app: &mut bar_gui::BarEditorApp,
) {
    ui.small(bar_gui::i18n::t("editor.viewport_3d.sculpt_controls_hint"));
    ui.separator();
    let has_content = core.current_frame.is_some();
    draw_viewport_body(
        core,
        res,
        has_content,
        gpu_context,
        render_state,
        ui,
        ctx,
        app,
    );
}

// ── Preview layout viewport drawing ──────────────────────────────────────────

/// Placeholder shown when the Preview layout can't display a BC1 texture.
pub fn draw_preview_placeholder(
    ui: &mut egui::Ui,
    supports_bc: bool,
    is_compiled: bool,
    compile_running: bool,
) {
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            if !supports_bc {
                ui.heading("BC texture compression unavailable");
                ui.add_space(8.0);
                ui.label("Your GPU does not support BC1/DXT1 texture compression.");
                ui.label("The native-resolution Preview layout requires it.");
            } else if !is_compiled {
                ui.heading("Not yet compiled");
                ui.add_space(8.0);
                ui.label("Run Compile to generate the native-resolution texture.");
                ui.add_space(16.0);
                if compile_running {
                    ui.label("Compiling...");
                } else {
                    ui.label("Use the Compile button in the toolbar.");
                }
            }
        });
    });
}

pub fn draw_preview_viewport(
    core: &mut ViewportCore,
    res: &ResolutionStatus,
    gpu_context: &Option<GpuContext>,
    render_state: &Option<eframe::egui_wgpu::RenderState>,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    app: &mut bar_gui::BarEditorApp,
) {
    // Preview has no current_frame (no eval pass), but the BC1 texture is
    // meaningful content once loaded. Signal ready based on whether the BC1
    // texture dims are known (set by load_compiled_bc1 on success).
    let has_content = res.current_tex_dims.is_some();
    draw_viewport_body(
        core,
        res,
        has_content,
        gpu_context,
        render_state,
        ui,
        ctx,
        app,
    );
}

// ── Shared viewport body ──────────────────────────────────────────────────────

fn draw_viewport_body(
    core: &mut ViewportCore,
    res: &ResolutionStatus,
    has_content: bool,
    gpu_context: &Option<GpuContext>,
    render_state: &Option<eframe::egui_wgpu::RenderState>,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    app: &mut bar_gui::BarEditorApp,
) {
    let available = ui.available_size();
    let vp_w = (available.x as u32).max(1);
    let vp_h = (available.y as u32).max(1);

    if let Some(ref gpu) = gpu_context {
        if let Some(ref mut renderer) = core.terrain_renderer {
            if renderer.width != vp_w || renderer.height != vp_h {
                renderer.resize(&gpu.device, vp_w, vp_h);
                let elapsed = core.started_at.elapsed().as_secs_f32();
                let frame = core.current_frame.as_ref().map(|f| f.as_frame(elapsed));
                renderer.render(&gpu.device, &gpu.queue, &core.camera, frame.as_ref());
                update_viewport_texture(
                    &mut core.viewport_texture_id,
                    &core.terrain_renderer,
                    render_state,
                    ctx,
                );
            }
        }
    }

    if has_content && core.viewport_texture_id.is_some() {
        let tex_id = core.viewport_texture_id.unwrap();
        let image = egui::Image::new(egui::load::SizedTexture::new(tex_id, available))
            .fit_to_exact_size(available)
            .sense(egui::Sense::click_and_drag());
        let response = ui.add(image);

        draw_resolution_badge(ui, &response.rect, res);
        if res.low_pending || res.high_pending {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }

        handle_camera_input(core, gpu_context, render_state, &response, ctx, app);
    } else {
        // No frame yet -- show a loading message with the target resolution.
        let target = if res.low_pending {
            res.low_tex_dims
        } else if res.high_pending {
            res.high_tex_dims
        } else {
            None
        };
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.35);
                ui.add(
                    egui::Spinner::new()
                        .size(48.0)
                        .color(egui::Color32::from_rgba_unmultiplied(255, 200, 80, 220)),
                );
                if let Some((tw, th)) = target {
                    ui.add_space(10.0);
                    ui.colored_label(
                        egui::Color32::from_rgba_unmultiplied(255, 200, 80, 200),
                        format!("Loading {}...", fmt_tex_res(tw, th)),
                    );
                }
            });
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}

fn fmt_tex_res(w: u32, h: u32) -> String {
    if w == h {
        format!("{}px", w)
    } else {
        format!("{}x{}px", w, h)
    }
}

fn draw_resolution_badge(ui: &egui::Ui, viewport_rect: &egui::Rect, res: &ResolutionStatus) {
    let target_dims = if res.low_pending {
        res.low_tex_dims
    } else if res.high_pending {
        res.high_tex_dims
    } else {
        None
    };

    let label: String = match (res.current_tex_dims, target_dims) {
        (None, Some((tw, th))) => format!("-> {}", fmt_tex_res(tw, th)),
        (Some((cw, ch)), Some((tw, th))) if (cw, ch) != (tw, th) => {
            format!("{} -> {}", fmt_tex_res(cw, ch), fmt_tex_res(tw, th))
        }
        (Some((cw, ch)), _) => fmt_tex_res(cw, ch),
        (None, None) => return,
    };

    let painter = ui.painter();
    let font = egui::FontId::monospace(11.0);
    let text_color = if target_dims.is_some() {
        egui::Color32::from_rgba_unmultiplied(255, 200, 80, 230)
    } else {
        egui::Color32::from_rgba_unmultiplied(160, 200, 160, 200)
    };

    let galley = painter.layout_no_wrap(label, font, text_color);
    let padding = egui::vec2(6.0, 3.0);
    let badge_size = galley.size() + padding * 2.0;
    let badge_pos =
        viewport_rect.right_bottom() - egui::vec2(badge_size.x + 8.0, badge_size.y + 8.0);
    let badge_rect = egui::Rect::from_min_size(badge_pos, badge_size);

    painter.rect_filled(
        badge_rect,
        4.0,
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 160),
    );
    painter.galley(badge_pos + padding, galley, text_color);
}

// ── Camera and sculpt input ───────────────────────────────────────────────────

fn handle_camera_input(
    core: &mut ViewportCore,
    gpu_context: &Option<GpuContext>,
    render_state: &Option<eframe::egui_wgpu::RenderState>,
    response: &egui::Response,
    ctx: &egui::Context,
    app: &mut bar_gui::BarEditorApp,
) {
    let mut camera_changed = false;
    let sculpt_active = app.is_sculpt_input_active();

    let cursor_uv = response.hover_pos().map(|p| {
        let r = response.rect;
        (
            (p.x - r.left()) / r.width().max(1.0),
            (p.y - r.top()) / r.height().max(1.0),
        )
    });
    let aspect = response.rect.width().max(1.0) / response.rect.height().max(1.0);
    let cursor_world = if sculpt_active {
        cursor_uv.and_then(|uv| {
            let hm = app.paint.heightmap.as_ref()?;
            let renderer = core.terrain_renderer.as_ref()?;
            let (height_scale, x_extent, z_extent) = renderer.mesh_extents();
            let pick = pick_terrain(
                &core.camera,
                aspect,
                uv,
                hm,
                x_extent,
                z_extent,
                height_scale,
            )?;
            let world_per_px = (2.0 * x_extent) / hm.width().max(1) as f32;
            let radius_world = app.paint.brush.radius_px * world_per_px;
            Some((pick.world.x, pick.world.z, radius_world))
        })
    } else {
        None
    };
    if let Some(ref mut renderer) = core.terrain_renderer {
        renderer.set_brush_cursor(cursor_world);
    }

    if response.dragged_by(egui::PointerButton::Primary) {
        if sculpt_active {
            apply_sculpt_dab_at_cursor(core, gpu_context, response, ctx, app);
        } else {
            let delta = response.drag_delta();
            core.camera.orbit(delta.x * 0.01, delta.y * 0.01);
            camera_changed = true;
        }
    }
    if sculpt_active && response.drag_stopped_by(egui::PointerButton::Primary) {
        if let Some(node_id) = app.paint.selected_sculpt_layer {
            app.end_brush_stroke_on_layer(node_id);
        } else {
            app.end_brush_stroke();
        }
    }

    if response.dragged_by(egui::PointerButton::Secondary) {
        let delta = response.drag_delta();
        core.camera.orbit(delta.x * 0.01, delta.y * 0.01);
        camera_changed = true;
    }

    if cursor_world.is_some() || sculpt_active {
        camera_changed = true;
    }

    if response.dragged_by(egui::PointerButton::Middle) {
        let delta = response.drag_delta();
        let speed = core.camera.distance * 0.0015;
        core.camera.pan_xz(delta.x * speed, -delta.y * speed);
        camera_changed = true;
    }

    if response.hovered() {
        let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.1 {
            let factor = (-scroll * 0.0015).clamp(-0.5, 0.5);
            core.camera.zoom(factor);
            camera_changed = true;
        }
    }

    if camera_changed {
        if let (Some(ref mut renderer), Some(ref gpu)) = (&mut core.terrain_renderer, gpu_context) {
            let elapsed = core.started_at.elapsed().as_secs_f32();
            let frame = core.current_frame.as_ref().map(|f| f.as_frame(elapsed));
            renderer.render(&gpu.device, &gpu.queue, &core.camera, frame.as_ref());
            update_viewport_texture(
                &mut core.viewport_texture_id,
                &core.terrain_renderer,
                render_state,
                ctx,
            );
        }
    }
}

fn apply_sculpt_dab_at_cursor(
    core: &mut ViewportCore,
    gpu_context: &Option<GpuContext>,
    response: &egui::Response,
    ctx: &egui::Context,
    app: &mut bar_gui::BarEditorApp,
) {
    let Some(pointer) = ctx.pointer_latest_pos() else {
        return;
    };
    let rect = response.rect;
    if !rect.contains(pointer) {
        return;
    }
    let cursor_uv = (
        (pointer.x - rect.left()) / rect.width().max(1.0),
        (pointer.y - rect.top()) / rect.height().max(1.0),
    );
    let aspect = rect.width().max(1.0) / rect.height().max(1.0);
    let Some(hm) = app.paint.heightmap.as_ref() else {
        return;
    };
    let Some(renderer) = core.terrain_renderer.as_ref() else {
        return;
    };
    let (height_scale, x_extent, z_extent) = renderer.mesh_extents();
    let pick = pick_terrain(
        &core.camera,
        aspect,
        cursor_uv,
        hm,
        x_extent,
        z_extent,
        height_scale,
    );
    let Some(p) = pick else {
        return;
    };

    let stroke_starting = !response.dragged_by(egui::PointerButton::Primary)
        || response.drag_started_by(egui::PointerButton::Primary);

    let selected_node: Option<(bar_graph::NodeId, bar_graph::NodeType)> = app
        .paint
        .selected_sculpt_layer
        .and_then(|id| app.graph().get_node(id).map(|n| (id, n.node_type.clone())));

    let changed = if let Some((node_id, ref node_type)) = selected_node {
        let paintable = matches!(
            node_type,
            bar_graph::NodeType::PaintedHeightmap
                | bar_graph::NodeType::PaintedTexture
                | bar_graph::NodeType::Sculpt
        );
        if paintable {
            if matches!(node_type, bar_graph::NodeType::PaintedTexture) {
                app.apply_color_brush_to_sculpt_layer(node_id, p.hm_x, p.hm_y)
            } else {
                app.apply_brush_to_sculpt_layer(node_id, p.hm_x, p.hm_y, stroke_starting)
            }
        } else {
            false
        }
    } else {
        false
    };

    if !changed {
        return;
    }

    let is_color = selected_node
        .as_ref()
        .map(|(_, nt)| matches!(nt, bar_graph::NodeType::PaintedTexture))
        .unwrap_or(false);

    if is_color {
        if let (Some(ref gpu), Some(updated)) = (gpu_context, app.paint.color_buffer.clone()) {
            if let Some(ref mut renderer) = core.terrain_renderer {
                renderer.update_albedo(&gpu.device, &gpu.queue, &updated);
                let elapsed = core.started_at.elapsed().as_secs_f32();
                let frame = core.current_frame.as_ref().map(|f| f.as_frame(elapsed));
                renderer.render(&gpu.device, &gpu.queue, &core.camera, frame.as_ref());
            }
        }
    } else {
        if let (Some(ref gpu), Some(updated)) = (gpu_context, app.paint.heightmap.clone()) {
            if let Some(ref mut renderer) = core.terrain_renderer {
                let br = app.paint.brush.radius_px.ceil() as i32 + 1;
                let hm_w = updated.width() as i32;
                let hm_h = updated.height() as i32;
                let x0 = ((p.hm_x as i32) - br).max(0) as u32;
                let y0 = ((p.hm_y as i32) - br).max(0) as u32;
                let x1 = ((p.hm_x as i32) + br + 1).min(hm_w) as u32;
                let y1 = ((p.hm_y as i32) + br + 1).min(hm_h) as u32;
                let rw = x1 - x0;
                let rh = y1 - y0;
                if rw > 0 && rh > 0 {
                    let hm_ref = &updated;
                    let data: Vec<f32> = (y0..y1)
                        .flat_map(|y| (x0..x1).map(move |x| hm_ref.get(x, y).unwrap_or(0.0)))
                        .collect();
                    renderer.update_heightmap_region(&gpu.queue, x0, y0, rw, rh, &data);
                }
                let elapsed = core.started_at.elapsed().as_secs_f32();
                let frame = core.current_frame.as_ref().map(|f| f.as_frame(elapsed));
                renderer.render(&gpu.device, &gpu.queue, &core.camera, frame.as_ref());
            }
        }
    }
}

// ── Graph evaluation ──────────────────────────────────────────────────────────

pub fn eval_preview(
    graph: &bar_graph::GraphEngine,
    executor: &dyn NodeExecutor,
    hm_width: u32,
    hm_height: u32,
    tex_width: u32,
    tex_height: u32,
) -> (Option<bar_data::Heightmap>, Option<bar_data::ColorBuffer>) {
    let result = match bar_graph::evaluate_graph(
        graph, executor, hm_width, hm_height, tex_width, tex_height,
    ) {
        Ok(outputs) => outputs,
        Err(_) => return (None, None),
    };

    let hm = bar_graph::get_preview_heightmap(graph, &result);
    let tex = bar_graph::get_texture_output(graph, &result);
    (hm, tex)
}

// ── Feature instance building ─────────────────────────────────────────────────

pub fn build_feature_instances(
    features: &[PlacedFeature],
    w: u32,
    h: u32,
    min_h: f32,
    max_h: f32,
    catalog: Option<&bar_engine::FeatureCatalog>,
    heightmap: Option<&bar_data::Heightmap>,
) -> Vec<FeatureInstance> {
    use glam::{Mat4, Quat, Vec3};

    let pw = (w as f32 - 1.0).max(1.0);
    let ph = (h as f32 - 1.0).max(1.0);
    let pm = pw.max(ph);
    let xe = (0.5 * pw / pm).min(0.5);
    let ze = (0.5 * ph / pm).min(0.5);
    let height_range = (max_h - min_h).abs().max(1.0);
    let hs = (height_range / (pm * 8.0)).max(0.005);
    let default_footprint = 2.0_f32;

    features
        .iter()
        .map(|f| {
            let rx = (f.x / (pw * 8.0) - 0.5) * 2.0 * xe;
            let rz = (f.z / (ph * 8.0) - 0.5) * 2.0 * ze;

            let (fp_x, fp_z) = catalog
                .and_then(|cat| cat.features.get(&f.feature_type.to_lowercase()))
                .map(|def| (def.footprint_x.max(1) as f32, def.footprint_z.max(1) as f32))
                .unwrap_or((default_footprint, default_footprint));
            let sx = fp_x / pm;
            let sz = fp_z / pm;
            let sy = sx.max(sz);

            let h_render = if let Some(hm) = heightmap {
                let hx = (f.x / (pw * 8.0)).clamp(0.0, 1.0) * (hm.width().saturating_sub(1)) as f32;
                let hz =
                    (f.z / (ph * 8.0)).clamp(0.0, 1.0) * (hm.height().saturating_sub(1)) as f32;
                hm.get(hx as u32, hz as u32).unwrap_or(0.0) * hs
            } else {
                hs * 0.5
            };
            let ry = if f.y.abs() < 0.01 {
                h_render
            } else {
                ((f.y - min_h) / height_range) * hs
            };

            let transform = Mat4::from_scale_rotation_translation(
                Vec3::new(sx, sy, sz),
                Quat::from_rotation_y(-f.angle.to_radians()),
                Vec3::new(rx, ry, rz),
            );
            let cols = transform.to_cols_array_2d();
            let tint = match catalog {
                Some(cat) if cat.is_known(&f.feature_type) => [0.2, 0.9, 0.2, 1.0],
                _ => [1.0, 0.5, 0.0, 1.0],
            };
            FeatureInstance {
                col0: cols[0],
                col1: cols[1],
                col2: cols[2],
                col3: cols[3],
                tint,
            }
        })
        .collect()
}

// ── BC1 texture loading ───────────────────────────────────────────────────────

/// Load the compiled native-resolution BC1 texture into the viewport renderer.
/// Returns `true` on success.
/// Load the compiled BC1 texture into the Preview slot's terrain renderer.
/// Returns the native texture dimensions `(w, h)` on success, `None` on failure.
pub fn load_compiled_bc1(
    project_dir: Option<&std::path::Path>,
    recipe_name: &str,
    core: &mut ViewportCore,
    gpu: &GpuContext,
) -> Option<(u32, u32)> {
    let project_dir = project_dir?;
    let pkg = bar_engine::PackageDir::open(project_dir).ok()?;
    let fp = pkg.read_fingerprint()?;

    let tiles_x = if fp.tiles_x > 0 {
        fp.tiles_x
    } else {
        fp.map_x / 4
    };
    let tiles_y = if fp.tiles_y > 0 {
        fp.tiles_y
    } else {
        fp.map_y / 4
    };
    if tiles_x == 0 || tiles_y == 0 {
        return None;
    }
    let tex_w = tiles_x * 32;
    let tex_h = tiles_y * 32;

    let smt_path = pkg.compiled_smt_path(recipe_name);
    let idx_path = pkg.compiled_tile_index_path();

    let tile_pool = std::fs::File::open(&smt_path)
        .and_then(|mut f| bar_data::read_smt_raw(&mut f).map_err(std::io::Error::other))
        .map_err(|e| tracing::warn!(err = %e, "Preview BC1: failed to read compiled SMT"))
        .ok()?;
    let tile_indices: Vec<i32> = std::fs::read(&idx_path)
        .map_err(|e| tracing::warn!(err = %e, "Preview BC1: failed to read tile index"))
        .ok()?
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();

    let bc1 = bar_data::assemble_bc1_linear(&tile_pool, &tile_indices, tiles_x, tiles_y);
    let renderer = core.terrain_renderer.as_mut()?;
    renderer.upload_bc1_texture(&gpu.device, &gpu.queue, &bc1, tex_w, tex_h);
    tracing::info!(
        tiles_x,
        tiles_y,
        "Preview BC1: uploaded native-resolution texture"
    );

    // Upload the compiled heightmap as the terrain mesh so the BC1 texture
    // has geometry to project onto. Without this the renderer just outputs
    // its clear color.
    if let Some(hm) = bar_engine::read_compiled_heightmap(&pkg) {
        let (w, h) = (fp.map_x, fp.map_y);
        let pw = (w as f32).max(1.0);
        let ph = (h as f32).max(1.0);
        let pm = pw.max(ph);
        let x_extent = (0.5 * pw / pm).min(0.5);
        let z_extent = (0.5 * ph / pm).min(0.5);
        // Use the world-space height range from the recipe (recorded in the
        // fingerprint at compile time). The heightmap.bin values are
        // normalized [0,1], so we must not derive the scale from them.
        let min_h = fp.min_height;
        let max_h = fp.max_height;
        let height_range = (max_h - min_h).abs().max(1.0);
        let height_scale = (height_range / (pm * 8.0)).max(0.005);
        let water_y = if min_h < 0.0 {
            (-min_h / height_range) * height_scale
        } else {
            -1.0
        };
        let grid_n = hm.width().max(hm.height()).min(2048);
        renderer.update_heightmap(
            &gpu.device,
            &gpu.queue,
            &hm,
            TerrainUpdateParams {
                height_scale,
                x_extent,
                z_extent,
                water_y,
                water_color: [0.2, 0.45, 0.75],
                grid_n,
            },
        );
        core.current_frame = Some(OwnedFrame {
            height_scale,
            x_extent,
            z_extent,
            water_y,
            water_color: [0.2, 0.45, 0.75],
            quality_high: true,
            smf_lighting: bar_render::SmfLighting::default(),
            tex_w,
            tex_h,
        });
        tracing::info!(
            w = hm.width(),
            h = hm.height(),
            "Preview BC1: uploaded terrain mesh from compiled heightmap"
        );
    } else {
        tracing::warn!("Preview BC1: no compiled heightmap.bin -- terrain mesh not loaded");
    }

    Some((tex_w, tex_h))
}

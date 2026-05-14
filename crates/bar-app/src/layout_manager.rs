//! Layout manager -- owns per-layout rendering slots and drives the 3D
//! viewport each frame.
//!
//! One `RenderSlot` (Sculpt3D) and one `PreviewSlot` (Preview) are created
//! per project session. The NodeGraph layout has no 3D rendering state.
//! Both slots are replaced atomically when the project changes.
//!
//! The manager claims the egui central panel for Sculpt3D and Preview layouts;
//! the NodeGraph layout's central panel is claimed by bar-gui's canvas panel.

use std::sync::Arc;
use std::time::Instant;

use bar_compute::GpuContext;
use bar_graph::NodeExecutor;
use eframe::egui;

use crate::viewport::{
    build_feature_instances, draw_preview_placeholder, draw_preview_viewport, draw_sculpt_viewport,
    eval_preview, load_compiled_bc1, update_viewport_texture, EvalState, FeatureMapDims,
    OwnedFrame, PreviewResult, ResolutionStatus, ViewportCore,
};

// ── Slot types ────────────────────────────────────────────────────────────────

/// Sculpt3D slot: live eval + RGBA viewport.
pub struct RenderSlot {
    pub core: ViewportCore,
    pub eval: EvalState,
}

impl RenderSlot {
    fn new(gpu_context: &Option<GpuContext>, session_id: u64) -> Self {
        Self {
            core: ViewportCore::new(gpu_context, session_id),
            eval: EvalState::new(),
        }
    }
}

/// Preview slot: BC1 viewport, no eval scheduler.
pub struct PreviewSlot {
    pub core: ViewportCore,
    pub bc1_loaded: bool,
    /// Native texture dims of the loaded BC1 texture (set on successful load).
    pub bc1_tex_dims: Option<(u32, u32)>,
    /// Feature instances need rebuild.
    pub features_dirty: bool,
    /// Heightmap revision at last feature rebuild.
    pub last_hm_rev: u64,
}

impl PreviewSlot {
    fn new(gpu_context: &Option<GpuContext>, session_id: u64) -> Self {
        Self {
            core: ViewportCore::new(gpu_context, session_id),
            bc1_loaded: false,
            bc1_tex_dims: None,
            features_dirty: true,
            last_hm_rev: u64::MAX,
        }
    }
}

// ── Layout manager ────────────────────────────────────────────────────────────

pub struct LayoutManager {
    pub sculpt3d: Option<RenderSlot>,
    pub preview: Option<PreviewSlot>,
    next_session_id: u64,
}

impl LayoutManager {
    pub fn new() -> Self {
        Self {
            sculpt3d: None,
            preview: None,
            next_session_id: 0,
        }
    }

    /// Replace both slots with fresh state. Called when the project changes.
    pub fn reset(&mut self, gpu_context: &Option<GpuContext>) {
        let id = self.next_session_id;
        self.next_session_id += 2;
        self.sculpt3d = Some(RenderSlot::new(gpu_context, id));
        self.preview = Some(PreviewSlot::new(gpu_context, id + 1));
    }

    /// Per-frame update. Drives eval scheduling, animation, and viewport
    /// rendering for whichever layout is currently active.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        app: &mut bar_gui::BarEditorApp,
        layout: bar_gui::Layout,
        gpu_context: &Option<GpuContext>,
        render_state: &Option<eframe::egui_wgpu::RenderState>,
        executor: &Arc<dyn NodeExecutor + Send + Sync>,
        feature_catalog: Option<&bar_engine::FeatureCatalog>,
    ) {
        if app.project.take_graph_reset() {
            self.reset(gpu_context);
        }

        // Propagate placement changes to both slots before layout dispatch.
        if app.map.features_placement_dirty {
            app.map.features_placement_dirty = false;
            if let Some(ref mut s) = self.sculpt3d {
                s.eval.features_dirty = true;
            }
            if let Some(ref mut s) = self.preview {
                s.features_dirty = true;
            }
        }

        match layout {
            bar_gui::Layout::NodeGraph => {}
            bar_gui::Layout::Sculpt3D => {
                self.update_sculpt3d(
                    ctx,
                    app,
                    gpu_context,
                    render_state,
                    executor,
                    feature_catalog,
                );
            }
            bar_gui::Layout::Preview => {
                self.update_preview(ctx, app, gpu_context, render_state, feature_catalog);
            }
        }
    }

    // ── Sculpt3D ─────────────────────────────────────────────────────────────

    fn update_sculpt3d(
        &mut self,
        ctx: &egui::Context,
        app: &mut bar_gui::BarEditorApp,
        gpu_context: &Option<GpuContext>,
        render_state: &Option<eframe::egui_wgpu::RenderState>,
        executor: &Arc<dyn NodeExecutor + Send + Sync>,
        feature_catalog: Option<&bar_engine::FeatureCatalog>,
    ) {
        let Some(ref mut slot) = self.sculpt3d else {
            return;
        };

        // Force refresh: bump session_id so any in-flight result is rejected.
        if slot.eval.force_refresh_requested {
            slot.eval.force_refresh_requested = false;
            slot.core.session_id = self.next_session_id;
            self.next_session_id += 1;
            slot.eval.last_low_res_key = u64::MAX;
            slot.eval.last_high_res_key = u64::MAX;
            slot.eval.low_res_pending = false;
            slot.eval.high_res_pending = false;
            slot.eval.low_res_completed_at = None;
            slot.core.current_frame = None;
        }

        // Feature instances: rebuild when dirty or when a new heightmap arrives.
        // Skip until the first eval completes -- without a heightmap the fallback
        // height (range midpoint) causes visible floating on non-flat maps.
        let hm_rev = app.paint.heightmap_rev;
        let needs_feature_rebuild = slot.eval.features_dirty || slot.eval.last_hm_rev != hm_rev;
        if needs_feature_rebuild && app.paint.heightmap.is_some() {
            if let (Some(ref mut renderer), Some(ref gpu)) =
                (&mut slot.core.terrain_renderer, gpu_context)
            {
                let (w, h) = app.map.dimensions();
                let (min_h, max_h) = app.map.height_range();
                let loaded: std::collections::HashSet<String> = renderer
                    .feature_renderer_mut()
                    .map(|fr| fr.loaded_model_names().map(|s| s.to_string()).collect())
                    .unwrap_or_default();
                let (groups, unknowns) = build_feature_instances(
                    &app.map.features,
                    &FeatureMapDims { w, h, min_h, max_h },
                    feature_catalog,
                    app.paint.heightmap.as_ref(),
                    &loaded,
                    app.map.selected_feature_idx,
                );
                renderer.update_feature_instances(&gpu.device, &groups, &unknowns);
                slot.eval.features_dirty = false;
                slot.eval.last_hm_rev = hm_rev;
            }
        }

        // Poll completed eval results.
        let current_key = app.preview_cache_key();
        while let Ok(result) = slot.eval.preview_rx.try_recv() {
            if result.is_low_res {
                slot.eval.low_res_pending = false;
            } else {
                slot.eval.high_res_pending = false;
            }
            apply_preview_result(
                result,
                current_key,
                &mut slot.core,
                gpu_context,
                render_state,
                ctx,
                app,
                &mut slot.eval,
            );
        }

        // Spawn eval passes as needed.
        if !app.graph().nodes().is_empty() {
            spawn_eval_passes(slot, current_key, app, executor, ctx);
        }

        // Animation tick: re-render each frame so water + clouds animate.
        if let Some(ref gpu) = gpu_context {
            if let Some(ref mut renderer) = slot.core.terrain_renderer {
                if let Some(ref owned) = slot.core.current_frame {
                    let elapsed = slot.core.started_at.elapsed().as_secs_f32();
                    let frame = owned.as_frame(elapsed);
                    renderer.render(&gpu.device, &gpu.queue, &slot.core.camera, Some(&frame));
                    update_viewport_texture(
                        &mut slot.core.viewport_texture_id,
                        &slot.core.terrain_renderer,
                        render_state,
                        ctx,
                    );
                    ctx.request_repaint_after(std::time::Duration::from_millis(16));
                }
            }
        }

        // Snapshot resolution state before borrowing core mutably.
        let res = ResolutionStatus {
            current_tex_dims: slot.core.current_frame.as_ref().map(|f| (f.tex_w, f.tex_h)),
            low_tex_dims: slot.eval.low_tex_dims,
            high_tex_dims: slot.eval.high_tex_dims,
            low_pending: slot.eval.low_res_pending,
            high_pending: slot.eval.high_res_pending,
        };
        let core = &mut slot.core;
        egui::CentralPanel::default().show(ctx, |ui| {
            draw_sculpt_viewport(core, &res, gpu_context, render_state, ui, ctx, app);
        });
    }

    // ── Preview ───────────────────────────────────────────────────────────────

    fn update_preview(
        &mut self,
        ctx: &egui::Context,
        app: &mut bar_gui::BarEditorApp,
        gpu_context: &Option<GpuContext>,
        render_state: &Option<eframe::egui_wgpu::RenderState>,
        feature_catalog: Option<&bar_engine::FeatureCatalog>,
    ) {
        let Some(ref mut slot) = self.preview else {
            return;
        };

        // Feature instances: rebuild when dirty or heightmap changes.
        let hm_rev = app.paint.heightmap_rev;
        let needs_feature_rebuild = slot.features_dirty || slot.last_hm_rev != hm_rev;
        if needs_feature_rebuild && app.paint.heightmap.is_some() {
            if let (Some(ref mut renderer), Some(ref gpu)) =
                (&mut slot.core.terrain_renderer, gpu_context)
            {
                let (w, h) = app.map.dimensions();
                let (min_h, max_h) = app.map.height_range();
                let loaded: std::collections::HashSet<String> = renderer
                    .feature_renderer_mut()
                    .map(|fr| fr.loaded_model_names().map(|s| s.to_string()).collect())
                    .unwrap_or_default();
                let (groups, unknowns) = build_feature_instances(
                    &app.map.features,
                    &FeatureMapDims { w, h, min_h, max_h },
                    feature_catalog,
                    app.paint.heightmap.as_ref(),
                    &loaded,
                    app.map.selected_feature_idx,
                );
                renderer.update_feature_instances(&gpu.device, &groups, &unknowns);
                slot.features_dirty = false;
                slot.last_hm_rev = hm_rev;
            }
        }

        let is_compiled = app
            .project
            .path
            .as_deref()
            .map(|p| {
                bar_engine::PackageDir::open(p)
                    .map(|pkg| pkg.is_compiled())
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let can_show = app.supports_bc && is_compiled;

        if !can_show {
            egui::CentralPanel::default().show(ctx, |ui| {
                draw_preview_placeholder(
                    ui,
                    app.supports_bc,
                    is_compiled,
                    app.preview.compile_running,
                );
            });
            return;
        }

        // Derive the expected native texture dims from map settings.
        let (map_w, map_h) = app.map.dimensions();
        let native_tex_dims = if map_w > 1 && map_h > 1 {
            Some(((map_w - 1) * 8, (map_h - 1) * 8))
        } else {
            None
        };

        // Load BC1 texture once per compile (synchronous).
        if !slot.bc1_loaded {
            let project_dir = app.project.path.clone();
            let recipe_name = app.recipe_for_export().name;
            if let Some(ref gpu) = gpu_context {
                if let Some(dims) =
                    load_compiled_bc1(project_dir.as_deref(), &recipe_name, &mut slot.core, gpu)
                {
                    slot.bc1_tex_dims = Some(dims);
                    app.set_status("Preview: native-resolution BC1 texture loaded");
                }
            }
            slot.bc1_loaded = true;
        }

        // Animation + initial render. Drive the render even when there is no
        // current_frame (no heightmap yet) so viewport_texture_id gets set
        // as soon as the BC1 albedo is available.
        if let Some(ref gpu) = gpu_context {
            if let Some(ref mut renderer) = slot.core.terrain_renderer {
                let elapsed = slot.core.started_at.elapsed().as_secs_f32();
                let frame = slot
                    .core
                    .current_frame
                    .as_ref()
                    .map(|f| f.as_frame(elapsed));
                renderer.render(&gpu.device, &gpu.queue, &slot.core.camera, frame.as_ref());
                update_viewport_texture(
                    &mut slot.core.viewport_texture_id,
                    &slot.core.terrain_renderer,
                    render_state,
                    ctx,
                );
                if slot.core.current_frame.is_some() {
                    ctx.request_repaint_after(std::time::Duration::from_millis(16));
                }
            }
        }

        // Build resolution status: current dims come from the loaded BC1 texture;
        // pending dims come from the native resolution we expect to load.
        let res = ResolutionStatus {
            current_tex_dims: if slot.bc1_loaded {
                slot.bc1_tex_dims
            } else {
                None
            },
            low_tex_dims: None,
            high_tex_dims: native_tex_dims,
            low_pending: false,
            high_pending: !slot.bc1_loaded,
        };

        let core = &mut slot.core;
        egui::CentralPanel::default().show(ctx, |ui| {
            draw_preview_viewport(core, &res, gpu_context, render_state, ui, ctx, app);
        });
    }

    /// Mark feature instances as dirty on all slots (e.g. after catalog load or model arrival).
    pub fn mark_features_dirty(&mut self) {
        if let Some(ref mut slot) = self.sculpt3d {
            slot.eval.features_dirty = true;
        }
        if let Some(ref mut slot) = self.preview {
            slot.features_dirty = true;
        }
    }

    /// Upload an S3O model to both slots' feature renderers.
    pub fn load_feature_mesh(
        &mut self,
        device: &wgpu::Device,
        name: &str,
        mesh: &bar_data::S3oMesh,
    ) {
        if let Some(ref mut slot) = self.sculpt3d {
            if let Some(ref mut renderer) = slot.core.terrain_renderer {
                renderer.load_feature_mesh(device, name, mesh);
            }
        }
        if let Some(ref mut slot) = self.preview {
            if let Some(ref mut renderer) = slot.core.terrain_renderer {
                renderer.load_feature_mesh(device, name, mesh);
            }
        }
    }

    /// Invalidate the Preview slot's BC1 texture so it reloads on next entry.
    pub fn invalidate_bc1(&mut self) {
        if let Some(ref mut slot) = self.preview {
            slot.bc1_loaded = false;
        }
    }
}

// ── Eval helpers ──────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn apply_preview_result(
    result: PreviewResult,
    current_key: u64,
    core: &mut ViewportCore,
    gpu_context: &Option<GpuContext>,
    render_state: &Option<eframe::egui_wgpu::RenderState>,
    ctx: &egui::Context,
    app: &mut bar_gui::BarEditorApp,
    eval: &mut EvalState,
) {
    if result.session_id != core.session_id || result.cache_key != current_key {
        tracing::debug!(
            pass = if result.is_low_res { "low" } else { "high" },
            reason = if result.session_id != core.session_id {
                "session"
            } else {
                "key"
            },
            "Eval: stale result discarded"
        );
        return;
    }
    tracing::info!(
        pass = if result.is_low_res { "low" } else { "high" },
        has_hm = result.heightmap.is_some(),
        has_tex = result.texture.is_some(),
        "Eval: applying result to renderer"
    );

    let grid_n = if result.is_low_res {
        96
    } else {
        let hm_size = result
            .heightmap
            .as_ref()
            .map(|h| h.width().max(h.height()))
            .unwrap_or(1024);
        hm_size.min(2048)
    };

    if let Some(heightmap) = result.heightmap {
        // Always update app.paint.heightmap so features can sample terrain height
        // on the low-res pass. The high-res pass overwrites it with full resolution.
        app.set_inspector_heightmap(heightmap.clone());
        if !result.is_low_res {
            if let Some(ref tex) = result.texture {
                app.paint.color_buffer = Some(tex.clone());
            }
        }
        core.last_water_y = result.water_y;
        core.last_water_color = result.water_color;
        core.current_frame = Some(OwnedFrame {
            height_scale: result.height_scale,
            x_extent: result.x_extent,
            z_extent: result.z_extent,
            water_y: result.water_y,
            water_color: result.water_color,
            quality_high: !result.is_low_res,
            smf_lighting: result.smf_lighting,
            tex_w: result.tex_w,
            tex_h: result.tex_h,
        });

        if let Some(ref gpu) = gpu_context {
            if let Some(ref mut renderer) = core.terrain_renderer {
                renderer.update_heightmap(
                    &gpu.device,
                    &gpu.queue,
                    &heightmap,
                    bar_render::TerrainUpdateParams {
                        height_scale: result.height_scale,
                        x_extent: result.x_extent,
                        z_extent: result.z_extent,
                        water_y: result.water_y,
                        water_color: result.water_color,
                        grid_n,
                    },
                );
                if let Some(ref tex) = result.texture {
                    renderer.update_albedo(&gpu.device, &gpu.queue, tex);
                } else {
                    renderer.clear_albedo(&gpu.device, &gpu.queue);
                }
            }
        }

        if core.current_frame.is_some() {
            if let Some(ref gpu) = gpu_context {
                if let Some(ref mut renderer) = core.terrain_renderer {
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
    } else if !result.is_low_res {
        core.current_frame = None;
    }

    if result.is_low_res {
        eval.last_low_res_key = result.cache_key;
        eval.low_res_completed_at = Some(Instant::now());
    } else {
        eval.last_high_res_key = result.cache_key;
    }
}

fn spawn_eval_passes(
    slot: &mut RenderSlot,
    current_key: u64,
    app: &mut bar_gui::BarEditorApp,
    executor: &Arc<dyn NodeExecutor + Send + Sync>,
    ctx: &egui::Context,
) {
    let (w, h) = app.map.dimensions();
    let (min_h, max_h) = app.map.height_range();

    let pw = (w as f32 - 1.0).max(1.0);
    let ph = (h as f32 - 1.0).max(1.0);
    let pm = pw.max(ph);
    let xe = (0.5 * pw / pm).min(0.5);
    let ze = (0.5 * ph / pm).min(0.5);
    let height_range = (max_h - min_h).abs().max(1.0);
    let hs = (height_range / (pm * 8.0)).max(0.005);
    let wy = if min_h < 0.0 {
        (-min_h / height_range) * hs
    } else {
        -1.0
    };
    let water_color = [0.2_f32, 0.45, 0.75];
    let smf = app.smf_lighting();
    let smf_lighting = bar_render::SmfLighting {
        sun_dir: smf.sun_dir,
        ground_ambient: smf.ground_ambient,
        ground_diffuse: smf.ground_diffuse,
        ground_specular: smf.ground_specular,
        specular_exponent: smf.specular_exponent,
        water_absorb: smf.water_absorb,
        water_base: smf.water_base,
        water_min: smf.water_min,
    };
    let height_scale = hs;
    let water_y = wy;
    let x_extent = xe;
    let z_extent = ze;
    let session_id = slot.core.session_id;

    const TEXTURE_WORKING_RES_CAP: u32 = 4096;
    let tex_w = ((w - 1) * 8).clamp(1, TEXTURE_WORKING_RES_CAP);
    let tex_h = ((h - 1) * 8).clamp(1, TEXTURE_WORKING_RES_CAP);
    const LOW_RES_MIN: u32 = 512;
    let low_tex_w = (tex_w / 4).max(LOW_RES_MIN).min(tex_w);
    let low_tex_h = (tex_h / 4).max(LOW_RES_MIN).min(tex_h);
    let low_hm_scale = low_tex_w as f32 / tex_w as f32;
    let low_hm_w = ((w as f32 * low_hm_scale).round() as u32).max(1);
    let low_hm_h = ((h as f32 * low_hm_scale).round() as u32).max(1);

    // Always record configured dims so the overlay can display them.
    slot.eval.low_tex_dims = Some((low_tex_w, low_tex_h));
    slot.eval.high_tex_dims = Some((tex_w, tex_h));

    let needs_low_res =
        current_key != slot.eval.last_low_res_key && current_key != slot.eval.last_high_res_key;

    if needs_low_res && !slot.eval.low_res_pending {
        tracing::info!(
            hm = format!("{low_hm_w}x{low_hm_h}"),
            tex = format!("{low_tex_w}x{low_tex_h}"),
            "Eval: spawning low-res pass"
        );
        let graph = app.graph().clone();
        let tx = slot.eval.preview_tx.clone();
        let ctx_clone = ctx.clone();
        let exec = Arc::clone(executor);
        slot.eval.low_res_pending = true;
        std::thread::spawn(move || {
            let t0 = std::time::Instant::now();
            let (heightmap, texture) = eval_preview(
                &graph,
                exec.as_ref(),
                low_hm_w,
                low_hm_h,
                low_tex_w,
                low_tex_h,
            );
            let ms = t0.elapsed().as_millis();
            tracing::info!(ms, "Eval: low-res pass complete");
            let _ = tx.send(PreviewResult {
                heightmap,
                texture,
                cache_key: current_key,
                session_id,
                height_scale,
                water_y,
                water_color,
                smf_lighting,
                is_low_res: true,
                x_extent,
                z_extent,
                tex_w: low_tex_w,
                tex_h: low_tex_h,
            });
            ctx_clone.request_repaint();
        });
    }

    let needs_high_res = current_key != slot.eval.last_high_res_key;
    let cooldown_done = slot
        .eval
        .low_res_completed_at
        .map(|t| t.elapsed().as_millis() >= 300)
        .unwrap_or(false);

    if needs_high_res && !slot.eval.low_res_pending && !slot.eval.high_res_pending && cooldown_done
    {
        tracing::info!(
            hm = format!("{w}x{h}"),
            tex = format!("{tex_w}x{tex_h}"),
            "Eval: spawning high-res pass"
        );
        let graph = app.graph().clone();
        let tx = slot.eval.preview_tx.clone();
        let ctx_clone = ctx.clone();
        let exec = Arc::clone(executor);
        slot.eval.high_res_pending = true;
        std::thread::spawn(move || {
            let t0 = std::time::Instant::now();
            let (heightmap, texture) = eval_preview(&graph, exec.as_ref(), w, h, tex_w, tex_h);
            let ms = t0.elapsed().as_millis();
            tracing::info!(ms, "Eval: high-res pass complete");
            let _ = tx.send(PreviewResult {
                heightmap,
                texture,
                cache_key: current_key,
                session_id,
                height_scale,
                water_y,
                water_color,
                smf_lighting,
                is_low_res: false,
                x_extent,
                z_extent,
                tex_w,
                tex_h,
            });
            ctx_clone.request_repaint();
        });
    }
}

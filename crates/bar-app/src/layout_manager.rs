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
    apply_compiled_bc1, build_feature_instances, draw_preview_placeholder, draw_preview_viewport,
    draw_sculpt_viewport, eval_preview, live_smf_lighting, read_compiled_bc1_off_thread,
    update_viewport_texture, EvalState, FeatureMapDims, OwnedFrame, PreviewResult,
    ResolutionStatus, ViewportCore,
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
    /// Set while a background BC1 read is in flight. Gates spawning a
    /// second worker for the same load.
    pub bc1_loading: bool,
    /// Channel for the in-flight BC1 worker's result. `None` when no
    /// load is in flight.
    pub bc1_load_rx: Option<std::sync::mpsc::Receiver<Option<crate::viewport::Bc1LoadResult>>>,
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
            bc1_loading: false,
            bc1_load_rx: None,
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
    /// Last observed value of `app.map.selected_feature_idx`. Selection
    /// changes flip per-instance highlight colour in the feature
    /// instance buffer but don't change the type-set, so they only
    /// need an instance-buffer rebuild -- not a model reload.
    last_selected_feature: Option<usize>,
    /// Whether `app.placement_ghost` was `Some` last frame. Combined
    /// with this frame's state to detect ghost appear / disappear /
    /// continuing-visible -- all of which need an instance rebuild.
    last_ghost_visible: bool,
}

impl LayoutManager {
    pub fn new() -> Self {
        Self {
            sculpt3d: None,
            preview: None,
            next_session_id: 0,
            last_selected_feature: None,
            last_ghost_visible: false,
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
        // engine_dir: path to the active BAR engine version
        // (`<install>/data/engine/<ver>/`) used to source engine-shipped
        // water assets (foam + caustics). `None` when no BAR install
        // was detected; the renderer falls back to inert defaults.
        engine_dir: Option<&std::path::Path>,
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
        // Selection-only changes (no add/remove/type swap) still need
        // the instance buffer rebuilt so the highlight tint follows
        // the new selection -- but explicitly do not go through
        // `features_placement_dirty`, which kicks the model loader.
        if app.map.selected_feature_idx != self.last_selected_feature {
            self.last_selected_feature = app.map.selected_feature_idx;
            if let Some(ref mut s) = self.sculpt3d {
                s.eval.features_dirty = true;
            }
            if let Some(ref mut s) = self.preview {
                s.features_dirty = true;
            }
        }
        // Placement-ghost: rebuild every frame the ghost is visible so
        // the preview tracks the cursor, plus once more on the frame
        // it disappears (so the stale instance is removed from the
        // buffer). One extra rebuild per idle frame in placement mode
        // is cheap; not rebuilding produces visible cursor lag.
        let ghost_visible = app.placement_ghost.is_some();
        if ghost_visible || self.last_ghost_visible {
            self.last_ghost_visible = ghost_visible;
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
                    engine_dir,
                );
            }
            bar_gui::Layout::Preview => {
                self.update_preview(
                    ctx,
                    app,
                    gpu_context,
                    render_state,
                    feature_catalog,
                    engine_dir,
                );
            }
        }
    }

    // ── Sculpt3D ─────────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn update_sculpt3d(
        &mut self,
        ctx: &egui::Context,
        app: &mut bar_gui::BarEditorApp,
        gpu_context: &Option<GpuContext>,
        render_state: &Option<eframe::egui_wgpu::RenderState>,
        executor: &Arc<dyn NodeExecutor + Send + Sync>,
        feature_catalog: Option<&bar_engine::FeatureCatalog>,
        engine_dir: Option<&std::path::Path>,
    ) {
        let Some(ref mut slot) = self.sculpt3d else {
            return;
        };

        // Skybox + detail texture: independent of compile state and
        // graph eval -- keep the renderer in sync with the current
        // project so they show the moment a project opens, regardless
        // of which viewport the user is in or whether they've compiled.
        //
        // Resolution order: `project.path` (saved .barproj) first, then
        // `pending_map_data_dir` (the SD7 work dir, set by the runner
        // immediately after extraction). Without the fallback the
        // skybox couldn't render after a fresh import until the user
        // explicitly saved -- pending_map_data_dir is where the DDS
        // actually lives at that moment.
        if let Some(ref gpu) = gpu_context {
            // Upload any textures that completed background decoding
            // since the last frame.
            crate::viewport::poll_pending_texture_loads(&mut slot.core, gpu);
            let asset_dir = app
                .project
                .path
                .as_deref()
                .or(app.project.pending_map_data_dir.as_deref());
            crate::viewport::sync_skybox(
                asset_dir,
                &app.map_settings().atmosphere.skybox,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_detail_texture(
                asset_dir,
                &app.map_settings().resources.detail_tex,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_splat_textures(
                asset_dir,
                &app.map_settings().resources,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_sky_reflect_mod(
                asset_dir,
                &app.map_settings().resources.sky_reflect_mod_tex,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_specular_tex(
                asset_dir,
                &app.map_settings().resources.specular_tex,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_grass_shading_tex(
                asset_dir,
                &app.map_settings().resources.grass_shading_tex,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_light_emission_tex(
                asset_dir,
                &app.map_settings().resources.light_emission_tex,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_detail_normal_tex(
                asset_dir,
                &app.map_settings().resources.detail_normal_tex,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_basic_splat_tex(
                asset_dir,
                &app.map_settings().resources.splat_detail_tex,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_caustics(engine_dir, &mut slot.core, gpu);
        }

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
                    app.placement_ghost.as_ref(),
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

        // (Previously this branch cleared `current_frame` and the
        // albedo when the cache key moved past the last result we
        // showed -- the intent was to make graph edits "visibly take
        // effect" while the new eval ran. In practice the only thing
        // it accomplished was to blink the viewport into a spinner
        // for ~100ms on every undo / redo / param tweak.
        //
        // The eval still spawns below for any genuine key change, so
        // the new heightmap / texture lands in the renderer the
        // moment it's ready -- the user sees the previous frame
        // until then, which is a much less jarring transition than
        // a blank "Loading" state. If a future graph edit fails to
        // re-fire eval, that's a bug in the eval spawn path, not
        // something to mask by clearing the viewport here.)

        // Spawn eval passes as needed.
        if !app.graph().nodes().is_empty() {
            spawn_eval_passes(slot, current_key, app, executor, ctx);
        }

        // Animation tick: re-render each frame so water + clouds animate.
        if let Some(ref gpu) = gpu_context {
            if let Some(ref mut renderer) = slot.core.terrain_renderer {
                if let Some(ref owned) = slot.core.current_frame {
                    let elapsed = slot.core.started_at.elapsed().as_secs_f32();
                    let frame = owned.as_frame(elapsed, live_smf_lighting(app));
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
        engine_dir: Option<&std::path::Path>,
    ) {
        let Some(ref mut slot) = self.preview else {
            return;
        };

        // Skybox + detail texture sync up front -- decoupled from
        // BC1 compile state so they show even before / without compilation.
        // See `update_sculpt3d` for why we fall back to
        // `pending_map_data_dir` -- it's where assets live after a
        // fresh SD7 import but before the user has saved.
        if let Some(ref gpu) = gpu_context {
            crate::viewport::poll_pending_texture_loads(&mut slot.core, gpu);
            let asset_dir = app
                .project
                .path
                .as_deref()
                .or(app.project.pending_map_data_dir.as_deref());
            crate::viewport::sync_skybox(
                asset_dir,
                &app.map_settings().atmosphere.skybox,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_detail_texture(
                asset_dir,
                &app.map_settings().resources.detail_tex,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_splat_textures(
                asset_dir,
                &app.map_settings().resources,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_sky_reflect_mod(
                asset_dir,
                &app.map_settings().resources.sky_reflect_mod_tex,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_specular_tex(
                asset_dir,
                &app.map_settings().resources.specular_tex,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_grass_shading_tex(
                asset_dir,
                &app.map_settings().resources.grass_shading_tex,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_light_emission_tex(
                asset_dir,
                &app.map_settings().resources.light_emission_tex,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_detail_normal_tex(
                asset_dir,
                &app.map_settings().resources.detail_normal_tex,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_basic_splat_tex(
                asset_dir,
                &app.map_settings().resources.splat_detail_tex,
                &mut slot.core,
                gpu,
            );
            crate::viewport::sync_caustics(engine_dir, &mut slot.core, gpu);
        }

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
                    app.placement_ghost.as_ref(),
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

        // BC1 load: spawn a worker on first entry (or after recompile via
        // `invalidate_bc1`) that does the SMT/index/heightmap read on a
        // background thread. The main thread polls the receiver each
        // frame and performs the GPU uploads + heightmap install when
        // the worker's result arrives. Keeps the layout-switch frame
        // responsive even on large maps where the BC1 assembly + tile
        // pool read can take a noticeable amount of CPU time.
        if !slot.bc1_loaded && !slot.bc1_loading {
            if let Some(project_dir) = app.project.path.clone() {
                let recipe_name = app.recipe_for_export().name;
                let (tx, rx) = std::sync::mpsc::channel();
                let ctx_clone = ctx.clone();
                slot.bc1_loading = true;
                slot.bc1_load_rx = Some(rx);
                std::thread::spawn(move || {
                    let result = read_compiled_bc1_off_thread(&project_dir, &recipe_name);
                    let _ = tx.send(result);
                    ctx_clone.request_repaint();
                });
            } else {
                // No project path -- nothing to load. Mark loaded so we
                // don't keep retrying every frame.
                slot.bc1_loaded = true;
            }
        }
        if let Some(rx) = slot.bc1_load_rx.as_ref() {
            if let Ok(maybe_result) = rx.try_recv() {
                slot.bc1_load_rx = None;
                slot.bc1_loading = false;
                slot.bc1_loaded = true;
                if let (Some(result), Some(ref gpu)) = (maybe_result, gpu_context) {
                    let water_color = app.smf_lighting().water_base;
                    let loaded = apply_compiled_bc1(result, &mut slot.core, gpu, water_color);
                    slot.bc1_tex_dims = Some(loaded.tex_dims);
                    // Install the compiled heightmap into `app.paint.heightmap`
                    // if nothing has populated it yet (typical when the user
                    // jumps straight to Preview without visiting Sculpt3D
                    // first). Without this, terrain picking silently
                    // no-ops in Preview because it reads `paint.heightmap`.
                    // If a higher-res Sculpt3D eval already populated one,
                    // prefer it (Preview compiles at most 2048 grid_n;
                    // Sculpt3D eval goes to 8192).
                    if app.paint.heightmap.is_none() {
                        if let Some(hm) = loaded.heightmap {
                            app.set_inspector_heightmap(hm);
                            slot.features_dirty = true;
                        }
                    }
                    app.set_status("Preview: native-resolution BC1 texture loaded");
                }
            }
        }

        // Animation + initial render. Drive the render even when there is no
        // current_frame (no heightmap yet) so viewport_texture_id gets set
        // as soon as the BC1 albedo is available.
        if let Some(ref gpu) = gpu_context {
            if let Some(ref mut renderer) = slot.core.terrain_renderer {
                let elapsed = slot.core.started_at.elapsed().as_secs_f32();
                let smf = live_smf_lighting(app);
                let frame = slot
                    .core
                    .current_frame
                    .as_ref()
                    .map(|f| f.as_frame(elapsed, smf));
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

    /// True if both render slots already hold a loaded S3O mesh for
    /// the given feature type. Used by the runner to dedupe model
    /// loading when the features array changes shape but no genuinely
    /// new type was introduced (e.g. delete).
    pub fn has_feature_model(&self, feature_type: &str) -> bool {
        let in_sculpt = self
            .sculpt3d
            .as_ref()
            .and_then(|s| s.core.terrain_renderer.as_ref())
            .and_then(|r| r.feature_renderer())
            .map(|fr| fr.has_model(feature_type))
            .unwrap_or(false);
        let in_preview = self
            .preview
            .as_ref()
            .and_then(|s| s.core.terrain_renderer.as_ref())
            .and_then(|r| r.feature_renderer())
            .map(|fr| fr.has_model(feature_type))
            .unwrap_or(false);
        in_sculpt && in_preview
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

    /// Upload an S3O model (and its tex1 / tex2, if any) to both slots'
    /// feature renderers. When a texture is `None` the renderer substitutes a
    /// default white texture so the mesh still draws (tex1 fallback = white,
    /// tex2 fallback = white meaning fully opaque).
    pub fn load_feature_mesh(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        name: &str,
        mesh: &bar_data::S3oMesh,
        tex1: Option<&crate::runner::TextureRgba>,
        tex2: Option<&crate::runner::TextureRgba>,
    ) {
        let bar_tex1 = tex1.map(|t| bar_render::FeatureTexture {
            width: t.width,
            height: t.height,
            rgba: t.rgba.as_slice(),
        });
        let bar_tex2 = tex2.map(|t| bar_render::FeatureTexture {
            width: t.width,
            height: t.height,
            rgba: t.rgba.as_slice(),
        });
        if let Some(ref mut slot) = self.sculpt3d {
            if let Some(ref mut renderer) = slot.core.terrain_renderer {
                renderer.load_feature_mesh(
                    device,
                    queue,
                    name,
                    mesh,
                    bar_tex1.as_ref(),
                    bar_tex2.as_ref(),
                );
            }
        }
        if let Some(ref mut slot) = self.preview {
            if let Some(ref mut renderer) = slot.core.terrain_renderer {
                renderer.load_feature_mesh(
                    device,
                    queue,
                    name,
                    mesh,
                    bar_tex1.as_ref(),
                    bar_tex2.as_ref(),
                );
            }
        }
    }

    /// Invalidate the Preview slot's BC1 texture so it reloads on next entry.
    /// Also drops any in-flight load so a fresh worker is spawned for the
    /// new compile output.
    pub fn invalidate_bc1(&mut self) {
        if let Some(ref mut slot) = self.preview {
            slot.bc1_loaded = false;
            slot.bc1_loading = false;
            slot.bc1_load_rx = None;
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
    tracing::debug!(
        pass = if result.is_low_res { "low" } else { "high" },
        has_hm = result.heightmap.is_some(),
        has_tex = result.texture.is_some(),
        "Eval: applying result to renderer"
    );

    let grid_n = if result.is_low_res {
        96
    } else {
        // Engine renders SMF terrain at 1:1 mesh-to-heightmap density: each
        // heightmap sample is a vertex. Match that. Cap at 8192 (BAR ships
        // up to 64-block maps with 8193-sample native heightmap), aligned
        // with `MAX_HM_RES` in `bar_engine::extract`. The cap is a memory
        // budget, not a quality choice: 8192 mesh costs ~3.7GB of vertex
        // + index buffer at the maximum, but the buffer is allocated only
        // for the currently-loaded map and freed on map switch. Smaller
        // maps (<= 16-block, the vast majority of BAR maps) are
        // heightmap-bounded by `min(...)` and unaffected by the cap.
        // If a future map exceeds 64 blocks OR the worst-case memory
        // becomes untenable, the engine-faithful next step is chunked
        // terrain rendering (Recoil drives the patches via
        // CSMFGroundDrawer / SMFGroundTextures), not a smaller cap.
        let hm_size = result
            .heightmap
            .as_ref()
            .map(|h| h.width().max(h.height()))
            .unwrap_or(1024);
        hm_size.min(8192)
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
            height_range_elmos: result.height_range_elmos,
            elmo_per_render_xz: result.elmo_per_render_xz,
            x_extent: result.x_extent,
            z_extent: result.z_extent,
            water_y: result.water_y,
            water_color: result.water_color,
            quality_high: !result.is_low_res,
            tex_w: result.tex_w,
            tex_h: result.tex_h,
        });

        if let Some(ref gpu) = gpu_context {
            if let Some(ref mut renderer) = core.terrain_renderer {
                // Bake a coast-distance + invwaterdepth field from the
                // raw heightmap and push it as the renderer's coastmap.
                // Engine bakes its equivalent via a multi-pass shader;
                // we do a chamfer distance transform CPU-side. Cost
                // is O(N) over heightmap texels -- fast enough to run
                // synchronously on each heightmap update.
                let water_threshold = if result.height_scale > 1e-6 {
                    result.water_y / result.height_scale
                } else {
                    0.0
                };
                let coastmap = bar_data::bake_coastmap(
                    heightmap.data(),
                    heightmap.width(),
                    heightmap.height(),
                    water_threshold,
                );
                renderer.update_coastmap(
                    &gpu.device,
                    &gpu.queue,
                    &coastmap,
                    heightmap.width(),
                    heightmap.height(),
                );
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
                        height_range_elmos: result.height_range_elmos,
                        elmo_per_render_xz: result.elmo_per_render_xz,
                        // Sculpt3D edit view -- no surrounding mirror.
                        include_edge_extension: false,
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
                    let smf = live_smf_lighting(app);
                    let frame = core
                        .current_frame
                        .as_ref()
                        .map(|f| f.as_frame(elapsed, smf));
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
    let height_range_elmos = height_range;
    // Same XZ conversion as in viewport.rs::load_compiled_bc1 -- elmos
    // per unit of render-space, needed by the splat-detail shader.
    let elmo_per_render_xz = [pw * 4.0 / xe.max(1e-4), ph * 4.0 / ze.max(1e-4)];
    let wy = if min_h < 0.0 {
        (-min_h / height_range) * hs
    } else {
        -1.0
    };
    let smf = app.smf_lighting();
    // Refraction pre-pass clear colour: comes from the map's water.basecolor
    // when authored, otherwise the WaterSettings default. This drives only
    // the colour visible behind the refraction texture where no terrain is
    // drawn; surface tint and the underwater-absorption gradient have their
    // own per-map values (water.surfacecolor / water.absorb / water.mincolor)
    // plumbed through `WaterParamsUniform`.
    let water_color = smf.water_base;
    // `smf_lighting` no longer flows through PreviewResult / OwnedFrame --
    // it's read live from app.smf_lighting() at render time via
    // `live_smf_lighting(app)`, so mapinfo-editor edits to water / lighting
    // values take effect without waiting for the next graph evaluation.
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
        tracing::debug!(
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
            tracing::debug!(ms, "Eval: low-res pass complete");
            let _ = tx.send(PreviewResult {
                heightmap,
                texture,
                cache_key: current_key,
                session_id,
                height_scale,
                height_range_elmos,
                elmo_per_render_xz,
                water_y,
                water_color,
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
        tracing::debug!(
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
            tracing::debug!(ms, "Eval: high-res pass complete");
            let _ = tx.send(PreviewResult {
                heightmap,
                texture,
                cache_key: current_key,
                session_id,
                height_scale,
                height_range_elmos,
                elmo_per_render_xz,
                water_y,
                water_color,
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

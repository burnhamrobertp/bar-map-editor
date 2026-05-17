//! Application runner -- the `eframe::App` impl that wires the GUI, layout
//! manager, and all background jobs together.

use std::sync::{mpsc, Arc};

use tracing::Level;

use anyhow::Result;
use bar_compute::GpuContext;
use bar_engine::{CpuExecutor, FeatureCatalog, HybridExecutor};
use bar_graph::NodeExecutor;
use eframe::egui;

use crate::bar_install;
use crate::layout_manager::LayoutManager;

pub struct PendingExportDir {
    pub rx: mpsc::Receiver<Option<std::path::PathBuf>>,
    pub run_filter_label: Option<String>,
}

/// Result of the background S3O + texture loader. Both textures are optional
/// -- the renderer substitutes a default white texture for any that failed.
/// `tex1` (the S3O `texture1` channel) is the diffuse RGB plus team-color mask
/// in alpha. `tex2` (the S3O `texture2` channel) carries glow / specular in
/// RGB and the actual opacity in alpha (per
/// `cont/base/springcontent/shaders/GLSL/ModelFragProg.glsl`).
pub struct LoadedModel {
    pub name: String,
    pub mesh: bar_data::S3oMesh,
    pub tex1: Option<TextureRgba>,
    pub tex2: Option<TextureRgba>,
}

/// CPU-side RGBA8 texture ready for `Rgba8UnormSrgb` upload.
pub struct TextureRgba {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct AppRunner {
    pub app: bar_gui::BarEditorApp,
    pub executor: Arc<dyn NodeExecutor + Send + Sync>,
    pub gpu_context: Option<GpuContext>,
    pub render_state: Option<eframe::egui_wgpu::RenderState>,
    pub export_result_rx: Option<mpsc::Receiver<String>>,
    pub progress_rx: Option<mpsc::Receiver<String>>,
    pub export_status: bar_gui::ExportStatus,
    pub sd7_extract_rx: Option<mpsc::Receiver<Result<bar_engine::WorkDirScan, String>>>,
    /// Side channel paired with `sd7_extract_rx`. The worker thread
    /// sends short step labels (e.g. "Extracting archive") here at
    /// the start of each import phase; the GUI polls them per frame
    /// into `app.project.import_status` so the centered progress
    /// modal shows the current step.
    pub sd7_progress_rx: Option<mpsc::Receiver<String>>,
    pub compile_result_rx: Option<mpsc::Receiver<Result<(), String>>>,
    pub test_in_bar_rx: Option<mpsc::Receiver<Result<(std::path::PathBuf, String), String>>>,
    pub pending_export_dir: Option<PendingExportDir>,
    pub bar_install: Option<bar_install::BarVersions>,
    pub layout_manager: LayoutManager,
    pub pending_maximize: bool,
    pub has_shown_window: bool,
    pub feature_catalog: Option<FeatureCatalog>,
    pub catalog_rx: Option<mpsc::Receiver<FeatureCatalog>>,
    pub catalog_archive_path: Option<std::path::PathBuf>,
    /// Work directory from the last SD7 extraction; contains map-specific
    /// feature defs and S3O models not in the game archive.
    pub map_work_dir: Option<std::path::PathBuf>,
    /// Receives parsed S3O meshes from the background model-loader threads.
    /// Each item is `(lowercase_feature_type_name, mesh, optional_diffuse_rgba)`.
    /// The texture, when present, is `(width, height, rgba8 bytes)` ready for
    /// direct GPU upload as `Rgba8UnormSrgb`.
    pub model_rx: Option<mpsc::Receiver<LoadedModel>>,
    /// Receives forwarded tracing events from the AppLogLayer for display in the BME log.
    pub log_rx: mpsc::Receiver<(Level, String)>,
}

impl eframe::App for AppRunner {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Drain forwarded tracing events into the BME log buffer. The BME
        // log panel has per-level visibility toggles (INF / WRN / ERR / DBG)
        // so DEBUG events route to LogLevel::Debug rather than collapsing
        // into Info.
        while let Ok((level, msg)) = self.log_rx.try_recv() {
            let bme_level = match level {
                Level::ERROR => bar_gui::LogLevel::Error,
                Level::WARN => bar_gui::LogLevel::Warning,
                Level::DEBUG => bar_gui::LogLevel::Debug,
                _ => bar_gui::LogLevel::Info,
            };
            self.app.log_at(bme_level, msg);
        }

        if self.pending_maximize {
            self.pending_maximize = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }

        if !self.has_shown_window {
            self.has_shown_window = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }

        {
            use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
            let handles = match (frame.window_handle(), frame.display_handle()) {
                (Ok(w), Ok(d)) => Some((w.as_raw(), d.as_raw())),
                _ => None,
            };
            self.app.set_parent_window_handles(handles);
        }

        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && !self.app.take_allow_close() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }

        if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
            let maximized = ctx.input(|i| i.viewport().maximized).unwrap_or(false);
            self.app.update_window_state(
                outer.min.x,
                outer.min.y,
                outer.width(),
                outer.height(),
                maximized,
            );
        }

        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some(path) = dropped.into_iter().find(|p| {
            matches!(
                p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .as_deref(),
                Some("barproj") | Some("sd7")
            )
        }) {
            self.app.open_path_external(path);
        }

        // Drain per-node progress messages from the active export thread.
        if let Some(ref prx) = self.progress_rx {
            while let Ok(msg) = prx.try_recv() {
                self.app.set_status(msg);
            }
        }

        // Poll export result.
        if let Some(ref rx) = self.export_result_rx {
            if let Ok(msg) = rx.try_recv() {
                if let Some(ref prx) = self.progress_rx {
                    while let Ok(pmsg) = prx.try_recv() {
                        self.app.set_status(pmsg);
                    }
                }
                self.progress_rx = None;
                self.app.set_status(msg);
                self.export_result_rx = None;
                self.export_status = bar_gui::ExportStatus::Idle;
            }
        }

        // Poll compile result.
        if let Some(rx) = &self.compile_result_rx {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    self.app.project.compile_dirty = false;
                    self.app.project.compiled_at = Some(std::time::Instant::now());
                    self.app.preview.compile_running = false;
                    self.compile_result_rx = None;
                    self.app.set_status("Compile complete");
                    self.layout_manager.invalidate_bc1();
                }
                Ok(Err(e)) => {
                    self.app.preview.compile_running = false;
                    self.compile_result_rx = None;
                    self.app.set_status(format!("Compile failed: {e}"));
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.app.preview.compile_running = false;
                    self.compile_result_rx = None;
                }
            }
        }
        if self.app.preview.take_compile_requested() && self.compile_result_rx.is_none() {
            if let Some(project_dir) = self.app.project.path.clone() {
                let graph = self.app.graph().clone();
                let recipe = self.app.recipe_for_export();
                let executor = Arc::clone(&self.executor);
                let (tx, rx) = mpsc::channel::<Result<(), String>>();
                self.compile_result_rx = Some(rx);
                self.app.preview.compile_running = true;
                let ctx_clone = ctx.clone();
                std::thread::spawn(move || {
                    let result = bar_engine::compile_project(
                        &project_dir,
                        &graph,
                        executor.as_ref(),
                        &recipe,
                        &|_| {},
                    )
                    .map_err(|e| e.to_string());
                    let _ = tx.send(result);
                    ctx_clone.request_repaint();
                });
            } else {
                self.app
                    .set_status("Project must be saved before compiling");
            }
        }

        // Handle Run / export button.
        let run_all = self.app.preview.take_run_requested();
        let run_export_node = self.app.preview.take_run_export_node();
        let run_filter_label = run_export_node
            .and_then(|id| self.app.graph().get_node(id))
            .map(|n| n.label.clone());
        let no_export_in_flight =
            self.export_result_rx.is_none() && self.pending_export_dir.is_none();
        let should_request_dir = (run_all || run_export_node.is_some()) && no_export_in_flight;

        if should_request_dir {
            let (tx, rx) = mpsc::channel::<Option<std::path::PathBuf>>();
            let ctx_clone = ctx.clone();
            let parent = self.app.parent_window();
            std::thread::spawn(move || {
                let mut dialog = rfd::FileDialog::new().set_title("Choose export folder");
                if let Some(parent) = &parent {
                    dialog = dialog.set_parent(parent);
                }
                let dir = dialog.pick_folder();
                let _ = tx.send(dir);
                ctx_clone.request_repaint();
            });
            self.pending_export_dir = Some(PendingExportDir {
                rx,
                run_filter_label,
            });
            self.export_status = match run_export_node {
                Some(id) => bar_gui::ExportStatus::One(id),
                None => bar_gui::ExportStatus::All,
            };
        }

        if let Some(pending) = self.pending_export_dir.take() {
            match pending.rx.try_recv() {
                Ok(Some(output_dir)) => {
                    let graph = self.app.graph().clone();
                    let recipe = self.app.recipe_for_export();
                    let (w, h) = self.app.map.dimensions();
                    let (tx, rx) = mpsc::channel::<String>();
                    self.export_result_rx = Some(rx);
                    let (progress_tx, progress_rx) = mpsc::channel::<String>();
                    self.progress_rx = Some(progress_rx);
                    self.app.set_status(bar_gui::i18n::t_args(
                        "editor.export.generating",
                        &[("w", &w.to_string()), ("h", &h.to_string())],
                    ));
                    let ctx_clone = ctx.clone();
                    let ctx_progress = ctx.clone();
                    let executor = Arc::clone(&self.executor);
                    let run_filter_label = pending.run_filter_label;
                    let export_project_dir = self.app.project.path.clone();
                    std::thread::spawn(move || {
                        let progress_cb = |msg: &str| {
                            let _ = progress_tx.send(msg.to_string());
                            ctx_progress.request_repaint();
                        };
                        let map_x = w - 1;
                        let map_y = h - 1;
                        tracing::info!(
                            map_x,
                            map_y,
                            "Bundle: evaluating graph at native resolution"
                        );
                        let msg = match bar_graph::evaluate_graph_with_progress(
                            &graph,
                            executor.as_ref(),
                            w,
                            h,
                            map_x * 8,
                            map_y * 8,
                            &progress_cb,
                        ) {
                            Ok(outputs) => {
                                // Auto-recompile when compiled state is stale so the bundler
                                // can copy the compiled SMT directly rather than re-encoding.
                                if let Some(ref proj_dir) = export_project_dir {
                                    match bar_engine::PackageDir::open(proj_dir) {
                                        Ok(pkg) => {
                                            let recipe_json =
                                                std::fs::read_to_string(pkg.recipe_path())
                                                    .unwrap_or_default();
                                            let stale = pkg.is_stale(&recipe_json, map_x, map_y);
                                            tracing::debug!(
                                                stale,
                                                map_x,
                                                map_y,
                                                "Bundle: compiled state staleness check"
                                            );
                                            if stale {
                                                tracing::info!("Bundle: compiled output stale -- recompiling before packaging");
                                                progress_cb("Bundle: recompiling stale output");
                                                if let Err(e) = bar_engine::compile_from_outputs(
                                                    proj_dir,
                                                    &graph,
                                                    &outputs,
                                                    &recipe,
                                                    bar_engine::CompileDims {
                                                        map_x,
                                                        map_y,
                                                        tex_w: map_x * 8,
                                                        tex_h: map_y * 8,
                                                    },
                                                    &progress_cb,
                                                ) {
                                                    tracing::warn!(err = %e, "Bundle: auto-recompile failed -- bundler will re-encode texture");
                                                } else {
                                                    tracing::info!(
                                                        "Bundle: auto-recompile complete"
                                                    );
                                                }
                                            } else {
                                                tracing::info!("Bundle: compiled output is current -- skipping recompile");
                                            }
                                        }
                                        Err(e) => {
                                            tracing::debug!(err = %e, "Bundle: cannot open package dir -- skipping compiled SMT fast-path");
                                        }
                                    }
                                }
                                let filter = run_filter_label.as_deref();
                                tracing::info!("Bundle: packaging");
                                match bar_engine::execute_bundlers(
                                    &graph,
                                    &outputs,
                                    &recipe,
                                    &output_dir,
                                    filter,
                                    export_project_dir.as_deref(),
                                ) {
                                    Ok(results) if !results.is_empty() => {
                                        tracing::info!(
                                            count = results.len(),
                                            dest = %output_dir.display(),
                                            "Bundle: complete"
                                        );
                                        format!(
                                            "Exported {} bundle(s) to {}",
                                            results.len(),
                                            output_dir.display()
                                        )
                                    }
                                    Ok(_) => {
                                        "No Bundler nodes found -- add a Bundler node to export"
                                            .to_string()
                                    }
                                    Err(e) => format!("Export failed: {e}"),
                                }
                            }
                            Err(e) => format!("Graph evaluation failed: {e}"),
                        };
                        let _ = tx.send(msg);
                        ctx_clone.request_repaint();
                    });
                }
                Ok(None) => {
                    self.export_status = bar_gui::ExportStatus::Idle;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.pending_export_dir = Some(pending);
                    ctx.request_repaint();
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.export_status = bar_gui::ExportStatus::Idle;
                }
            }
        }

        // Test in BAR.
        if self.app.preview.take_test_in_bar() && self.test_in_bar_rx.is_none() {
            self.start_test_in_bar(ctx);
        }
        if let Some(ref rx) = self.test_in_bar_rx {
            if let Ok(result) = rx.try_recv() {
                self.test_in_bar_rx = None;
                self.export_status = bar_gui::ExportStatus::Idle;
                if let Some(ref prx) = self.progress_rx {
                    while let Ok(pmsg) = prx.try_recv() {
                        self.app.set_status(pmsg);
                    }
                }
                self.progress_rx = None;
                match result {
                    Ok((sd7_path, map_internal_name)) => {
                        self.finish_test_in_bar(&sd7_path, &map_internal_name)
                    }
                    Err(e) => self.app.set_status(format!("Test in BAR: {e}")),
                }
            }
        }

        self.app.preview.set_export_status(self.export_status);

        // Run the bar-gui frame (menus, palette, properties, modals, layout panels).
        self.app.update(ctx, frame);

        // Feature catalog: detect archive change, poll background load.
        let desired_archive = self.app.settings().selected_game_archive.clone();
        if desired_archive != self.catalog_archive_path {
            self.catalog_rx = None;
            self.model_rx = None;
            self.feature_catalog = None;
            self.catalog_archive_path = desired_archive.clone();
            if let Some(archive) = desired_archive {
                let (tx, rx) = mpsc::channel::<FeatureCatalog>();
                self.catalog_rx = Some(rx);
                let ctx_clone = ctx.clone();
                std::thread::spawn(move || {
                    let catalog = FeatureCatalog::from_archive(&archive);
                    let _ = tx.send(catalog);
                    ctx_clone.request_repaint();
                });
            }
        }
        if let Some(ref rx) = self.catalog_rx {
            if let Ok(mut catalog) = rx.try_recv() {
                self.catalog_rx = None;
                // Merge map-level defs that arrived before the game catalog.
                if let Some(ref work_dir) = self.map_work_dir {
                    let map_catalog = FeatureCatalog::from_dir(work_dir);
                    if !map_catalog.is_empty() {
                        catalog.merge(map_catalog);
                    }
                }
                let count = catalog.features.len();
                let mut names: Vec<String> = catalog.features.keys().cloned().collect();
                names.sort();
                self.app.feature_palette_names = names;
                self.app
                    .set_status(format!("Feature catalog: {count} definitions"));
                self.feature_catalog = Some(catalog);
                self.spawn_model_loader(ctx);
                self.layout_manager.mark_features_dirty();
            }
        }

        // Barproj open: features_changed is pulsed by apply_project; trigger model loading.
        if self.app.project.features_changed {
            self.app.project.features_changed = false;
            self.model_rx = None; // cancel prior loader for a different map
                                  // On barproj reload no SD7 extraction runs, so map_work_dir may be
                                  // None. Check whether the project dir has objects3d/ copied in
                                  // (happens on first save after import); if so, use it as the map
                                  // data root so S3O models and feature defs are found without any
                                  // reference to the original archive.
            if self.map_work_dir.is_none() {
                if let Some(ref project_dir) = self.app.project.path.clone() {
                    if project_dir.join("objects3d").is_dir() {
                        let map_catalog = FeatureCatalog::from_dir(project_dir);
                        if !map_catalog.is_empty() {
                            if let Some(ref mut catalog) = self.feature_catalog {
                                catalog.merge(map_catalog);
                                let mut names: Vec<String> =
                                    catalog.features.keys().cloned().collect();
                                names.sort();
                                self.app.feature_palette_names = names;
                            }
                        }
                        self.map_work_dir = Some(project_dir.clone());
                    }
                }
            }
            self.spawn_model_loader(ctx);
        }

        // Poll loaded S3O models; upload each one to the feature renderer.
        let mut any_model_loaded = false;
        let mut model_loader_done = false;
        if let Some(ref rx) = self.model_rx {
            loop {
                match rx.try_recv() {
                    Ok(loaded) => {
                        if let Some(ref gpu) = self.gpu_context {
                            self.layout_manager.load_feature_mesh(
                                &gpu.device,
                                &gpu.queue,
                                &loaded.name,
                                &loaded.mesh,
                                loaded.tex1.as_ref(),
                                loaded.tex2.as_ref(),
                            );
                            any_model_loaded = true;
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        model_loader_done = true;
                        break;
                    }
                }
            }
        }
        if model_loader_done {
            self.model_rx = None;
        }
        if any_model_loaded {
            self.layout_manager.mark_features_dirty();
        }

        // Drain the SD7 progress side channel into `import_status`
        // BEFORE polling the result channel below, so the user sees
        // intermediate steps land before the final modal-close
        // happens. Only the most recent message matters; older
        // messages are superseded.
        if let Some(ref rx) = self.sd7_progress_rx {
            while let Ok(step) = rx.try_recv() {
                self.app.project.import_status = Some(step);
            }
        }

        // Poll SD7 extraction.
        if let Some(ref rx) = self.sd7_extract_rx {
            match rx.try_recv() {
                Ok(Ok(scan)) => {
                    let work_dir = scan.work_dir.clone();
                    self.map_work_dir = Some(work_dir.clone());
                    self.app.project.pending_map_data_dir = Some(work_dir.clone());
                    // Merge map-specific feature defs into the game catalog while
                    // we still have both. If the game catalog isn't loaded yet, the
                    // merge happens in spawn_model_loader via map_work_dir.
                    if let Some(ref mut catalog) = self.feature_catalog {
                        let map_catalog = FeatureCatalog::from_dir(&work_dir);
                        if !map_catalog.is_empty() {
                            tracing::info!(
                                count = map_catalog.features.len(),
                                "Merged map-level feature definitions"
                            );
                            catalog.merge(map_catalog);
                        }
                        let mut names: Vec<String> = catalog.features.keys().cloned().collect();
                        names.sort();
                        self.app.feature_palette_names = names;
                    }
                    self.app.finish_open_map(scan);
                    self.app.project.features_changed = false; // handled here, not by the generic poll below
                    self.app.project.import_status = None;
                    self.sd7_extract_rx = None;
                    self.sd7_progress_rx = None;
                    self.model_rx = None;
                    self.spawn_model_loader(ctx);
                }
                Ok(Err(e)) => {
                    self.app.set_status(format!("Failed to open: {e}"));
                    self.app.project.import_status = None;
                    self.sd7_extract_rx = None;
                    self.sd7_progress_rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.app
                        .set_status("Open operation failed unexpectedly".to_string());
                    self.app.project.import_status = None;
                    self.sd7_extract_rx = None;
                    self.sd7_progress_rx = None;
                }
            }
        }
        if let Some(sd7_path) = self.app.project.sd7_open_request.take() {
            let (result_tx, result_rx) = mpsc::channel::<Result<bar_engine::WorkDirScan, String>>();
            let (progress_tx, progress_rx) = mpsc::channel::<String>();
            self.sd7_extract_rx = Some(result_rx);
            self.sd7_progress_rx = Some(progress_rx);
            // Seed an initial step so the modal renders immediately --
            // worker startup latency would otherwise leave a blank
            // modal for a frame or two.
            self.app.project.import_status = Some("Starting import".to_string());
            let ctx_clone = ctx.clone();
            std::thread::spawn(move || {
                let progress_tx_inner = progress_tx.clone();
                let ctx_for_progress = ctx_clone.clone();
                let progress = move |step: &str| {
                    let _ = progress_tx_inner.send(step.to_string());
                    // Wake the GUI loop so the modal updates without
                    // waiting on the next user-input frame.
                    ctx_for_progress.request_repaint();
                };
                let result =
                    bar_engine::extract_sd7_to_work_dir_with_progress(&sd7_path, &progress)
                        .map_err(|e| e.to_string());
                let _ = result_tx.send(result);
                ctx_clone.request_repaint();
            });
        }

        // If the user placed new features interactively, ensure their models are loaded.
        if self.app.map.features_placement_dirty {
            self.spawn_model_loader(ctx);
        }

        // Delegate layout rendering to the layout manager.
        let layout = self.app.active_layout();
        self.layout_manager.update(
            ctx,
            &mut self.app,
            layout,
            &self.gpu_context,
            &self.render_state,
            &self.executor,
            self.feature_catalog.as_ref(),
        );
    }
}

impl AppRunner {
    /// Spawn the background S3O model loader for all feature types in the current
    /// map that have a catalog entry with a non-empty object field.
    /// No-op if the catalog is not loaded, the archive path is unknown, or
    /// a loader is already running (model_rx is Some).
    fn spawn_model_loader(&mut self, ctx: &egui::Context) {
        if self.model_rx.is_some() {
            return;
        }
        let (catalog, archive) = match (&self.feature_catalog, &self.catalog_archive_path) {
            (Some(c), Some(a)) => (c, a),
            _ => return,
        };
        let unique_types: std::collections::HashSet<String> = self
            .app
            .map
            .features
            .iter()
            .map(|f| f.feature_type.to_lowercase())
            .collect();
        let to_load: Vec<(String, String)> = unique_types
            .iter()
            .filter(|name| !self.layout_manager.has_feature_model(name))
            .filter_map(|name| {
                let def = catalog.features.get(name)?;
                if def.object.is_empty() {
                    return None;
                }
                let obj = &def.object;
                let path = if obj.contains('.') {
                    format!("objects3d/{obj}")
                } else {
                    format!("objects3d/{obj}.s3o")
                };
                Some((name.clone(), path))
            })
            .collect();
        if to_load.is_empty() {
            // All required types already loaded (or absent from the
            // catalog); no work and nothing to say.
            return;
        }
        self.app
            .set_status(format!("Loading {} feature models...", to_load.len()));
        let (tx, rx) = mpsc::channel::<LoadedModel>();
        self.model_rx = Some(rx);
        let archive = archive.clone();
        let work_dir = self.map_work_dir.clone();
        // Sibling .sdz/.sd7 archives alongside a .sdd stub hold the real game content.
        let sibling_archives = bar_engine::sibling_zip_archives(&archive);
        // Cached SD7 work dirs from prior imports. Used as a fallback for
        // textures missing from the project_dir (e.g. .barproj saved before
        // the unittextures copy fix landed).
        let work_dir_cache = list_cached_work_dirs();
        // data_dir is two levels up from BAR.sdd (games/ -> data/) for pool reading.
        let pool_data_dir = archive
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf());
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let mut loaded = 0usize;
            for (name, path) in to_load {
                let asset_sources = AssetSources {
                    archive: &archive,
                    siblings: &sibling_archives,
                    work_dir: work_dir.as_deref(),
                    work_dir_cache: &work_dir_cache,
                    pool_data_dir: pool_data_dir.as_deref(),
                };
                let Some(mesh) = load_s3o_from_sources(&name, &path, &asset_sources) else {
                    tracing::warn!(
                        feature = %name,
                        path = %path,
                        "S3O not found or unparseable in any source"
                    );
                    continue;
                };
                let tex1 = if mesh.texture1.is_empty() {
                    None
                } else {
                    load_texture_from_sources(&name, "tex1", &mesh.texture1, &asset_sources)
                };
                let tex2 = if mesh.texture2.is_empty() {
                    None
                } else {
                    load_texture_from_sources(&name, "tex2", &mesh.texture2, &asset_sources)
                };
                loaded += 1;
                let msg = LoadedModel {
                    name: name.clone(),
                    mesh,
                    tex1,
                    tex2,
                };
                if tx.send(msg).is_err() {
                    break;
                }
                ctx_clone.request_repaint();
            }
            tracing::info!(loaded, "S3O model loading complete");
        });
    }

    fn start_test_in_bar(&mut self, ctx: &egui::Context) {
        if self.bar_install.is_none() {
            self.app.set_status(
                "BAR install not found. Install Beyond All Reason or set the path manually."
                    .to_string(),
            );
            return;
        }

        let temp_dir = std::env::temp_dir().join(format!(
            "om_test_in_bar_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
        ));
        if let Err(e) = std::fs::create_dir_all(&temp_dir) {
            self.app
                .set_status(format!("Test in BAR: cannot create temp dir: {e}"));
            return;
        }

        let graph = self.app.graph().clone();
        let recipe = self.app.recipe_for_export();
        let (w, h) = self.app.map.dimensions();
        let executor = Arc::clone(&self.executor);
        let (tx, rx) = mpsc::channel::<Result<(std::path::PathBuf, String), String>>();
        self.test_in_bar_rx = Some(rx);
        let (progress_tx, progress_rx) = mpsc::channel::<String>();
        self.progress_rx = Some(progress_rx);
        self.export_status = bar_gui::ExportStatus::All;
        self.app
            .set_status(format!("Generating {}x{} map...", w, h));
        let ctx_clone = ctx.clone();
        let ctx_progress = ctx.clone();
        let test_project_dir = self.app.project.path.clone();

        std::thread::spawn(move || {
            let progress_cb = |msg: &str| {
                let _ = progress_tx.send(msg.to_string());
                ctx_progress.request_repaint();
            };
            let result = match bar_graph::evaluate_graph_with_progress(
                &graph,
                executor.as_ref(),
                w,
                h,
                (w - 1) * 8,
                (h - 1) * 8,
                &progress_cb,
            ) {
                Ok(outputs) => match bar_engine::execute_bundlers(
                    &graph,
                    &outputs,
                    &recipe,
                    &temp_dir,
                    None,
                    test_project_dir.as_deref(),
                ) {
                    Ok(results) => results
                        .into_iter()
                        .find(|r| r.output_path.extension().and_then(|s| s.to_str()) == Some("sd7"))
                        .map(|r| Ok((r.output_path, r.map_internal_name)))
                        .unwrap_or_else(|| Err("Bundler produced no SD7".to_string())),
                    Err(e) => Err(format!("Bundler error: {e}")),
                },
                Err(e) => Err(format!("Graph evaluation failed: {e:?}")),
            };
            let _ = tx.send(result);
            ctx_clone.request_repaint();
        });
    }

    fn finish_test_in_bar(&mut self, sd7_path: &std::path::Path, map_internal_name: &str) {
        let Some(ref install) = self.bar_install else {
            self.app
                .set_status("BAR install vanished mid-flight".to_string());
            return;
        };
        let game_idx = self.app.bar_versions.selected_game;
        let engine_idx = self.app.bar_versions.selected_engine;
        match install.launch_skirmish(sd7_path, map_internal_name, game_idx, engine_idx) {
            Ok(bar_install::LaunchOutcome::EngineStarted { map_name }) => {
                self.app
                    .set_status(format!("BAR started: skirmish on {map_name}"));
            }
            Err(e) => self.app.set_status(format!("Test in BAR: {e}")),
        }
    }
}

// ── Asset loading helpers ─────────────────────────────────────────────────────

/// Bundle of asset sources searched when looking up a model or texture.
/// All paths use forward slashes (archive-internal convention); on-disk
/// lookups rewrite to the platform separator before reading.
struct AssetSources<'a> {
    archive: &'a std::path::Path,
    siblings: &'a [std::path::PathBuf],
    /// Primary "map data root" -- either the freshly extracted SD7 work-dir or,
    /// after a .barproj reload, the project directory (which has objects3d/ and
    /// features/ -- and, post-fix, unittextures/ -- copied in on first save).
    work_dir: Option<&'a std::path::Path>,
    /// Sibling cached SD7 work dirs (BarEditor cache). Searched after the
    /// primary work_dir so a .barproj saved before the unittextures copy fix
    /// still finds map-bundled textures from the SD7 cache by file name. All
    /// extracted SD7 work dirs share the same file layout (objects3d/,
    /// unittextures/, features/), so a missed name in one is harmless --
    /// duplicates resolve to the same content.
    work_dir_cache: &'a [std::path::PathBuf],
    pool_data_dir: Option<&'a std::path::Path>,
}

impl<'a> AssetSources<'a> {
    /// Try to read a single asset by archive-internal path in priority order:
    /// 1. primary archive (BAR.sdd usually -- often a stub),
    /// 2. sibling archives in the same directory,
    /// 3. the map's primary work directory (project_dir or extracted SD7),
    /// 4. any other cached SD7 work directory (fallback for partial bundles),
    /// 5. the rapid content pool.
    fn read(&self, path: &str) -> Option<(&'static str, Vec<u8>)> {
        if let Some(b) = bar_engine::read_file_from_archive(self.archive, path) {
            return Some(("archive", b));
        }
        for sibling in self.siblings {
            if let Some(b) = bar_engine::read_file_from_archive(sibling, path) {
                return Some(("sibling", b));
            }
        }
        let rel = path.replace('/', std::path::MAIN_SEPARATOR_STR);
        if let Some(wd) = self.work_dir {
            if let Ok(b) = std::fs::read(wd.join(&rel)) {
                return Some(("work_dir", b));
            }
        }
        for cache in self.work_dir_cache {
            if Some(cache.as_path()) == self.work_dir {
                continue;
            }
            if let Ok(b) = std::fs::read(cache.join(&rel)) {
                return Some(("work_dir_cache", b));
            }
        }
        if let Some(dd) = self.pool_data_dir {
            if let Some(b) = bar_engine::read_file_from_rapid_pool(dd, path) {
                return Some(("pool", b));
            }
        }
        None
    }
}

/// Enumerate cached SD7 work directories under `work_dir_root()`. Best-effort:
/// returns empty on any error. Used as a fallback asset source so a .barproj
/// saved before the unittextures copy fix still finds map textures.
fn list_cached_work_dirs() -> Vec<std::path::PathBuf> {
    let root = bar_engine::work_dir_root();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.is_dir() {
                Some(p)
            } else {
                None
            }
        })
        .collect()
}

/// Find and parse an S3O for `feature_name` at archive-internal `path`. Walks
/// the source priority list; on parse failure for any source, falls through to
/// the next (some maps ship broken stubs alongside the real model elsewhere).
fn load_s3o_from_sources(
    feature_name: &str,
    path: &str,
    sources: &AssetSources,
) -> Option<bar_data::S3oMesh> {
    if let Some((source, data)) = sources.read(path) {
        match bar_data::parse_s3o(&data) {
            Ok(mesh) => {
                tracing::info!(
                    feature = %feature_name,
                    path = %path,
                    source,
                    bytes = data.len(),
                    "S3O loaded"
                );
                return Some(mesh);
            }
            Err(e) => {
                tracing::warn!(
                    feature = %feature_name,
                    path = %path,
                    source,
                    bytes = data.len(),
                    err = %e,
                    "S3O parse failed in primary source"
                );
            }
        }
    }
    None
}

/// Resolve a texture filename declared in the S3O header. S3O stores just the
/// base filename; the engine prefixes `unittextures/` to it. If the declared
/// extension does not exist in any source, retry with alternate extensions
/// (Recoil's model loader does the same fallback). `slot` is "tex1" or "tex2"
/// purely for logging so the caller can tell which channel resolved.
fn load_texture_from_sources(
    feature_name: &str,
    slot: &'static str,
    tex_filename: &str,
    sources: &AssetSources,
) -> Option<TextureRgba> {
    let trimmed = tex_filename.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Build candidate paths: declared name first, then a few common BAR/Spring
    // alternates with the same stem.
    let (stem, declared_ext) = match trimmed.rsplit_once('.') {
        Some((s, e)) => (s, Some(e.to_ascii_lowercase())),
        None => (trimmed, None),
    };
    let mut tried: Vec<String> = Vec::new();
    if let Some(ext) = declared_ext.as_deref() {
        tried.push(format!("unittextures/{stem}.{ext}"));
    } else {
        tried.push(format!("unittextures/{stem}"));
    }
    for alt in ["dds", "tga", "png", "bmp"] {
        if declared_ext.as_deref() == Some(alt) {
            continue;
        }
        tried.push(format!("unittextures/{stem}.{alt}"));
    }

    for cand in &tried {
        let Some((source, bytes)) = sources.read(cand) else {
            continue;
        };
        // DDS is the dominant format for BAR mod feature textures
        // (rocks30_snow_color.dds etc.). The `image` crate 0.25 ships
        // without DDS support in our build, so we have to use our own
        // DDS decoder for that extension; without this branch, every
        // mod-feature S3O ends up rendering with the default white
        // texture, producing oddly-tinted features (the texture's
        // baked tone is missing, so the mapinfo lighting × white
        // makes rocks appear tan/khaki instead of grey).
        let ext = std::path::Path::new(cand.as_str())
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        if ext.as_deref() == Some("dds") {
            match bar_data::load_dds_2d_bytes(&bytes) {
                Ok((rgba, w, h)) => {
                    tracing::info!(
                        feature = %feature_name,
                        slot,
                        texture = %cand,
                        source,
                        bytes = bytes.len(),
                        width = w,
                        height = h,
                        "Texture loaded (DDS)"
                    );
                    return Some(TextureRgba {
                        width: w,
                        height: h,
                        rgba,
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        feature = %feature_name,
                        slot,
                        texture = %cand,
                        source,
                        err = %e,
                        "DDS decode failed; trying next candidate"
                    );
                    continue;
                }
            }
        }
        // `image::load_from_memory` does magic-byte sniffing, which fails on
        // headerless TGA 1.0 files (no signature at offset 0; only TGA 2.0 has
        // a footer at the end). Most BAR feature textures are exactly that --
        // headerless TGA -- so always derive the format from the filename
        // extension first, falling back to sniff only if that fails or the
        // extension is unknown.
        let format_hint = ext.as_deref().and_then(image::ImageFormat::from_extension);
        let decode_result = match format_hint {
            Some(fmt) => image::load_from_memory_with_format(&bytes, fmt)
                .or_else(|_| image::load_from_memory(&bytes)),
            None => image::load_from_memory(&bytes),
        };
        match decode_result {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                tracing::info!(
                    feature = %feature_name,
                    slot,
                    texture = %cand,
                    source,
                    bytes = bytes.len(),
                    width = w,
                    height = h,
                    "Texture loaded"
                );
                return Some(TextureRgba {
                    width: w,
                    height: h,
                    rgba: rgba.into_raw(),
                });
            }
            Err(e) => {
                tracing::warn!(
                    feature = %feature_name,
                    slot,
                    texture = %cand,
                    source,
                    err = %e,
                    "Texture decode failed"
                );
            }
        }
    }
    tracing::warn!(
        feature = %feature_name,
        slot,
        declared = %tex_filename,
        "Texture not found in any source"
    );
    None
}

// ── Executor construction (shared between main and tests) ─────────────────────

pub fn make_executor(gpu_context: &Option<GpuContext>) -> Arc<dyn NodeExecutor + Send + Sync> {
    if let Some(ref ctx) = gpu_context {
        tracing::info!("Using HybridExecutor (GPU-accelerated noise)");
        Arc::new(HybridExecutor::new(ctx.clone()))
    } else {
        tracing::info!("No GPU available, using CpuExecutor");
        Arc::new(CpuExecutor)
    }
}

//! Application runner -- the `eframe::App` impl that wires the GUI, layout
//! manager, and all background jobs together.

use std::sync::{mpsc, Arc};

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

pub struct AppRunner {
    pub app: bar_gui::BarEditorApp,
    pub executor: Arc<dyn NodeExecutor + Send + Sync>,
    pub gpu_context: Option<GpuContext>,
    pub render_state: Option<eframe::egui_wgpu::RenderState>,
    pub export_result_rx: Option<mpsc::Receiver<String>>,
    pub progress_rx: Option<mpsc::Receiver<String>>,
    pub export_status: bar_gui::ExportStatus,
    pub sd7_extract_rx: Option<mpsc::Receiver<Result<bar_engine::WorkDirScan, String>>>,
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
}

impl eframe::App for AppRunner {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
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
        let run_bundler_node = self.app.preview.take_run_bundler_node();
        let run_filter_label = run_bundler_node
            .and_then(|id| self.app.graph().get_node(id))
            .map(|n| n.label.clone());
        let no_export_in_flight =
            self.export_result_rx.is_none() && self.pending_export_dir.is_none();
        let should_request_dir = (run_all || run_bundler_node.is_some()) && no_export_in_flight;

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
            self.export_status = match run_bundler_node {
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
            if let Ok(catalog) = rx.try_recv() {
                self.catalog_rx = None;
                tracing::info!(count = catalog.features.len(), "Feature catalog loaded");
                self.feature_catalog = Some(catalog);
                self.layout_manager.mark_features_dirty();
            }
        }

        // Poll SD7 extraction.
        if let Some(ref rx) = self.sd7_extract_rx {
            match rx.try_recv() {
                Ok(Ok(scan)) => {
                    self.app.finish_open_map(scan);
                    self.sd7_extract_rx = None;
                }
                Ok(Err(e)) => {
                    self.app.set_status(format!("Failed to open: {e}"));
                    self.sd7_extract_rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.app
                        .set_status("Open operation failed unexpectedly".to_string());
                    self.sd7_extract_rx = None;
                }
            }
        }
        if let Some(sd7_path) = self.app.project.sd7_open_request.take() {
            let (tx, rx) = mpsc::channel::<Result<bar_engine::WorkDirScan, String>>();
            self.sd7_extract_rx = Some(rx);
            let ctx_clone = ctx.clone();
            std::thread::spawn(move || {
                let result =
                    bar_engine::extract_sd7_to_work_dir(&sd7_path).map_err(|e| e.to_string());
                let _ = tx.send(result);
                ctx_clone.request_repaint();
            });
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

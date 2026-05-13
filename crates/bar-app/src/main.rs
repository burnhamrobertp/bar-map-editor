mod bar_install;

use std::sync::{mpsc, Arc};
use std::time::Instant;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

use bar_compute::GpuContext;
use bar_engine::recipe::PlacedFeature;
use bar_engine::{CpuExecutor, FeatureCatalog, HybridExecutor};
use bar_graph::{evaluate_graph, NodeExecutor};
use bar_render::{pick_terrain, Camera, FeatureInstance, TerrainRenderer, TerrainUpdateParams};

/// Result sent back from a background preview evaluation thread.
///
/// Two quality passes are used for each graph revision:
/// - Low-res (128 px): spawned immediately for fast visual feedback.
/// - High-res (512 px): spawned after the low-res completes + a short cooldown.
///
/// `session_id` and `cache_key` together guarantee that stale results
/// from a previous project or a superseded preview-input set are never
/// applied. `cache_key` is a hash of every input the preview depends on
/// (graph revision, preview-target node, map dimensions, height range),
/// so any of those changing triggers a fresh eval and any in-flight
/// result for an older state is rejected.
struct PreviewResult {
    heightmap: Option<bar_data::Heightmap>,
    texture: Option<bar_data::ColorBuffer>,
    cache_key: u64,
    session_id: u64,
    /// Vertically-scaled height for the renderer (physical scale × artistic exaggeration).
    height_scale: f32,
    /// Render-space Y of the water / lava surface.  Negative = no water.
    water_y: f32,
    /// Water / lava colour (RGB).
    water_color: [f32; 3],
    /// SMF ground shading inputs (from `MapSettings.lighting/water`).
    /// Snapshotted into the eval thread so concurrent edits don't
    /// flicker the preview.
    smf_lighting: bar_render::SmfLighting,
    /// True if this is the coarse first pass (128 px); false for the refined pass (512 px).
    is_low_res: bool,
    /// XZ extents of the terrain mesh (≤ 0.5 each; preserves physical aspect ratio).
    x_extent: f32,
    z_extent: f32,
}

/// Owned counterpart to `bar_render::PreviewFrame`. Lives on the
/// `Session` so the renderer can be re-presented on every UI tick
/// (camera changes, animation) without re-running graph evaluation.
/// On a new eval result we replace this whole struct — never mutate
/// it field-by-field — so leftover per-field state can never bleed
/// across frames.
#[derive(Clone)]
struct OwnedFrame {
    height_scale: f32,
    x_extent: f32,
    z_extent: f32,
    water_y: f32,
    water_color: [f32; 3],
    quality_high: bool,
    smf_lighting: bar_render::SmfLighting,
}

impl OwnedFrame {
    /// Build a `PreviewFrame` for the renderer. `time` comes from the
    /// per-frame animation tick so animation can run without re-evaluating.
    fn as_frame(&self, time: f32) -> bar_render::PreviewFrame {
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

/// All per-project / per-session state.
///
/// Dropping this value atomically releases every GPU resource, background
/// channel, and per-project counter associated with the current project.
/// Opening a new project simply replaces `AppWrapper::session` with a fresh
/// `Session`; no manual field-by-field reset is required.
struct Session {
    /// Terrain mesh + albedo texture for the 3D viewport.
    terrain_renderer: Option<TerrainRenderer>,
    /// Camera orbit / zoom / pan state (reset to default for each new session).
    camera: Camera,
    /// egui handle into the GPU render target (None until first render completes).
    viewport_texture_id: Option<eframe::egui::TextureId>,
    /// Last water plane Y / color passed to the renderer. Cached so a
    /// per-dab mesh re-upload during 3D sculpting can preserve the
    /// water plane instead of dropping it.
    last_water_y: f32,
    last_water_color: [f32; 3],
    /// Owned copy of whatever frame the renderer is currently
    /// presenting. `None` means "nothing wired" and the renderer
    /// shows an empty viewport. Replaced wholesale on each eval
    /// result; never partially mutated, so there's no path for
    /// stale per-field state to leak across frames.
    current_frame: Option<OwnedFrame>,

    // ── Progressive preview state ──────────────────────────────────────────
    /// Cache key for which a low-res result has been applied.
    last_low_res_key: u64,
    /// Cache key for which a high-res (final) result has been applied.
    last_high_res_key: u64,
    /// True while a low-res evaluation thread is in flight.
    low_res_pending: bool,
    /// True while a high-res evaluation thread is in flight.
    high_res_pending: bool,
    /// When the most recent low-res result was applied (used to gate the high-res cooldown).
    low_res_completed_at: Option<Instant>,

    /// Sender half given to each background eval thread (shared by both passes).
    preview_tx: mpsc::Sender<PreviewResult>,
    /// Receiver polled every frame.
    preview_rx: mpsc::Receiver<PreviewResult>,
    /// Monotonically-increasing ID for this session.
    /// Included in every `PreviewResult` so that results from a previous
    /// session's in-flight thread are silently discarded rather than applied.
    session_id: u64,
    /// Wall-clock origin for animation (water waves, cloud drift). Each
    /// frame we set the renderer's time uniform to `elapsed since start`.
    started_at: Instant,
    /// Set by the viewport's refresh button. Consumed at the top of the
    /// next `update()` to forcibly invalidate the progressive-preview
    /// caches (revisions, pending flags, current frame) and bump
    /// session_id so any in-flight result is rejected. A debug override
    /// for cases where the gating logic is suspected of being stuck.
    force_refresh_requested: bool,
    /// True while the feature instance buffer needs rebuilding.
    /// Starts true so the first render after a project load uploads instances.
    features_dirty: bool,
}

impl Session {
    fn new(gpu_context: &Option<GpuContext>, session_id: u64) -> Self {
        let terrain_renderer = gpu_context.as_ref().map(|ctx| {
            let mut r =
                TerrainRenderer::new(&ctx.device, &ctx.queue, wgpu::TextureFormat::Rgba8UnormSrgb);
            r.resize(&ctx.device, 512, 512);
            r
        });
        let (preview_tx, preview_rx) = mpsc::channel::<PreviewResult>();
        Session {
            terrain_renderer,
            camera: Camera::default(),
            viewport_texture_id: None,
            last_water_y: -1.0,
            last_water_color: [0.0, 0.4, 0.6],
            current_frame: None,
            last_low_res_key: u64::MAX,
            last_high_res_key: u64::MAX,
            low_res_pending: false,
            high_res_pending: false,
            low_res_completed_at: None,
            preview_tx,
            preview_rx,
            session_id,
            started_at: Instant::now(),
            force_refresh_requested: false,
            features_dirty: true,
        }
    }
}

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting BAR - Map Editor");

    // Install the i18n backend before any UI code runs. The backend
    // walks the embedded `language/<locale>/<namespace>.json` tree
    // (bar-localizations layout) and registers translations under
    // the right locales, replacing the empty default backend that
    // `i18n!()` installs at startup.
    bar_gui::i18n::init();

    // Prune SD7 extraction work directories that haven't been touched in 14
    // days. Best-effort; no action required from the user. Keeps the cache
    // dir from growing unbounded while still preserving recent in-place edits.
    bar_engine::prune_old_work_dirs(std::time::Duration::from_secs(60 * 60 * 24 * 14));

    // Load the application icon from the bundled PNG.
    let icon = load_icon();

    // Restore previous window position/size if available; otherwise default
    // to maximized so first-run users land in a usable workspace instead of
    // a 1440×900 island.
    //
    // Window-state restoration on Windows has a sharp edge: the
    // ViewportBuilder's `with_maximized(true)` is treated as advisory by
    // winit/Windows — the OS honours the initial size/position values and
    // applies the maximize flag *internally* without actually maximising
    // the window. Result: the window paints at the saved rect with a
    // taskbar-height chunk chopped off. We work around it by always
    // setting an inner_size (so the restored unmaximized rect is sensible
    // when the user un-maximises later) and then sending an explicit
    // `ViewportCommand::Maximized(true)` once the GUI is up. The runtime
    // command is honoured even when the initial hint isn't.
    let saved_settings = bar_gui::Settings::load();
    let (default_pos, default_size, default_maximized) = match saved_settings.window.as_ref() {
        Some(w) => (Some([w.x, w.y]), [w.width, w.height], w.maximized),
        None => (None, [1440.0, 900.0], true),
    };

    // Run the GUI application. The window is created hidden and made
    // visible after the first frame paints; otherwise winit/Windows
    // briefly flashes the OS-default white background between window
    // creation and the first egui frame, which is jarring on every
    // reload.
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size(default_size)
        .with_min_inner_size([800.0, 600.0])
        .with_maximized(default_maximized)
        .with_visible(false)
        .with_title("BAR - Map Editor");
    if let Some(pos) = default_pos {
        viewport = viewport.with_position(pos);
    }
    if let Some(icon_data) = icon {
        viewport = viewport.with_icon(icon_data);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "BAR - Map Editor",
        options,
        Box::new(move |cc| {
            let mut app = bar_gui::BarEditorApp::new(cc);

            // Extract wgpu render state from eframe for shared GPU access
            let render_state = cc.wgpu_render_state.clone();

            let gpu_context = render_state
                .as_ref()
                .map(|rs| GpuContext::from_existing(rs.device.clone(), rs.queue.clone()));

            // Create HybridExecutor if GPU available, otherwise use CPU only
            let executor: Arc<dyn NodeExecutor + Send + Sync> = if let Some(ref ctx) = gpu_context {
                tracing::info!("Using HybridExecutor (GPU-accelerated noise)");
                Arc::new(HybridExecutor::new(ctx.clone()))
            } else {
                tracing::info!("No GPU available, using CpuExecutor");
                Arc::new(CpuExecutor)
            };

            // Start with an empty session (no project loaded yet)
            let initial_session = Session::new(&gpu_context, 0);

            // Detect BAR install once at startup and populate the version
            // picker labels so the toolbar can show the chevron immediately.
            let bar_install = bar_install::BarVersions::detect();
            if let Some(ref versions) = bar_install {
                app.bar_versions.game_labels =
                    versions.games.iter().map(|g| g.label.clone()).collect();
                app.bar_versions.engine_labels =
                    versions.engines.iter().map(|e| e.label.clone()).collect();
            }

            // Auto-detect a game archive for the feature catalog if the user
            // has not already configured one. Pick the first locally installed
            // archive (skipping the synthetic "latest" rapid entry which has no path).
            if saved_settings.selected_game_archive.is_none() {
                if let Some(path) = bar_install
                    .as_ref()
                    .and_then(|v| v.games.iter().find_map(|g| g.path.clone()))
                {
                    app.set_game_archive(path);
                }
            }

            Ok(Box::new(AppWrapper {
                app,
                executor,
                gpu_context,
                render_state,
                export_result_rx: None,
                progress_rx: None,
                export_status: bar_gui::ExportStatus::Idle,
                sd7_extract_rx: None,
                test_in_bar_rx: None,
                pending_export_dir: None,
                bar_install,
                session: Some(initial_session),
                next_session_id: 1,
                pending_maximize: default_maximized,
                has_shown_window: false,
                feature_catalog: None,
                catalog_rx: None,
                catalog_archive_path: None,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))?;

    Ok(())
}

/// Wraps the GUI app with graph evaluation, 3D viewport, and export capability.
struct PendingExportDir {
    /// Receiver carrying the user's folder choice (None if cancelled).
    rx: mpsc::Receiver<Option<std::path::PathBuf>>,
    /// Cached label of the targeted bundler at request time, used to
    /// filter `execute_bundlers`. Captured up front so a node rename
    /// or deletion mid-flow doesn't change which bundle gets exported.
    run_filter_label: Option<String>,
}

struct AppWrapper {
    app: bar_gui::BarEditorApp,
    // ── Infrastructure (never replaced, lives for the process lifetime) ─────
    /// Shared compute backend — Arc allows cheap clone into background threads.
    executor: Arc<dyn NodeExecutor + Send + Sync>,
    gpu_context: Option<GpuContext>,
    render_state: Option<eframe::egui_wgpu::RenderState>,
    // ── Per-export communication (one-shot, not per-project) ──────────────
    /// Receiver for export thread results (Some while an export is running).
    export_result_rx: Option<mpsc::Receiver<String>>,
    /// Per-node progress messages from the active export or test-in-bar thread.
    /// Drained each frame and routed to the app log / status bar.
    progress_rx: Option<mpsc::Receiver<String>>,
    /// Which bundler(s) the in-flight export is for. Set when an export
    /// begins, cleared when the result is consumed. Pushed to the GUI each
    /// frame so the bundle buttons can show busy state.
    export_status: bar_gui::ExportStatus,
    /// Receiver for SD7 extraction results (Some while extraction is in progress).
    sd7_extract_rx: Option<mpsc::Receiver<Result<bar_engine::WorkDirScan, String>>>,
    /// In-flight "Test in BAR" pipeline. Background thread exports the
    /// project to a temp dir and sends back the SD7 path; the main loop
    /// then copies the file into BAR's maps dir and spawns the lobby.
    test_in_bar_rx: Option<mpsc::Receiver<Result<(std::path::PathBuf, String), String>>>,
    /// Two-phase export flow. Phase 1: when the user clicks Run, the
    /// native folder picker is spawned on a worker thread (synchronous
    /// `pick_folder` would freeze the egui main loop for the full
    /// modal lifetime — typically hundreds of ms even before the user
    /// interacts). The receiver here delivers the chosen path. Phase 2:
    /// once the path arrives, the actual evaluate-graph + bundle export
    /// is spawned on `export_result_rx`. Carries the original request
    /// context (which bundler, optional filter label) so the export
    /// matches what the user clicked even if state changed mid-flow.
    pending_export_dir: Option<PendingExportDir>,
    /// Detected BAR install with all available game/engine versions.
    /// `None` when BAR is not installed on this machine.
    bar_install: Option<bar_install::BarVersions>,
    // ── Per-project session (replaced atomically on every project switch) ──
    session: Option<Session>,
    next_session_id: u64,
    /// When `true`, the next `update()` sends a runtime
    /// `ViewportCommand::Maximized(true)` and clears the flag. Belt-and-
    /// braces against Windows ignoring the `with_maximized` builder hint:
    /// the runtime command is reliably honoured. Set from the saved
    /// window state at startup.
    pending_maximize: bool,
    /// `false` until the first frame has been queued, then flips to
    /// `true` and a viewport-show command is emitted. We start the
    /// window hidden (see `with_visible(false)` in `main`) so the
    /// OS-default white background doesn't flash before egui paints.
    has_shown_window: bool,
    /// Feature type catalog built from the selected BAR game archive.
    /// `None` until the background load completes or no archive is configured.
    feature_catalog: Option<FeatureCatalog>,
    /// Background catalog load receiver. `Some` while a load is in flight.
    catalog_rx: Option<mpsc::Receiver<FeatureCatalog>>,
    /// Archive path used to build the current `feature_catalog`. Compared
    /// each frame against `settings.selected_game_archive` to detect changes.
    catalog_archive_path: Option<std::path::PathBuf>,
}

impl eframe::App for AppWrapper {
    fn update(&mut self, ctx: &eframe::egui::Context, frame: &mut eframe::Frame) {
        use eframe::egui;

        // Honour pending maximize on the first paint. Done here rather
        // than in main() because the ViewportCommand is the reliable
        // path on Windows; `with_maximized(true)` on ViewportBuilder is
        // treated as advisory and routinely silently ignored.
        if self.pending_maximize {
            self.pending_maximize = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }

        // Reveal the window after the first frame has been queued. The
        // window was created hidden in `main` so the OS doesn't paint
        // a white default background before egui's first frame lands.
        if !self.has_shown_window {
            self.has_shown_window = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }

        // Hand the editor's window + display handles to bar-gui so
        // native file dialogs spawned from worker threads can be
        // parented to *our* window instead of whichever OS window
        // happens to be foreground at dialog-spawn time.
        {
            use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
            let handles = match (frame.window_handle(), frame.display_handle()) {
                (Ok(w), Ok(d)) => Some((w.as_raw(), d.as_raw())),
                _ => None,
            };
            self.app.set_parent_window_handles(handles);
        }

        // ── Window close intercept ──────────────────────────────────────────
        // The OS / X-button sets close_requested on the viewport. The GUI
        // notices this in its own update() and either approves it (clean) or
        // shows an unsaved-changes prompt. Until the GUI signals approval via
        // `take_allow_close()`, cancel the close so eframe doesn't tear down
        // the window. This lets the modal stay on screen.
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && !self.app.take_allow_close() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }

        // Track current window rect + maximized flag so the GUI can persist
        // them on shutdown.
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

        // ── Drag-drop file open ────────────────────────────────────────────
        // First .barproj or .sd7 dropped on the window opens. Anything else is
        // ignored (silent — egui already shows a hover indicator).
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
        // Each message has the form "[XX%] NodeLabel"; routed to the status
        // bar and log so the user can see the process is still running.
        if let Some(ref prx) = self.progress_rx {
            while let Ok(msg) = prx.try_recv() {
                self.app.set_status(msg);
            }
        }

        // Poll for completed export result
        if let Some(ref rx) = self.export_result_rx {
            if let Ok(msg) = rx.try_recv() {
                // Final drain before the result message so no progress
                // lines are lost if both arrive in the same frame.
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

        // Handle Run button (toolbar = all bundlers; per-node button = that bundler only)
        let run_all = self.app.preview.take_run_requested();
        let run_bundler_node = self.app.preview.take_run_bundler_node();
        let run_filter_label = run_bundler_node
            .and_then(|id| self.app.graph().get_node(id))
            .map(|n| n.label.clone());
        let no_export_in_flight =
            self.export_result_rx.is_none() && self.pending_export_dir.is_none();
        let should_request_dir = (run_all || run_bundler_node.is_some()) && no_export_in_flight;

        // Phase 1: spawn the folder picker on a worker. The egui main
        // loop keeps rendering while the OS dialog is up, instead of
        // freezing for the dialog's whole lifetime.
        if should_request_dir {
            let (tx, rx) = mpsc::channel::<Option<std::path::PathBuf>>();
            let ctx_clone = ctx.clone();
            // Capture the editor's window + display handles on the main
            // thread so the folder picker is parented to our window
            // rather than the OS's current foreground.
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
            // Reflect "an export is in progress" the moment the
            // dialog opens — gates other run buttons + drives the
            // bundler-busy UI state.
            self.export_status = match run_bundler_node {
                Some(id) => bar_gui::ExportStatus::One(id),
                None => bar_gui::ExportStatus::All,
            };
        }

        // Phase 2: poll the folder picker. Cancel → drop the pending
        // export and clear the busy status. Path → spawn the
        // evaluate-graph + bundler export on a separate worker.
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

                    std::thread::spawn(move || {
                        let progress_cb = |msg: &str| {
                            let _ = progress_tx.send(msg.to_string());
                            ctx_progress.request_repaint();
                        };
                        let msg = match bar_graph::evaluate_graph_with_progress(
                            &graph,
                            executor.as_ref(),
                            w,
                            h,
                            &progress_cb,
                        ) {
                            Ok(outputs) => {
                                let filter = run_filter_label.as_deref();
                                match bar_engine::execute_bundlers(
                                    &graph,
                                    &outputs,
                                    &recipe,
                                    &output_dir,
                                    filter,
                                ) {
                                    Ok(results) if !results.is_empty() => {
                                        format!(
                                            "Exported {} bundle(s) to {}",
                                            results.len(),
                                            output_dir.display(),
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
                    // User cancelled the folder picker. Clear the
                    // busy status so the UI returns to idle.
                    self.export_status = bar_gui::ExportStatus::Idle;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    // Dialog still open — keep the pending entry so
                    // we'll poll again next frame.
                    self.pending_export_dir = Some(pending);
                    ctx.request_repaint();
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    // Worker panicked or hung up; treat as cancel.
                    self.export_status = bar_gui::ExportStatus::Idle;
                }
            }
        }

        // ── Test in BAR: handle button + chain export → lobby launch ──────
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

        // Push current export status to the GUI so bundle buttons render
        // busy state. Cheap (single Copy) and idempotent.
        self.app.preview.set_export_status(self.export_status);

        // Run the GUI (menus, node palette, properties, status bar)
        self.app.update(ctx, frame);

        // ── Feature catalog: detect archive change, poll background load ─────
        // When the selected game archive changes (settings edit or auto-detect),
        // spawn a background thread to parse the Lua feature definitions. On
        // completion, store the catalog and mark features dirty so the render
        // instances are rebuilt with updated tints.
        let desired_archive = self.app.settings().selected_game_archive.clone();
        if desired_archive != self.catalog_archive_path {
            // Archive changed -- cancel any in-flight load (just drop the rx).
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
                // Rebuild feature instances with updated tints.
                if let Some(ref mut session) = self.session {
                    session.features_dirty = true;
                }
            }
        }

        // Poll for completed SD7 extraction
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

        // Handle SD7 open requests queued by the GUI file dialog
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

        // Replace the session atomically when the project changes.
        // Dropping the old Session frees its GPU buffers, channels, and camera
        // state; the new Session starts completely clean — no manual per-field
        // reset list required.
        if self.app.project.take_graph_reset() {
            let id = self.next_session_id;
            self.next_session_id += 1;
            self.session = Some(Session::new(&self.gpu_context, id));
        }

        // Honour a force-refresh request from the viewport's refresh
        // button: bump session_id so any in-flight pass result is
        // rejected, then clear all the gating state so the next frame
        // re-spawns both passes and re-uploads the mesh.
        if let Some(ref mut session) = self.session {
            if session.force_refresh_requested {
                session.force_refresh_requested = false;
                session.session_id = self.next_session_id;
                self.next_session_id += 1;
                session.last_low_res_key = u64::MAX;
                session.last_high_res_key = u64::MAX;
                session.low_res_pending = false;
                session.high_res_pending = false;
                session.low_res_completed_at = None;
                session.current_frame = None;
            }
        }

        // All remaining logic operates on the current session (if any).
        let Some(ref mut session) = self.session else {
            return;
        };

        // Upload feature instances once per session (dirty flag set on new Session).
        if session.features_dirty {
            if let (Some(ref mut renderer), Some(ref gpu)) =
                (&mut session.terrain_renderer, &self.gpu_context)
            {
                let (w, h) = self.app.map.dimensions();
                let (min_h, max_h) = self.app.map.height_range();
                let instances = build_feature_instances(
                    &self.app.map.features,
                    w,
                    h,
                    min_h,
                    max_h,
                    self.feature_catalog.as_ref(),
                    self.app.paint.heightmap.as_ref(),
                );
                renderer.update_feature_instances(&gpu.device, &instances);
                session.features_dirty = false;
            }
        }

        // ── Progressive preview: poll completed background evaluations ──────
        //
        // Two quality passes per cache key:
        //   1. Low-res  (128 px) — spawned immediately for fast visual feedback.
        //   2. High-res (512 px) — spawned after the low-res completes + 300 ms cooldown.
        //
        // Results from both passes share a single channel.  `is_low_res` tells us
        // which pending flag to clear.  `session_id + cache_key` guards against
        // stale results from a previous project or a superseded preview-input set.
        let current_key = self.app.preview_cache_key();
        while let Ok(result) = session.preview_rx.try_recv() {
            // Always clear the right in-flight flag, even for stale results, so we
            // never deadlock waiting for a thread that has already exited.
            if result.is_low_res {
                session.low_res_pending = false;
            } else {
                session.high_res_pending = false;
            }

            if result.session_id == session.session_id && result.cache_key == current_key {
                // grid_n: low-res passes use a coarse mesh so the user can
                // see the refinement when the high-res pass arrives. High-res
                // uses up to the full heightmap resolution (capped at 2048).
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
                    if !result.is_low_res {
                        self.app.set_inspector_heightmap(heightmap.clone());
                        if let Some(ref tex) = result.texture {
                            self.app.paint.color_buffer = Some(tex.clone());
                        }
                    }
                    session.last_water_y = result.water_y;
                    session.last_water_color = result.water_color;
                    session.current_frame = Some(OwnedFrame {
                        height_scale: result.height_scale,
                        x_extent: result.x_extent,
                        z_extent: result.z_extent,
                        water_y: result.water_y,
                        water_color: result.water_color,
                        quality_high: !result.is_low_res,
                        smf_lighting: result.smf_lighting,
                    });
                    if let Some(ref gpu) = self.gpu_context {
                        if let Some(ref mut renderer) = session.terrain_renderer {
                            renderer.update_heightmap(
                                &gpu.device,
                                &gpu.queue,
                                &heightmap,
                                TerrainUpdateParams {
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
                } else if !result.is_low_res {
                    // High-pass eval with nothing wired to the preview target.
                    session.current_frame = None;
                }
                // Low-pass with no heightmap: keep the current frame so the
                // viewport isn't blanked while waiting for the high-pass.

                if let Some(ref gpu) = self.gpu_context {
                    if let Some(ref mut renderer) = session.terrain_renderer {
                        let elapsed = session.started_at.elapsed().as_secs_f32();
                        let frame_borrow =
                            session.current_frame.as_ref().map(|f| f.as_frame(elapsed));
                        renderer.render(
                            &gpu.device,
                            &gpu.queue,
                            &session.camera,
                            frame_borrow.as_ref(),
                        );
                        // Only expose the texture once we have real terrain
                        // data. Registering a blank render would dismiss the
                        // loading spinner before the first valid frame arrives.
                        if session.current_frame.is_some() {
                            Self::update_viewport_texture_on(
                                &mut session.viewport_texture_id,
                                &session.terrain_renderer,
                                &self.render_state,
                                ctx,
                            );
                        }
                    }
                }
                if result.is_low_res {
                    session.last_low_res_key = result.cache_key;
                    session.low_res_completed_at = Some(Instant::now());
                } else {
                    session.last_high_res_key = result.cache_key;
                }
            }
        }

        // ── Progressive preview: spawn passes as needed ───────────────────
        if !self.app.graph().nodes().is_empty() {
            // Compute height/water params once; both passes share the same values.
            let (w, h) = self.app.map.dimensions();
            let (height_scale, water_y, x_extent, z_extent) = {
                let (min_h, max_h) = self.app.map.height_range();
                // 1:1 with the engine. Spring renders 1 elmo X = 1 elmo Y =
                // 1 elmo Z; we mirror that by normalising X/Z to a unit-cube
                // mesh and scaling Y by the same factor (1 / (pm * 8)). The
                // user can orbit/zoom the preview to inspect terrain detail
                // — the same way players adjust the camera in-game.
                let pw = (w as f32 - 1.0).max(1.0);
                let ph = (h as f32 - 1.0).max(1.0);
                let pm = pw.max(ph);
                let xe = (0.5 * pw / pm).min(0.5);
                let ze = (0.5 * ph / pm).min(0.5);
                let height_range = (max_h - min_h).abs().max(1.0);
                let hs = (height_range / (pm * 8.0)).max(0.005);
                // water_y: render-space Y of Spring's Y=0 elmo plane.
                // 0 elmos normalised = -min_h / height_range, then scaled by hs.
                let wy = if min_h < 0.0 {
                    (-min_h / height_range) * hs
                } else {
                    -1.0
                };
                (hs, wy, xe, ze)
            };
            let water_color = [0.2_f32, 0.45, 0.75];
            // Snapshot SMF lighting once per frame so both the low-res
            // and high-res passes use consistent values even if the
            // user is mid-edit on a slider.
            let smf = self.app.smf_lighting();
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
            let preview_node_id = self.app.preview.node();
            let session_id = session.session_id;

            // Pass 1 — low-res (128 px): fires immediately when any preview
            // input changes. Allowed to run even while a stale high-res thread
            // is still in flight; the stale result will be discarded by the
            // session_id + cache_key guard.
            let needs_low_res =
                current_key != session.last_low_res_key && current_key != session.last_high_res_key;

            if needs_low_res && !session.low_res_pending {
                let low_res_size = 128u32.min(w.min(h));
                let graph = self.app.graph().clone();
                let tx = session.preview_tx.clone();
                let ctx_clone = ctx.clone();
                let executor = Arc::clone(&self.executor);

                session.low_res_pending = true;
                std::thread::spawn(move || {
                    let (heightmap, texture) =
                        eval_preview(&graph, executor.as_ref(), low_res_size, preview_node_id);
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
                    });
                    ctx_clone.request_repaint();
                });
            }

            // Pass 2 — high-res (512 px): spawned once the low-res is done and a
            // short cooldown has elapsed so rapid edits don't queue expensive renders.
            let needs_high_res = current_key != session.last_high_res_key;
            let cooldown_done = session
                .low_res_completed_at
                .map(|t| t.elapsed().as_millis() >= 300)
                .unwrap_or(false);

            if needs_high_res
                && !session.low_res_pending
                && !session.high_res_pending
                && cooldown_done
            {
                let high_res_size = 512u32.min(w.min(h));
                let graph = self.app.graph().clone();
                let tx = session.preview_tx.clone();
                let ctx_clone = ctx.clone();
                let executor = Arc::clone(&self.executor);

                session.high_res_pending = true;
                std::thread::spawn(move || {
                    let (heightmap, texture) =
                        eval_preview(&graph, executor.as_ref(), high_res_size, preview_node_id);
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
                    });
                    ctx_clone.request_repaint();
                });
            }
        }

        // ── Per-frame animation tick ────────────────────────────────────────
        // Re-render the preview each frame so animated water + drifting
        // clouds stay alive. Cheap: we don't rebuild the mesh, just push
        // a fresh time uniform and submit one draw. Skipped when there's
        // no mesh yet (nothing to animate) or no preview window open.
        if self.app.preview.is_open() {
            if let Some(ref gpu) = self.gpu_context {
                if let Some(ref mut renderer) = session.terrain_renderer {
                    if let Some(ref owned) = session.current_frame {
                        let elapsed = session.started_at.elapsed().as_secs_f32();
                        let frame = owned.as_frame(elapsed);
                        renderer.render(&gpu.device, &gpu.queue, &session.camera, Some(&frame));
                        Self::update_viewport_texture_on(
                            &mut session.viewport_texture_id,
                            &session.terrain_renderer,
                            &self.render_state,
                            ctx,
                        );
                        // Continuous animation — request a redraw so this
                        // path runs again next frame. ~60fps.
                        ctx.request_repaint_after(std::time::Duration::from_millis(16));
                    }
                }
            }
        }

        // When the Sculpt3D layout is active the central panel is left
        // unclaimed by bar-gui so we can fill it here with the 3D viewport.
        // `session` is already in scope from the guard above.
        if self.app.active_layout() == bar_gui::Layout::Sculpt3D {
            egui::CentralPanel::default().show(ctx, |ui| {
                Self::draw_viewport_on(
                    session,
                    &self.gpu_context,
                    &self.render_state,
                    ui,
                    ctx,
                    &mut self.app,
                );
            });
        }

        // Show 3D viewport window when a preview has been opened. Default
        // position docks the window to the right edge of the screen, just
        // left of the Properties side panel — that's a far more useful
        // initial location than the top-left corner.
        // Not shown in Sculpt3D layout -- the embedded panel above takes over.
        if self.app.preview.is_open() && self.app.active_layout() != bar_gui::Layout::Sculpt3D {
            let title = self.app.preview_node_label();
            let mut preview_open = true;
            // Properties panel is 250 px wide; allow ~24 px gutter.
            let screen = ctx.screen_rect();
            let preview_w = 512.0_f32;
            let preview_h = 512.0_f32;
            let default_x = (screen.right() - 250.0 - preview_w - 24.0).max(20.0);
            let default_y = screen.top() + 60.0;
            egui::Window::new(&title)
                .default_size([preview_w, preview_h])
                .default_pos([default_x, default_y])
                .open(&mut preview_open)
                .show(ctx, |ui| {
                    Self::draw_viewport_on(
                        session,
                        &self.gpu_context,
                        &self.render_state,
                        ui,
                        ctx,
                        &mut self.app,
                    );
                });
            if !preview_open {
                self.app.preview.set_open(false);
            }
        }
    }
}

impl AppWrapper {
    /// Begin the "Test in BAR" flow: detect the BAR install, kick off a
    /// background thread that exports the current project to a temp
    /// directory. The completed SD7 path comes back through
    /// `test_in_bar_rx`; `finish_test_in_bar` then copies it into BAR
    /// and launches the engine directly into a skirmish.
    fn start_test_in_bar(&mut self, ctx: &eframe::egui::Context) {
        if self.bar_install.is_none() {
            self.app.set_status(
                "BAR install not found. Install Beyond All Reason or set the path manually."
                    .to_string(),
            );
            return;
        }

        // Export to a temp directory unique to this run so concurrent
        // tests don't collide.
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
                &progress_cb,
            ) {
                Ok(outputs) => {
                    match bar_engine::execute_bundlers(&graph, &outputs, &recipe, &temp_dir, None) {
                        Ok(results) => {
                            // Pick the first SD7 produced. Bundlers can
                            // emit several, but for "Test in BAR" we
                            // launch with the first one.
                            results
                                .into_iter()
                                .find(|r| {
                                    r.output_path.extension().and_then(|s| s.to_str())
                                        == Some("sd7")
                                })
                                .map(|r| Ok((r.output_path, r.map_internal_name)))
                                .unwrap_or_else(|| Err("Bundler produced no SD7".to_string()))
                        }
                        Err(e) => Err(format!("Bundler error: {e}")),
                    }
                }
                Err(e) => Err(format!("Graph evaluation failed: {e:?}")),
            };
            let _ = tx.send(result);
            ctx_clone.request_repaint();
        });
    }

    /// Copy the just-built SD7 into BAR's maps directory and launch the
    /// engine directly into a skirmish using the versions selected in the
    /// toolbar picker. Surfaces the result in the status bar.
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

    /// Register or update the terrain render texture in egui.
    fn update_viewport_texture_on(
        viewport_texture_id: &mut Option<eframe::egui::TextureId>,
        terrain_renderer: &Option<TerrainRenderer>,
        render_state: &Option<eframe::egui_wgpu::RenderState>,
        ctx: &eframe::egui::Context,
    ) {
        let Some(ref renderer) = terrain_renderer else {
            return;
        };
        let Some(view) = renderer.output_view() else {
            return;
        };
        let Some(ref rs) = render_state else { return };

        let mut egui_rend = rs.renderer.write();

        if let Some(tex_id) = *viewport_texture_id {
            egui_rend.update_egui_texture_from_wgpu_texture(
                &rs.device,
                view,
                wgpu::FilterMode::Linear,
                tex_id,
            );
        } else {
            let tex_id =
                egui_rend.register_native_texture(&rs.device, view, wgpu::FilterMode::Linear);
            *viewport_texture_id = Some(tex_id);
        }

        ctx.request_repaint();
    }

    /// Apply one brush dab at the cursor's current pick point on the
    /// terrain. Re-uploads the mesh after the dab so the user sees the
    /// edit immediately. Cheap when no hit (cursor over the sky):
    /// returns without touching anything.
    fn apply_sculpt_dab_at_cursor(
        session: &mut Session,
        gpu_context: &Option<GpuContext>,
        response: &eframe::egui::Response,
        ctx: &eframe::egui::Context,
        app: &mut bar_gui::BarEditorApp,
    ) {
        let Some(pointer) = ctx.pointer_latest_pos() else {
            tracing::debug!("sculpt: no pointer pos");
            return;
        };
        let rect = response.rect;
        if !rect.contains(pointer) {
            tracing::debug!(
                "sculpt: pointer outside rect ({:?} not in {:?})",
                pointer,
                rect
            );
            return;
        }
        let cursor_uv = (
            (pointer.x - rect.left()) / rect.width().max(1.0),
            (pointer.y - rect.top()) / rect.height().max(1.0),
        );
        let aspect = rect.width().max(1.0) / rect.height().max(1.0);
        let Some(hm) = app.paint.heightmap.as_ref() else {
            tracing::debug!("sculpt: no inspector heightmap");
            return;
        };
        let Some(renderer) = session.terrain_renderer.as_ref() else {
            tracing::debug!("sculpt: no terrain renderer");
            return;
        };
        let (height_scale, x_extent, z_extent) = renderer.mesh_extents();
        let pick = pick_terrain(
            &session.camera,
            aspect,
            cursor_uv,
            hm,
            x_extent,
            z_extent,
            height_scale,
        );
        let Some(p) = pick else {
            tracing::debug!(
                "sculpt: pick_terrain miss (uv={:?}, extents={:?})",
                cursor_uv,
                (x_extent, z_extent, height_scale)
            );
            return;
        };
        tracing::debug!("sculpt: dab at hm=({:.1},{:.1})", p.hm_x, p.hm_y);
        let stroke_starting = !response.dragged_by(eframe::egui::PointerButton::Primary)
            || response.drag_started_by(eframe::egui::PointerButton::Primary);

        // Snapshot the selected node's id and type before mutably borrowing app.
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
            // Colour stroke: re-upload the full albedo texture.
            if let (Some(ref gpu), Some(updated)) = (gpu_context, app.paint.color_buffer.clone()) {
                if let Some(ref mut renderer) = session.terrain_renderer {
                    renderer.update_albedo(&gpu.device, &gpu.queue, &updated);
                    let elapsed = session.started_at.elapsed().as_secs_f32();
                    let frame_borrow = session.current_frame.as_ref().map(|f| f.as_frame(elapsed));
                    renderer.render(
                        &gpu.device,
                        &gpu.queue,
                        &session.camera,
                        frame_borrow.as_ref(),
                    );
                }
            }
        } else {
            // Heightmap stroke: upload only the dirty rectangle around the brush.
            if let (Some(ref gpu), Some(updated)) = (gpu_context, app.paint.heightmap.clone()) {
                if let Some(ref mut renderer) = session.terrain_renderer {
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
                    let elapsed = session.started_at.elapsed().as_secs_f32();
                    let frame_borrow = session.current_frame.as_ref().map(|f| f.as_frame(elapsed));
                    renderer.render(
                        &gpu.device,
                        &gpu.queue,
                        &session.camera,
                        frame_borrow.as_ref(),
                    );
                }
            }
        }
    }

    /// Draw the 3D viewport panel contents.
    fn draw_viewport_on(
        session: &mut Session,
        gpu_context: &Option<GpuContext>,
        render_state: &Option<eframe::egui_wgpu::RenderState>,
        ui: &mut eframe::egui::Ui,
        ctx: &eframe::egui::Context,
        app: &mut bar_gui::BarEditorApp,
    ) {
        use eframe::egui;

        let is_sculpt_layout = app.active_layout() == bar_gui::Layout::Sculpt3D;
        if is_sculpt_layout {
            ui.small(bar_gui::i18n::t("editor.viewport_3d.sculpt_controls_hint"));
        } else {
            ui.horizontal(|ui| {
                ui.small(bar_gui::i18n::t("editor.viewport_3d.controls_hint"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let resp = ui
                        .small_button("\u{27F3}")
                        .on_hover_text(bar_gui::i18n::t("editor.viewport_3d.force_refresh"));
                    if resp.clicked() {
                        session.force_refresh_requested = true;
                    }
                });
            });
        }
        ui.separator();

        let available_size = ui.available_size();
        // Convert to integer pixel dimensions for the GPU texture (minimum 1×1).
        let vp_w = (available_size.x as u32).max(1);
        let vp_h = (available_size.y as u32).max(1);

        // Resize the renderer whenever the viewport dimensions change so the
        // GPU texture is always the same size as the display area.  Without
        // this the fixed-size texture is stretched to fill the window, which
        // distorts the perspective and squashes / stretches the terrain.
        if let Some(ref gpu) = gpu_context {
            if let Some(ref mut renderer) = session.terrain_renderer {
                if renderer.width != vp_w || renderer.height != vp_h {
                    renderer.resize(&gpu.device, vp_w, vp_h);
                    let elapsed = session.started_at.elapsed().as_secs_f32();
                    let frame_borrow = session.current_frame.as_ref().map(|f| f.as_frame(elapsed));
                    renderer.render(
                        &gpu.device,
                        &gpu.queue,
                        &session.camera,
                        frame_borrow.as_ref(),
                    );
                    // Re-register the new texture view with egui only when
                    // we have real terrain data; otherwise the loading
                    // spinner is dismissed by a blank resize render.
                    if session.current_frame.is_some() {
                        Self::update_viewport_texture_on(
                            &mut session.viewport_texture_id,
                            &session.terrain_renderer,
                            render_state,
                            ctx,
                        );
                    }
                }
            }
        }

        if let Some(tex_id) = session.viewport_texture_id {
            // Display size matches the texture exactly — no stretching.
            let image = egui::Image::new(egui::load::SizedTexture::new(tex_id, available_size))
                .fit_to_exact_size(available_size)
                .sense(egui::Sense::click_and_drag());
            let response = ui.add(image);

            // Overlay a spinner in the bottom-right corner while the high-res
            // refinement pass is in progress.  The low-res pass is fast enough
            // (< 100 ms typically) that a spinner would only flicker, so we skip
            // it there and only indicate the slower high-res refinement.
            if session.high_res_pending {
                let corner = response.rect.right_bottom() - egui::vec2(22.0, 22.0);
                ui.put(
                    egui::Rect::from_center_size(corner, egui::Vec2::splat(20.0)),
                    egui::Spinner::new()
                        .size(16.0)
                        .color(egui::Color32::from_rgba_unmultiplied(255, 200, 80, 220)),
                );
                // Drive spinner animation.
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }

            Self::handle_camera_input_on(session, gpu_context, render_state, &response, ctx, app);
        } else {
            // No texture yet -- initial render in progress. Show a centered
            // spinner sized to the context: large in the dedicated sculpt
            // workspace, medium in the floating preview window.
            let spinner_size = if is_sculpt_layout { 80.0 } else { 48.0 };
            ui.centered_and_justified(|ui| {
                ui.add(
                    egui::Spinner::new()
                        .size(spinner_size)
                        .color(egui::Color32::from_rgba_unmultiplied(255, 200, 80, 220)),
                );
            });
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    /// Handle mouse input for camera orbit/zoom/pan, and 3D sculpt
    /// brush application when the GUI is in Sculpt mode.
    fn handle_camera_input_on(
        session: &mut Session,
        gpu_context: &Option<GpuContext>,
        render_state: &Option<eframe::egui_wgpu::RenderState>,
        response: &eframe::egui::Response,
        ctx: &eframe::egui::Context,
        app: &mut bar_gui::BarEditorApp,
    ) {
        let mut camera_changed = false;
        let sculpt_active = app.is_sculpt_input_active();

        // Brush cursor visualisation. When sculpt mode is active and
        // the cursor is over the viewport, project the cursor through
        // the terrain and feed the world-space pick result into the
        // renderer so the shader can draw a ring at the brush
        // footprint. None when the cursor leaves the viewport — the
        // ring disappears so the user knows their next click won't
        // land. Updated every frame regardless of drag state, so the
        // ring follows the cursor while idle.
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
                let renderer = session.terrain_renderer.as_ref()?;
                let (height_scale, x_extent, z_extent) = renderer.mesh_extents();
                let pick = pick_terrain(
                    &session.camera,
                    aspect,
                    uv,
                    hm,
                    x_extent,
                    z_extent,
                    height_scale,
                )?;
                // Brush radius in world-space units. The mesh spans
                // 2 * x_extent across `hm.width()` heightmap pixels,
                // so radius_px → radius_world is just the per-pixel
                // world step times the brush radius.
                let world_per_px = (2.0 * x_extent) / hm.width().max(1) as f32;
                let radius_world = app.paint.brush.radius_px * world_per_px;
                Some((pick.world.x, pick.world.z, radius_world))
            })
        } else {
            None
        };
        if let Some(ref mut renderer) = session.terrain_renderer {
            renderer.set_brush_cursor(cursor_world);
        }

        if response.dragged_by(eframe::egui::PointerButton::Primary) {
            if sculpt_active {
                Self::apply_sculpt_dab_at_cursor(session, gpu_context, response, ctx, app);
            } else {
                let delta = response.drag_delta();
                session.camera.orbit(delta.x * 0.01, delta.y * 0.01);
                camera_changed = true;
            }
        }
        if sculpt_active && response.drag_stopped_by(eframe::egui::PointerButton::Primary) {
            if let Some(node_id) = app.paint.selected_sculpt_layer {
                app.end_brush_stroke_on_layer(node_id);
            } else {
                app.end_brush_stroke();
            }
        }

        // RMB always orbits regardless of sculpt mode.
        if response.dragged_by(eframe::egui::PointerButton::Secondary) {
            let delta = response.drag_delta();
            session.camera.orbit(delta.x * 0.01, delta.y * 0.01);
            camera_changed = true;
        }

        // The cursor ring is purely visual — flag a render so the
        // shader picks up the new uniform even when nothing else
        // changed this frame.
        if cursor_world.is_some() || sculpt_active {
            camera_changed = true;
        }

        if response.dragged_by(eframe::egui::PointerButton::Middle) {
            // Pan in the camera-aligned XZ plane: drag right ⇒ camera moves
            // right; drag up/forward ⇒ camera moves into the scene. Speed
            // scales with current distance so the pan feels consistent at
            // any zoom level.
            let delta = response.drag_delta();
            let speed = session.camera.distance * 0.0015;
            session.camera.pan_xz(delta.x * speed, -delta.y * speed);
            camera_changed = true;
        }

        if response.hovered() {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.1 {
                // Multiplicative zoom: ~6% per "tick" of scroll. Smooth at
                // any distance — was previously additive 0.01 which felt
                // gentle far out and explosive close in.
                let factor = (-scroll * 0.0015).clamp(-0.5, 0.5);
                session.camera.zoom(factor);
                camera_changed = true;
            }
        }

        if camera_changed {
            if let (Some(ref mut renderer), Some(ref gpu)) =
                (&mut session.terrain_renderer, gpu_context)
            {
                let elapsed = session.started_at.elapsed().as_secs_f32();
                let frame_borrow = session.current_frame.as_ref().map(|f| f.as_frame(elapsed));
                renderer.render(
                    &gpu.device,
                    &gpu.queue,
                    &session.camera,
                    frame_borrow.as_ref(),
                );
                Self::update_viewport_texture_on(
                    &mut session.viewport_texture_id,
                    &session.terrain_renderer,
                    render_state,
                    ctx,
                );
            }
        }
    }
}

/// Evaluate the graph and extract the data driving the 3D viewport.
///
/// The Preview node is just a node — its executor passes its
/// declared inputs through into the runtime output map under the
/// same names. To render the viewport we look up the active
/// preview node's outputs the same way any consumer reads any
/// node's outputs: `outputs[node_id][port_name]`. There's no
/// "global ingest" pathway that special-cases Preview.
///
/// Returns all-None when:
/// - `preview_node_id` is None, or doesn't point at a Preview node
/// - the named ports aren't present in the node's runtime output
///   (which happens when nothing's wired into them, or when the
///   upstream chain failed to evaluate).
fn eval_preview(
    graph: &bar_graph::GraphEngine,
    executor: &dyn NodeExecutor,
    size: u32,
    preview_node_id: Option<bar_graph::NodeId>,
) -> (Option<bar_data::Heightmap>, Option<bar_data::ColorBuffer>) {
    let result = match evaluate_graph(graph, executor, size, size) {
        Ok(outputs) => outputs,
        Err(_) => return (None, None),
    };

    let Some(pid) = preview_node_id else {
        return (None, None);
    };
    let is_preview = graph
        .get_node(pid)
        .map(|n| n.node_type == bar_graph::NodeType::Preview)
        .unwrap_or(false);
    if !is_preview {
        return (None, None);
    }

    let hm = bar_graph::get_node_output_heightmap_named(&result, pid, "heightmap");
    let tex = bar_graph::get_node_output_color_named(&result, pid, "texture");
    (hm, tex)
}

/// Convert `PlacedFeature` world positions (Spring elmos) to render-space
/// `FeatureInstance` values using the same normalisation as the terrain mesh.
///
/// Spring stores map dimensions in squares (1 square = 8 elmos). Features use
/// elmo coordinates, so `x` ranges over `[0, (w-1)*8]`.
fn build_feature_instances(
    features: &[PlacedFeature],
    w: u32,
    h: u32,
    min_h: f32,
    max_h: f32,
    catalog: Option<&FeatureCatalog>,
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
    // Fallback footprint (2 Spring squares = 16 elmos) used when the catalog
    // doesn't have a definition for a feature type.
    let default_footprint = 2.0_f32;

    features
        .iter()
        .map(|f| {
            let rx = (f.x / (pw * 8.0) - 0.5) * 2.0 * xe;
            let rz = (f.z / (ph * 8.0) - 0.5) * 2.0 * ze;

            // Footprint in Spring squares from the game's FeatureDef; fall back
            // to default_footprint for unknown types.
            let (fp_x, fp_z) = catalog
                .and_then(|cat| cat.features.get(&f.feature_type.to_lowercase()))
                .map(|def| (def.footprint_x.max(1) as f32, def.footprint_z.max(1) as f32))
                .unwrap_or((default_footprint, default_footprint));
            // 1 Spring square = 8 elmos; convert to render-space units (pm squares wide).
            let sx = fp_x / pm;
            let sz = fp_z / pm;
            // Height: approximate with max horizontal dimension (no model data).
            let sy = sx.max(sz);

            // Y position: sample the heightmap at this feature's XZ so the box
            // sits on the actual terrain surface rather than floating or buried.
            // Spring snaps y=0 features to terrain at runtime, so we mirror that.
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

            // Spring angle: degrees CCW from +Z around Y axis. Negate to match
            // render-space handedness.
            let transform = Mat4::from_scale_rotation_translation(
                Vec3::new(sx, sy, sz),
                Quat::from_rotation_y(-f.angle.to_radians()),
                Vec3::new(rx, ry, rz),
            );
            let cols = transform.to_cols_array_2d();
            // Green = recognized type from catalog; orange = unknown placeholder.
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

fn load_icon() -> Option<eframe::egui::IconData> {
    let bytes = include_bytes!("../../../assets/bar-map-editor.png");
    let image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(eframe::egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

mod app_log_layer;
mod bar_install;
mod layout_manager;
mod runner;
mod viewport;

use std::sync::mpsc;

use anyhow::Result;
use bar_compute::GpuContext;
use tracing::Level;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

use runner::{make_executor, AppRunner};

fn main() -> Result<()> {
    let (log_tx, log_rx) = mpsc::channel::<(Level, String)>();
    // BME log panel filter: DEBUG so the panel's "DBG" visibility toggle
    // has something to surface. The panel hides DEBUG events by default;
    // the user enables them with the per-level button. Stdout filter
    // stays at INFO (or RUST_LOG override) to avoid debug-spam in the
    // terminal.
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        ))
        .with(
            app_log_layer::AppLogLayer::new(log_tx)
                .with_filter(tracing_subscriber::filter::LevelFilter::DEBUG),
        )
        .init();

    tracing::info!("Starting BAR - Map Editor");

    bar_gui::i18n::init();

    bar_engine::prune_old_work_dirs(std::time::Duration::from_secs(60 * 60 * 24 * 14));

    let icon = load_icon();

    // Window-state restoration on Windows: ViewportBuilder's with_maximized is
    // advisory; we send an explicit ViewportCommand::Maximized on first frame.
    let saved_settings = bar_gui::Settings::load();
    let (default_pos, default_size, default_maximized) = match saved_settings.window.as_ref() {
        Some(w) => (Some([w.x, w.y]), [w.width, w.height], w.maximized),
        None => (None, [1440.0, 900.0], true),
    };

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

    // Request BC texture compression + large max texture dimension so native-
    // resolution SMT textures (e.g. 12288px for Supreme Isthmus) can be uploaded.
    let wgpu_setup =
        eframe::egui_wgpu::WgpuSetup::CreateNew(eframe::egui_wgpu::WgpuSetupCreateNew {
            device_descriptor: std::sync::Arc::new(|adapter| {
                let bc = if adapter
                    .features()
                    .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
                {
                    wgpu::Features::TEXTURE_COMPRESSION_BC
                } else {
                    wgpu::Features::empty()
                };
                let base_limits = if adapter.get_info().backend == wgpu::Backend::Gl {
                    wgpu::Limits::downlevel_webgl2_defaults()
                } else {
                    wgpu::Limits::default()
                };
                wgpu::DeviceDescriptor {
                    label: Some("bar-editor"),
                    required_features: bc,
                    required_limits: wgpu::Limits {
                        max_texture_dimension_2d: adapter.limits().max_texture_dimension_2d,
                        max_storage_buffer_binding_size: 512 * 1024 * 1024,
                        max_buffer_size: 512 * 1024 * 1024,
                        // Terrain pipeline now uses 5 bind groups (camera +
                        // textures + water_planes + heightmap + shadow). Bump
                        // the limit accordingly; both desktop GL and modern
                        // backends support 8 groups, so 5 is well within range.
                        max_bind_groups: 8.min(adapter.limits().max_bind_groups),
                        ..base_limits
                    },
                    ..Default::default()
                }
            }),
            ..Default::default()
        });
    let wgpu_options = eframe::egui_wgpu::WgpuConfiguration {
        wgpu_setup,
        ..Default::default()
    };
    let options = eframe::NativeOptions {
        viewport,
        wgpu_options,
        ..Default::default()
    };

    eframe::run_native(
        "BAR - Map Editor",
        options,
        Box::new(move |cc| {
            let mut app = bar_gui::BarEditorApp::new(cc);

            let render_state = cc.wgpu_render_state.clone();
            let gpu_context = render_state
                .as_ref()
                .map(|rs| GpuContext::from_existing(rs.device.clone(), rs.queue.clone()));

            app.supports_bc = gpu_context.as_ref().map(|c| c.supports_bc).unwrap_or(false);

            let executor = make_executor(&gpu_context);

            let bar_install = bar_install::BarVersions::detect();
            if let Some(ref versions) = bar_install {
                app.bar_versions.game_labels =
                    versions.games.iter().map(|g| g.label.clone()).collect();
                app.bar_versions.engine_labels =
                    versions.engines.iter().map(|e| e.label.clone()).collect();
            }

            if saved_settings.selected_game_archive.is_none() {
                if let Some(path) = bar_install
                    .as_ref()
                    .and_then(|v| v.games.iter().find_map(|g| g.path.clone()))
                {
                    app.set_game_archive(path);
                }
            }

            Ok(Box::new(AppRunner {
                app,
                executor,
                gpu_context,
                render_state,
                export_result_rx: None,
                progress_rx: None,
                export_status: bar_gui::ExportStatus::Idle,
                sd7_extract_rx: None,
                compile_result_rx: None,
                test_in_bar_rx: None,
                pending_export_dir: None,
                bar_install,
                layout_manager: layout_manager::LayoutManager::new(),
                pending_maximize: default_maximized,
                has_shown_window: false,
                feature_catalog: None,
                catalog_rx: None,
                catalog_archive_path: None,
                map_work_dir: None,
                model_rx: None,
                log_rx,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))?;

    Ok(())
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

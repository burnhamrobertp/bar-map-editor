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
    /// Map's vertical elmo span (max_h - min_h). Needed by the renderer so
    /// the underwater-absorption shader can convert render-Y back to elmos.
    pub height_range_elmos: f32,
    /// Elmos per unit of render-space XZ. See
    /// `bar_render::TerrainUpdateParams::elmo_per_render_xz`.
    pub elmo_per_render_xz: [f32; 2],
    pub water_y: f32,
    pub water_color: [f32; 3],
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
    /// Map's vertical elmo span; see [`PreviewResult::height_range_elmos`].
    pub height_range_elmos: f32,
    /// Elmos per unit of render-space XZ.
    pub elmo_per_render_xz: [f32; 2],
    pub x_extent: f32,
    pub z_extent: f32,
    pub water_y: f32,
    pub water_color: [f32; 3],
    pub quality_high: bool,
    /// `smf_lighting` is no longer stored in OwnedFrame -- it's read live from
    /// `app.smf_lighting()` on every render call so map-settings edits (water
    /// colour, fresnel, etc.) take effect immediately without needing a graph
    /// re-evaluation to bump the cache key. See `as_frame(time, smf)`.
    /// Texture resolution this frame was evaluated at.
    pub tex_w: u32,
    pub tex_h: u32,
}

impl OwnedFrame {
    /// Build a renderer-side `PreviewFrame`. `smf_lighting` is supplied
    /// per-render-call (from `app.smf_lighting()`) so editing the map-info
    /// water / lighting panel updates the GPU uniforms immediately.
    pub fn as_frame(
        &self,
        time: f32,
        smf_lighting: bar_render::SmfLighting,
    ) -> bar_render::PreviewFrame {
        bar_render::PreviewFrame {
            height_scale: self.height_scale,
            x_extent: self.x_extent,
            z_extent: self.z_extent,
            water_y: self.water_y,
            water_color: self.water_color,
            quality_high: self.quality_high,
            time,
            smf_lighting,
            height_range_elmos: self.height_range_elmos,
            elmo_per_render_xz: self.elmo_per_render_xz,
        }
    }
}

/// Read the current map-settings water / lighting block as a `SmfLighting`
/// suitable for `OwnedFrame::as_frame`. `bar_gui::SmfLightingSnapshot` is
/// a type alias for `bar_render::SmfLighting`, so this is just a forward
/// of `app.smf_lighting()` -- kept as a function so existing callers (and
/// the documentation about why we re-read every frame instead of caching
/// on OwnedFrame) stay valid. Calling on every render call closes the
/// data-flow gap that made the mapinfo-editor water panel a no-op
/// (changes used to be lost because they didn't bump the eval cache key).
pub fn live_smf_lighting(app: &bar_gui::BarEditorApp) -> bar_render::SmfLighting {
    app.smf_lighting()
}

// ── Per-slot state types ──────────────────────────────────────────────────────

/// Rendering state shared by Sculpt3D and Preview slots.
/// Tracks an in-progress drag-to-move gesture on a placed feature. Set
/// when the user presses primary mouse over the already-selected feature
/// and the drag threshold is crossed; cleared on release. The offsets
/// capture the difference between the press point's terrain pick and the
/// feature's recorded XZ so the feature follows the cursor naturally
/// (the press point doesn't have to be the feature's exact base).
#[derive(Clone, Copy)]
pub struct FeatureDragState {
    pub feature_idx: usize,
    pub offset_x: f32,
    pub offset_z: f32,
}

pub struct ViewportCore {
    pub camera: Camera,
    pub terrain_renderer: Option<TerrainRenderer>,
    pub viewport_texture_id: Option<egui::TextureId>,
    pub current_frame: Option<OwnedFrame>,
    pub last_water_y: f32,
    pub last_water_color: [f32; 3],
    pub session_id: u64,
    pub started_at: Instant,
    pub feature_drag: Option<FeatureDragState>,
    /// Timestamp of the most recent rotation-gesture mutation. Used to
    /// coalesce a continuous wheel-rotation flurry into a single undo
    /// entry: the first event after a quiet gap (>= `ROTATE_GESTURE_GAP`)
    /// snapshots state, subsequent events within the window mutate
    /// without pushing.
    pub last_rotate_at: Option<Instant>,
    /// Tracks the `(project_dir, skybox_filename)` the renderer's cubemap
    /// was last loaded for. We re-attempt the upload whenever either side
    /// of this tuple changes -- not gated on compilation, so the skybox
    /// shows up the moment a project opens regardless of compile state.
    /// `None` means "not yet attempted for any project".
    pub skybox_loaded_for: Option<(std::path::PathBuf, String)>,
    /// Same idea as `skybox_loaded_for`, but for the detail texture
    /// from mapinfo's `resources.detailTex`.
    pub detail_loaded_for: Option<(std::path::PathBuf, String)>,
    /// Tracks `(project_dir, [splat_dn_1..4, splat_distr])`. Cleared
    /// on project switch so the splat detail upload re-runs.
    pub splat_loaded_for: Option<(std::path::PathBuf, [String; 5])>,
    /// Same idea as `skybox_loaded_for` for the sky reflection mask.
    pub sky_reflect_mod_loaded_for: Option<(std::path::PathBuf, String)>,
    /// Same idea as `skybox_loaded_for` for the per-pixel specular texture.
    pub specular_tex_loaded_for: Option<(std::path::PathBuf, String)>,
    /// Key currently being decoded on a background thread for each of the
    /// async-loaded texture slots. Set by the sync function when it spawns
    /// a worker; cleared on `poll_pending_texture_loads` once the result
    /// arrives. Used to dedupe: the same key won't fire two concurrent
    /// loads while one is still in flight.
    pub skybox_loading_for: Option<(std::path::PathBuf, String)>,
    pub detail_loading_for: Option<(std::path::PathBuf, String)>,
    pub splat_loading_for: Option<(std::path::PathBuf, [String; 5])>,
    pub sky_reflect_mod_loading_for: Option<(std::path::PathBuf, String)>,
    pub specular_tex_loading_for: Option<(std::path::PathBuf, String)>,
    /// Background-decoded texture payloads ready to be uploaded on the
    /// main thread. Drained by `poll_pending_texture_loads` each frame.
    pub texture_load_tx: mpsc::Sender<TextureLoadResult>,
    pub texture_load_rx: mpsc::Receiver<TextureLoadResult>,
}

/// Single mip level: `(rgba_bytes, width, height)`.
pub type Mip = (Vec<u8>, u32, u32);
/// Mip pyramid for one 2D texture.
pub type MipChain = Vec<Mip>;
/// Splat textures arrive as four detail-normal channels + one
/// distribution texture, each with its own pre-baked mip pyramid.
pub type SplatChains = [MipChain; 5];

/// Texture payloads produced by background workers spawned in the sync
/// functions. Carries the key the load was scheduled for so the receiver
/// can mark the slot loaded; `data` is `None` for failures (file not
/// found / decode failed). Setting `loaded_for = key` on failure
/// suppresses retry storms.
pub enum TextureLoadResult {
    Skybox {
        key: (std::path::PathBuf, String),
        data: Option<bar_data::Cubemap>,
    },
    Detail {
        key: (std::path::PathBuf, String),
        data: Option<Mip>,
    },
    Splat {
        key: (std::path::PathBuf, [String; 5]),
        data: Option<SplatChains>,
    },
    SkyReflectMod {
        key: (std::path::PathBuf, String),
        data: Option<Mip>,
    },
    SpecularTex {
        key: (std::path::PathBuf, String),
        data: Option<Mip>,
    },
}

impl ViewportCore {
    pub fn new(gpu_context: &Option<GpuContext>, session_id: u64) -> Self {
        let terrain_renderer = gpu_context.as_ref().map(|ctx| {
            let mut r =
                TerrainRenderer::new(&ctx.device, &ctx.queue, wgpu::TextureFormat::Rgba8UnormSrgb);
            r.resize(&ctx.device, 512, 512);
            r
        });
        let (texture_load_tx, texture_load_rx) = mpsc::channel();
        Self {
            camera: Camera::default(),
            terrain_renderer,
            viewport_texture_id: None,
            current_frame: None,
            last_water_y: -1.0,
            last_water_color: [0.0, 0.4, 0.6],
            session_id,
            started_at: Instant::now(),
            feature_drag: None,
            last_rotate_at: None,
            skybox_loaded_for: None,
            detail_loaded_for: None,
            splat_loaded_for: None,
            sky_reflect_mod_loaded_for: None,
            specular_tex_loaded_for: None,
            skybox_loading_for: None,
            detail_loading_for: None,
            splat_loading_for: None,
            sky_reflect_mod_loading_for: None,
            specular_tex_loading_for: None,
            texture_load_tx,
            texture_load_rx,
        }
    }
}

/// Decode a 2D image file into RGBA8 bytes. Handles the formats BAR
/// maps actually ship: BMP, TGA, PNG, JPG via the `image` crate, plus
/// DDS (uncompressed and BC1/3) by reusing the cubemap decoder for
/// its 2D fallback path.
fn load_2d_image(path: &std::path::Path) -> Option<(Vec<u8>, u32, u32)> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if ext == "dds" {
        // Try the dedicated 2D DDS loader first -- it handles the
        // BC1 / BC3 / uncompressed pixel formats BAR ships for splat
        // distribution, splat detail-normals, sky reflection mask, and
        // map detail textures. The `image` crate v0.25 no longer
        // includes DDS support so without this path those files don't
        // decode at all.
        if let Ok((rgba, w, h)) = bar_data::load_dds_2d(path) {
            return Some((rgba, w, h));
        }
        // Cubemap-flagged DDS: extract face 0 as a 2D image. Some legacy
        // BAR maps mislabel 2D textures with the CUBEMAP cap; falling
        // back to the cubemap loader preserves that path too.
        if let Ok(cm) = bar_data::load_dds_cubemap(path) {
            return Some((cm.faces[0].clone(), cm.width, cm.height));
        }
        // Final fallback to the `image` crate -- catches any DDS variant
        // outside our supported pixel formats if the user happens to have
        // an `image` build with the optional `dds` feature enabled.
    }
    let bytes = std::fs::read(path).ok()?;
    let fmt = image::ImageFormat::from_extension(&ext)?;
    let img = image::load_from_memory_with_format(&bytes, fmt).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Some((rgba.into_raw(), w, h))
}

/// Drain any texture-load results that arrived from background workers
/// and upload them to the GPU. Call once per frame from the layout
/// manager before the sync_* calls. On failure the slot is still
/// marked `loaded_for = key` so we don't retry until the key changes.
pub fn poll_pending_texture_loads(core: &mut ViewportCore, gpu: &GpuContext) {
    while let Ok(result) = core.texture_load_rx.try_recv() {
        match result {
            TextureLoadResult::Skybox { key, data } => {
                core.skybox_loading_for = None;
                core.skybox_loaded_for = Some(key);
                if let (Some(cm), Some(renderer)) = (data, core.terrain_renderer.as_mut()) {
                    renderer.update_skybox(&gpu.device, &gpu.queue, &cm);
                    tracing::info!(w = cm.width, h = cm.height, "Skybox cubemap loaded");
                }
            }
            TextureLoadResult::Detail { key, data } => {
                core.detail_loading_for = None;
                core.detail_loaded_for = Some(key);
                if let (Some((rgba, w, h)), Some(renderer)) = (data, core.terrain_renderer.as_mut())
                {
                    renderer.update_detail_texture(&gpu.device, &gpu.queue, &rgba, w, h);
                    tracing::info!(w, h, "Detail texture loaded");
                }
            }
            TextureLoadResult::Splat { key, data } => {
                core.splat_loading_for = None;
                core.splat_loaded_for = Some(key);
                match (data, core.terrain_renderer.as_mut()) {
                    (Some(arr), Some(renderer)) => {
                        renderer.update_splat_textures(&gpu.device, &gpu.queue, arr);
                        tracing::info!("Splat detail textures loaded (with mip chains)");
                    }
                    (None, Some(renderer)) => {
                        renderer.clear_splat_textures(&gpu.device, &gpu.queue);
                    }
                    _ => {}
                }
            }
            TextureLoadResult::SkyReflectMod { key, data } => {
                core.sky_reflect_mod_loading_for = None;
                core.sky_reflect_mod_loaded_for = Some(key);
                match (data, core.terrain_renderer.as_mut()) {
                    (Some((rgba, w, h)), Some(renderer)) => {
                        renderer.update_sky_reflect_mod(&gpu.device, &gpu.queue, &rgba, w, h);
                        tracing::info!(w, h, "skyReflectModTex loaded");
                    }
                    (None, Some(renderer)) => {
                        renderer.clear_sky_reflect_mod(&gpu.device, &gpu.queue);
                    }
                    _ => {}
                }
            }
            TextureLoadResult::SpecularTex { key, data } => {
                core.specular_tex_loading_for = None;
                core.specular_tex_loaded_for = Some(key);
                match (data, core.terrain_renderer.as_mut()) {
                    (Some((rgba, w, h)), Some(renderer)) => {
                        renderer.update_specular_tex(&gpu.device, &gpu.queue, &rgba, w, h);
                        tracing::info!(w, h, "specularTex loaded");
                    }
                    (None, Some(renderer)) => {
                        renderer.clear_specular_tex(&gpu.device, &gpu.queue);
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Schedule a background load of the map-authored sky reflection mask
/// texture (mapinfo `resources.skyReflectModTex`). The decode runs off-
/// thread; `poll_pending_texture_loads` performs the GPU upload when
/// the worker finishes. Idempotent: returns immediately if the key is
/// already loaded or has a load in flight.
pub fn sync_sky_reflect_mod(
    project_dir: Option<&std::path::Path>,
    filename: &str,
    core: &mut ViewportCore,
    gpu: &GpuContext,
) {
    let key = project_dir.map(|p| (p.to_path_buf(), filename.to_string()));
    if core.sky_reflect_mod_loaded_for == key || core.sky_reflect_mod_loading_for == key {
        return;
    }
    if filename.is_empty() {
        if let Some(renderer) = core.terrain_renderer.as_mut() {
            renderer.clear_sky_reflect_mod(&gpu.device, &gpu.queue);
        }
        core.sky_reflect_mod_loaded_for = key;
        return;
    }
    let Some(project_dir) = project_dir else {
        return;
    };
    let key_pair = (project_dir.to_path_buf(), filename.to_string());
    core.sky_reflect_mod_loading_for = Some(key_pair.clone());
    let tx = core.texture_load_tx.clone();
    std::thread::spawn(move || {
        let (project_dir, filename) = key_pair.clone();
        let path = find_file_in_dir(&project_dir.join("passthrough"), &filename)
            .or_else(|| find_file_in_dir(&project_dir, &filename));
        let data = match path {
            Some(p) => load_2d_image(&p).or_else(|| {
                tracing::warn!(file = %filename, "Failed to decode skyReflectModTex");
                None
            }),
            None => {
                tracing::warn!(file = %filename, "skyReflectModTex not found in project");
                None
            }
        };
        let _ = tx.send(TextureLoadResult::SkyReflectMod {
            key: key_pair,
            data,
        });
    });
}

/// Schedule a background load of the map-authored specular texture
/// (mapinfo `resources.specularTex`, engine path
/// `SMF_SPECULAR_LIGHTING`). Decode runs off-thread; upload happens in
/// `poll_pending_texture_loads`.
pub fn sync_specular_tex(
    project_dir: Option<&std::path::Path>,
    filename: &str,
    core: &mut ViewportCore,
    gpu: &GpuContext,
) {
    let key = project_dir.map(|p| (p.to_path_buf(), filename.to_string()));
    if core.specular_tex_loaded_for == key || core.specular_tex_loading_for == key {
        return;
    }
    if filename.is_empty() {
        if let Some(renderer) = core.terrain_renderer.as_mut() {
            renderer.clear_specular_tex(&gpu.device, &gpu.queue);
        }
        core.specular_tex_loaded_for = key;
        return;
    }
    let Some(project_dir) = project_dir else {
        return;
    };
    let key_pair = (project_dir.to_path_buf(), filename.to_string());
    core.specular_tex_loading_for = Some(key_pair.clone());
    let tx = core.texture_load_tx.clone();
    std::thread::spawn(move || {
        let (project_dir, filename) = key_pair.clone();
        let path = find_file_in_dir(&project_dir.join("passthrough"), &filename)
            .or_else(|| find_file_in_dir(&project_dir, &filename));
        let data = match path {
            Some(p) => load_2d_image(&p).or_else(|| {
                tracing::warn!(file = %filename, "Failed to decode specularTex");
                None
            }),
            None => {
                tracing::warn!(file = %filename, "specularTex not found in project");
                None
            }
        };
        let _ = tx.send(TextureLoadResult::SpecularTex {
            key: key_pair,
            data,
        });
    });
}

/// Ensure the renderer's splat-detail textures match the current
/// project. Pulls the five filenames from `MapSettings.resources`,
/// resolves them under `<project>/passthrough/`, decodes via
/// `load_2d_image`, and uploads. Only fires when ALL five textures
/// resolve -- otherwise the renderer's `advanced_splat_enabled` stays
/// off and the playable area renders without splat detail. Tracked by
/// `splat_loaded_for` on `ViewportCore` so a successful load isn't
/// retried each frame, and a different project triggers re-load.
pub fn sync_splat_textures(
    project_dir: Option<&std::path::Path>,
    settings: &bar_project::ResourcesSettings,
    core: &mut ViewportCore,
    gpu: &GpuContext,
) {
    let names = [
        settings.splat_detail_normal_tex_1.clone(),
        settings.splat_detail_normal_tex_2.clone(),
        settings.splat_detail_normal_tex_3.clone(),
        settings.splat_detail_normal_tex_4.clone(),
        settings.splat_distr_tex.clone(),
    ];
    let key = project_dir.map(|p| (p.to_path_buf(), names.clone()));
    if core.splat_loaded_for == key || core.splat_loading_for == key {
        return;
    }
    let Some(project_dir) = project_dir else {
        return;
    };

    let distr_name = &names[4];
    if distr_name.is_empty() {
        if let Some(renderer) = core.terrain_renderer.as_mut() {
            renderer.clear_splat_textures(&gpu.device, &gpu.queue);
        }
        core.splat_loaded_for = key;
        return;
    }

    let key_pair = (project_dir.to_path_buf(), names);
    core.splat_loading_for = Some(key_pair.clone());
    let tx = core.texture_load_tx.clone();
    std::thread::spawn(move || {
        let (project_dir, names) = key_pair.clone();
        let resolve_and_decode = |name: &str| -> Option<Vec<(Vec<u8>, u32, u32)>> {
            if name.is_empty() {
                return None;
            }
            let path = find_file_in_dir(&project_dir.join("passthrough"), name)
                .or_else(|| find_file_in_dir(&project_dir, name))?;
            load_2d_image_with_mips(&path)
        };

        // Distribution texture is required; without it the whole splat
        // path is disabled. The 4 detail-normals are independently
        // optional -- missing ones get a 1x1 mid-grey mip so the
        // shader's `*2-1` decode produces zero contribution.
        let distr = match resolve_and_decode(&names[4]) {
            Some(m) => m,
            None => {
                tracing::warn!(
                    file = %names[4],
                    "Splat distribution texture missing or failed to decode; advanced splat disabled"
                );
                let _ = tx.send(TextureLoadResult::Splat {
                    key: key_pair,
                    data: None,
                });
                return;
            }
        };

        type MipChain = Vec<(Vec<u8>, u32, u32)>;
        let default_mip = || -> MipChain { vec![(vec![127u8, 127, 127, 127], 1, 1)] };
        let mut dn: [Option<MipChain>; 4] = [None, None, None, None];
        for (i, name) in names[..4].iter().enumerate() {
            match resolve_and_decode(name) {
                Some(m) => dn[i] = Some(m),
                None if name.is_empty() => {}
                None => {
                    tracing::warn!(
                        file = %name,
                        slot = i + 1,
                        "Splat-detail texture missing; channel will be inactive"
                    );
                }
            }
        }
        let arr: [MipChain; 5] = [
            dn[0].take().unwrap_or_else(default_mip),
            dn[1].take().unwrap_or_else(default_mip),
            dn[2].take().unwrap_or_else(default_mip),
            dn[3].take().unwrap_or_else(default_mip),
            distr,
        ];
        let _ = tx.send(TextureLoadResult::Splat {
            key: key_pair,
            data: Some(arr),
        });
    });
}

/// Decode a 2D image file and return ALL mip levels as `(rgba, w, h)`.
/// For DDS sources this is the file's pre-baked mip chain; for other
/// formats it's a single-entry chain (caller can extend via box filter
/// if it needs the higher pyramid levels).
fn load_2d_image_with_mips(path: &std::path::Path) -> Option<Vec<(Vec<u8>, u32, u32)>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if ext == "dds" {
        if let Ok(mips) = bar_data::load_dds_2d_with_mips(path) {
            return Some(
                mips.into_iter()
                    .map(|m| (m.rgba, m.width, m.height))
                    .collect(),
            );
        }
        if let Ok(cm) = bar_data::load_dds_cubemap(path) {
            // Mislabeled 2D-as-cubemap fallback. Single mip only -- the
            // cubemap loader doesn't expose the per-face mip chain.
            return Some(vec![(cm.faces[0].clone(), cm.width, cm.height)]);
        }
    }
    // Non-DDS or DDS that fell through: produce a single-mip chain so the
    // caller can decide whether to synthesise additional levels.
    let (rgba, w, h) = load_2d_image(path)?;
    Some(vec![(rgba, w, h)])
}

/// Ensure the renderer's detail texture matches the current project's
/// `resources.detailTex`. Idempotent: tracks `(project_dir, filename)`
/// on the core so re-uploads only fire on project / filename change.
pub fn sync_detail_texture(
    project_dir: Option<&std::path::Path>,
    detail_filename: &str,
    core: &mut ViewportCore,
    gpu: &GpuContext,
) {
    let key = project_dir.map(|p| (p.to_path_buf(), detail_filename.to_string()));
    if core.detail_loaded_for == key || core.detail_loading_for == key {
        return;
    }
    if detail_filename.is_empty() {
        // Reset to the 1x1 grey default so the contribution goes to zero.
        if let Some(renderer) = core.terrain_renderer.as_mut() {
            let mid_grey = [128u8, 128, 128, 255];
            renderer.update_detail_texture(&gpu.device, &gpu.queue, &mid_grey, 1, 1);
        }
        core.detail_loaded_for = key;
        return;
    }
    let Some(project_dir) = project_dir else {
        return;
    };
    let key_pair = (project_dir.to_path_buf(), detail_filename.to_string());
    core.detail_loading_for = Some(key_pair.clone());
    let tx = core.texture_load_tx.clone();
    std::thread::spawn(move || {
        let (project_dir, filename) = key_pair.clone();
        let path = find_file_in_dir(&project_dir.join("passthrough"), &filename)
            .or_else(|| find_file_in_dir(&project_dir, &filename));
        let data = match path {
            Some(p) => load_2d_image(&p).or_else(|| {
                tracing::warn!(file = %filename, "Failed to decode detail texture");
                None
            }),
            None => {
                tracing::warn!(file = %filename, "Detail texture not found in project");
                None
            }
        };
        let _ = tx.send(TextureLoadResult::Detail {
            key: key_pair,
            data,
        });
    });
}

/// Ensure the renderer's cubemap matches the current project's
/// `atmosphere.skyBox`. Idempotent: tracks `(project_dir, filename)` on
/// the core so the upload only happens on project changes. Decoupled
/// from the BC1 compile path so the skybox renders even before / without
/// a compile.
pub fn sync_skybox(
    project_dir: Option<&std::path::Path>,
    skybox_filename: &str,
    core: &mut ViewportCore,
    gpu: &GpuContext,
) {
    let key = project_dir.map(|p| (p.to_path_buf(), skybox_filename.to_string()));
    if core.skybox_loaded_for == key || core.skybox_loading_for == key {
        return;
    }
    if skybox_filename.is_empty() {
        // Map switched to one without a skybox -- clear the cubemap so
        // the procedural sky kicks back in.
        if let Some(renderer) = core.terrain_renderer.as_mut() {
            renderer.clear_skybox(&gpu.device, &gpu.queue);
        }
        core.skybox_loaded_for = key;
        return;
    }
    let Some(project_dir) = project_dir else {
        return;
    };
    let key_pair = (project_dir.to_path_buf(), skybox_filename.to_string());
    core.skybox_loading_for = Some(key_pair.clone());
    let tx = core.texture_load_tx.clone();
    std::thread::spawn(move || {
        let (project_dir, filename) = key_pair.clone();
        let path = find_file_in_dir(&project_dir.join("passthrough"), &filename)
            .or_else(|| find_file_in_dir(&project_dir, &filename));
        let data = match path {
            Some(p) => match bar_data::load_dds_cubemap(&p) {
                Ok(c) => Some(c),
                Err(e) => {
                    tracing::warn!(file = %filename, err = %e, "Failed to decode skybox DDS");
                    None
                }
            },
            None => {
                tracing::warn!(file = %filename, "Skybox DDS not found in project");
                None
            }
        };
        let _ = tx.send(TextureLoadResult::Skybox {
            key: key_pair,
            data,
        });
    });
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
    /// `app.paint.heightmap_rev` value when feature instances were last built.
    /// `u64::MAX` means never built. Rebuilt whenever the rev advances so
    /// instances track terrain height after the first eval completes.
    pub last_hm_rev: u64,
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
            last_hm_rev: u64::MAX,
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
        draw_viewport_debug_overlay(ui, &response.rect, &core.camera, app);

        handle_camera_input(core, gpu_context, render_state, &response, ctx, app);

        // Floating feature popover -- anchors next to the selected
        // feature in the viewport when one is selected. Hidden when
        // the feature is offscreen or behind the camera. Projection
        // is computed first (immutable borrows of app); the draw call
        // takes &mut app for the delete-button path, so the borrows
        // are split intentionally.
        if let Some(frame) = core.current_frame.as_ref() {
            let aspect = response.rect.width().max(1.0) / response.rect.height().max(1.0);
            let view_projection = core.camera.view_projection(aspect);
            let dims = bar_gui::panels::feature_popover::PopoverDims {
                map_w: app.map.width,
                map_h: app.map.height,
                min_height: app.map.min_height,
                max_height: app.map.max_height,
                x_extent: frame.x_extent,
                z_extent: frame.z_extent,
                height_scale: frame.height_scale,
            };
            let anchor = app
                .map
                .selected_feature_idx
                .and_then(|idx| app.map.features.get(idx))
                .and_then(|feature| {
                    bar_gui::panels::feature_popover::project_feature_to_screen(
                        feature,
                        &dims,
                        app.paint.heightmap.as_ref(),
                        view_projection,
                        response.rect,
                    )
                });
            if let Some(anchor) = anchor {
                bar_gui::panels::feature_popover::draw(ctx, app, anchor);
            }
        }
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

fn draw_resolution_badge(ui: &mut egui::Ui, viewport_rect: &egui::Rect, res: &ResolutionStatus) {
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

    let font = egui::FontId::monospace(11.0);
    let text_color = if target_dims.is_some() {
        egui::Color32::from_rgba_unmultiplied(255, 200, 80, 230)
    } else {
        egui::Color32::from_rgba_unmultiplied(160, 200, 160, 200)
    };

    let galley = ui.painter().layout_no_wrap(label, font, text_color);
    let padding = egui::vec2(6.0, 3.0);
    let badge_size = galley.size() + padding * 2.0;
    let badge_pos =
        viewport_rect.right_bottom() - egui::vec2(badge_size.x + 8.0, badge_size.y + 8.0);
    let badge_rect = egui::Rect::from_min_size(badge_pos, badge_size);

    ui.painter().rect_filled(
        badge_rect,
        5.0,
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 160),
    );
    ui.painter().galley(badge_pos + padding, galley, text_color);

    // Loading affordance: when the high-res pass is still working,
    // wrap the badge with the same travelling-glow outline used on
    // the Compile button so it reads as an active state rather than
    // a static label.
    if res.high_pending {
        bar_gui::layouts::preview::draw_animated_border(ui, badge_rect);
    }
}

/// Draw the bottom-left gear button + any active debug overlays
/// (camera readout etc.). Clicking the gear opens a small menu of
/// toggles stored on `app.viewport_debug`. The gear stays out of the
/// way until clicked; when toggles are active their overlays draw
/// over the viewport image.
fn draw_viewport_debug_overlay(
    ui: &mut egui::Ui,
    viewport_rect: &egui::Rect,
    camera: &bar_render::Camera,
    app: &mut bar_gui::BarEditorApp,
) {
    let gear_size = egui::vec2(20.0, 20.0);
    let gear_pos = viewport_rect.left_bottom() + egui::vec2(8.0, -gear_size.y - 8.0);
    let gear_rect = egui::Rect::from_min_size(gear_pos, gear_size);

    let response = ui.interact(
        gear_rect,
        egui::Id::new("viewport_debug_gear"),
        egui::Sense::click(),
    );
    let gear_color = if response.hovered() {
        egui::Color32::from_rgba_unmultiplied(220, 220, 240, 230)
    } else {
        egui::Color32::from_rgba_unmultiplied(160, 160, 180, 200)
    };
    ui.painter().rect_filled(
        gear_rect,
        4.0,
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 160),
    );
    ui.painter().text(
        gear_rect.center(),
        egui::Align2::CENTER_CENTER,
        "*",
        egui::FontId::monospace(14.0),
        gear_color,
    );

    let popup_id = egui::Id::new("viewport_debug_menu");
    if response.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    egui::popup::popup_below_widget(
        ui,
        popup_id,
        &response,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(180.0);
            ui.checkbox(
                &mut app.viewport_debug.show_camera_readout,
                "Camera readout",
            );
        },
    );

    if app.viewport_debug.show_camera_readout {
        let pos = camera.position();
        let label = format!(
            "cam pos  {:>8.3} {:>8.3} {:>8.3}\n     az  {:>7.2} deg\n     el  {:>7.2} deg\n     d   {:>8.3}",
            pos.x,
            pos.y,
            pos.z,
            camera.azimuth.to_degrees(),
            camera.elevation.to_degrees(),
            camera.distance,
        );
        let font = egui::FontId::monospace(11.0);
        let text_color = egui::Color32::from_rgba_unmultiplied(200, 220, 200, 230);
        let galley = ui.painter().layout_no_wrap(label, font, text_color);
        let padding = egui::vec2(6.0, 4.0);
        let size = galley.size() + padding * 2.0;
        // Anchor above the gear so the two don't overlap.
        let pos = egui::pos2(viewport_rect.left() + 8.0, gear_pos.y - size.y - 6.0);
        let bg = egui::Rect::from_min_size(pos, size);
        ui.painter()
            .rect_filled(bg, 4.0, egui::Color32::from_rgba_unmultiplied(0, 0, 0, 160));
        ui.painter().galley(pos + padding, galley, text_color);
    }
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
    // Snapshot camera state before any per-frame mutation so we can
    // revert at the end if the net effect would have pushed the
    // camera below the terrain. Reverting (rather than clamping
    // target.y to a floor) is what prevents the camera from sliding
    // along the terrain surface when the user keeps orbiting against
    // the constraint: each frame's mutation is dropped wholesale, so
    // the camera freezes at the last valid orientation.
    let pre_camera = core.camera.clone();
    let mut camera_changed = false;
    let sculpt_active = app.sculpt_input_active();

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

    // Feature interaction: only when the Pointer tool is active and not in the read-only Preview layout.
    let feature_type = app.selected_feature_type.clone();
    if app.paint.brush.tool == bar_gui::BrushTool::Pointer
        && app.active_layout() != bar_gui::Layout::Preview
    {
        // Start a drag-to-move gesture when the press lands on the
        // currently-selected feature. The check runs at drag-start
        // time so a click that doesn't move (`clicked_by` below)
        // still goes through the selection path.
        if response.drag_started_by(egui::PointerButton::Primary)
            && feature_type.is_none()
            && !sculpt_active
        {
            if let (Some(uv), Some(sel_idx)) = (cursor_uv, app.map.selected_feature_idx) {
                if let Some(renderer) = core.terrain_renderer.as_ref() {
                    let pickable = build_pickable_features(
                        &app.map.features,
                        &FeatureMapDims {
                            w: app.map.width,
                            h: app.map.height,
                            min_h: app.map.min_height,
                            max_h: app.map.max_height,
                        },
                        app.paint.heightmap.as_ref(),
                        renderer,
                    );
                    let hit = bar_render::pick_feature(&core.camera, aspect, uv, &pickable);
                    if hit == Some(sel_idx) {
                        // Capture press-point offset so the feature follows
                        // the cursor rather than snapping its base to the
                        // cursor projection on each frame.
                        if let Some(hm) = app.paint.heightmap.as_ref() {
                            let (height_scale, x_extent, z_extent) = renderer.mesh_extents();
                            if let Some(pick) = pick_terrain(
                                &core.camera,
                                aspect,
                                uv,
                                hm,
                                x_extent,
                                z_extent,
                                height_scale,
                            ) {
                                let (map_w, map_h) = app.map.dimensions();
                                let press_spring_x = (pick.world.x / x_extent + 1.0)
                                    * 0.5
                                    * map_w.max(1) as f32
                                    * 8.0;
                                let press_spring_z = (pick.world.z / z_extent + 1.0)
                                    * 0.5
                                    * map_h.max(1) as f32
                                    * 8.0;
                                if let Some((fx, fz)) =
                                    app.map.features.get(sel_idx).map(|f| (f.x, f.z))
                                {
                                    app.push_undo("Move feature");
                                    core.feature_drag = Some(FeatureDragState {
                                        feature_idx: sel_idx,
                                        offset_x: fx - press_spring_x,
                                        offset_z: fz - press_spring_z,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        if response.clicked_by(egui::PointerButton::Primary) {
            if let Some(ref feature_type) = feature_type {
                // Placement mode: place a new feature at the terrain pick position.
                if let Some(uv) = cursor_uv {
                    if let Some(hm) = app.paint.heightmap.as_ref() {
                        if let Some(renderer) = core.terrain_renderer.as_ref() {
                            let (height_scale, x_extent, z_extent) = renderer.mesh_extents();
                            if let Some(pick) = pick_terrain(
                                &core.camera,
                                aspect,
                                uv,
                                hm,
                                x_extent,
                                z_extent,
                                height_scale,
                            ) {
                                let (map_w, map_h) = app.map.dimensions();
                                let spring_x = (pick.world.x / x_extent + 1.0)
                                    * 0.5
                                    * map_w.max(1) as f32
                                    * 8.0;
                                let spring_z = (pick.world.z / z_extent + 1.0)
                                    * 0.5
                                    * map_h.max(1) as f32
                                    * 8.0;
                                app.push_undo("Place feature");
                                app.map.features.push(PlacedFeature {
                                    feature_type: feature_type.clone(),
                                    x: spring_x,
                                    y: 0.0,
                                    z: spring_z,
                                    angle: app.pending_placement_angle,
                                    taken_damage: 0,
                                });
                                app.map.features_placement_dirty = true;
                            }
                        }
                    }
                }
            } else {
                // Selection mode: cast the cursor ray against each feature's
                // oriented bounding box and select the closest hit. The hit
                // test runs against the actual rendered geometry's bounds,
                // not the terrain under the cursor, so clicks on the visible
                // body of a tall feature select reliably at any camera angle.
                if let Some(uv) = cursor_uv {
                    if let Some(renderer) = core.terrain_renderer.as_ref() {
                        let pickable = build_pickable_features(
                            &app.map.features,
                            &FeatureMapDims {
                                w: app.map.width,
                                h: app.map.height,
                                min_h: app.map.min_height,
                                max_h: app.map.max_height,
                            },
                            app.paint.heightmap.as_ref(),
                            renderer,
                        );
                        app.map.selected_feature_idx =
                            bar_render::pick_feature(&core.camera, aspect, uv, &pickable);
                    }
                }
            }
        }

        // Delete key removes the selected feature.
        if response.has_focus() || response.hovered() {
            if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
                if let Some(idx) = app.map.selected_feature_idx.take() {
                    if idx < app.map.features.len() {
                        app.push_undo("Delete feature");
                        app.map.features.remove(idx);
                        app.map.features_placement_dirty = true;
                    }
                }
            }

            // Escape: cancel placement or deselect.
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                if app.selected_feature_type.is_some() {
                    app.selected_feature_type = None;
                } else if app.map.selected_feature_idx.is_some() {
                    app.map.selected_feature_idx = None;
                }
            }
        }
    }

    // On the frame egui first recognises a drag, `drag_delta` is the
    // motion from the press point through the drag-recognition
    // threshold (a few pixels). Applying it produces a visible
    // single-frame jump on the camera. Subsequent frames give clean
    // incremental motion. Helper returns the per-frame delta after
    // suppressing this initial threshold-cross frame; sculpt
    // dabbing uses cursor position rather than delta so it's not
    // affected by this gating.
    let drag_delta_after_start = |button: egui::PointerButton| -> egui::Vec2 {
        if response.drag_started_by(button) {
            egui::Vec2::ZERO
        } else {
            response.drag_delta()
        }
    };

    if response.dragged_by(egui::PointerButton::Primary) {
        if sculpt_active {
            apply_sculpt_dab_at_cursor(core, gpu_context, response, ctx, app);
        } else if let Some(drag) = core.feature_drag {
            // Translate the dragged feature: cursor-projected terrain XZ
            // plus the press-time offset.
            if let (Some(uv), Some(hm), Some(renderer)) = (
                cursor_uv,
                app.paint.heightmap.as_ref(),
                core.terrain_renderer.as_ref(),
            ) {
                let (height_scale, x_extent, z_extent) = renderer.mesh_extents();
                if let Some(pick) = pick_terrain(
                    &core.camera,
                    aspect,
                    uv,
                    hm,
                    x_extent,
                    z_extent,
                    height_scale,
                ) {
                    let (map_w, map_h) = app.map.dimensions();
                    let cursor_spring_x =
                        (pick.world.x / x_extent + 1.0) * 0.5 * map_w.max(1) as f32 * 8.0;
                    let cursor_spring_z =
                        (pick.world.z / z_extent + 1.0) * 0.5 * map_h.max(1) as f32 * 8.0;
                    let new_x =
                        (cursor_spring_x + drag.offset_x).clamp(0.0, map_w.max(1) as f32 * 8.0);
                    let new_z =
                        (cursor_spring_z + drag.offset_z).clamp(0.0, map_h.max(1) as f32 * 8.0);
                    if let Some(f) = app.map.features.get_mut(drag.feature_idx) {
                        f.x = new_x;
                        f.z = new_z;
                        // y stays at the persisted value or 0 (re-snap
                        // to heightmap handled by the instance builder).
                        app.map.features_placement_dirty = true;
                    }
                }
            }
        } else if feature_type.is_none() {
            let delta = drag_delta_after_start(egui::PointerButton::Primary);
            core.camera.orbit(delta.x * 0.01, delta.y * 0.01);
            camera_changed = true;
        }
    }
    if response.drag_stopped_by(egui::PointerButton::Primary) {
        if sculpt_active {
            if let Some(kind) = app.paint.selected_fc_layer {
                app.end_brush_stroke_on_fc_layer(kind);
            } else {
                app.end_brush_stroke();
            }
        }
        core.feature_drag = None;
    }

    if response.dragged_by(egui::PointerButton::Secondary) {
        let delta = drag_delta_after_start(egui::PointerButton::Secondary);
        core.camera.orbit(delta.x * 0.01, delta.y * 0.01);
        camera_changed = true;
    }

    if cursor_world.is_some() || sculpt_active {
        camera_changed = true;
    }

    if response.dragged_by(egui::PointerButton::Middle) {
        let delta = drag_delta_after_start(egui::PointerButton::Middle);
        let speed = core.camera.distance * 0.0015;
        // Grab-and-drag-the-world: cursor stays anchored to a point
        // on the terrain. Drag the cursor right and the world slides
        // right with it, which means the camera target moves left
        // (negative right). Same logic on Y: drag down and the world
        // slides down, so the target moves forward into the scene.
        core.camera.pan_xz(-delta.x * speed, delta.y * speed);
        camera_changed = true;
    }

    if response.hovered() {
        // Rotation gesture: Ctrl + vertical scroll or any horizontal
        // scroll rotates the active feature -- either the selected
        // placed feature, or the pending-placement angle when the
        // user has a feature type queued for placing. Spring's heading
        // unit (`-32768..32767`, full circle = 65536) is what
        // `PlacedFeature::angle` stores and what `build_feature_instances`
        // converts to radians via `* pi / 32768`.
        //
        // We read raw `MouseWheel` events instead of `smooth_scroll_delta`
        // because egui rewrites Ctrl+wheel into Zoom events: it drops out
        // of the regular scroll delta entirely. Iterating events lets us
        // see both ctrl-modified vertical wheel AND horizontal wheel
        // with their original modifier state.
        let (rotation_lines, zoom_scroll_pixels) = ctx.input(|i| {
            let mut rot = 0.0;
            let mut zoom = 0.0;
            for event in &i.events {
                if let egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                } = event
                {
                    // Normalise to lines so the rotation step feels
                    // consistent across touchpads (Point) and wheel
                    // mice (Line).
                    let lines_per_unit = match unit {
                        egui::MouseWheelUnit::Line => 1.0,
                        egui::MouseWheelUnit::Point => 1.0 / 50.0,
                        egui::MouseWheelUnit::Page => 10.0,
                    };
                    let lines = *delta * lines_per_unit;
                    rot += lines.x;
                    if modifiers.ctrl {
                        rot += lines.y;
                    } else {
                        // Feed the existing zoom path in its original
                        // smooth_scroll_delta-ish units (~50 px per line).
                        zoom += lines.y * 50.0;
                    }
                }
            }
            (rot, zoom)
        });
        let rotate_active = app.paint.brush.tool == bar_gui::BrushTool::Pointer
            && app.active_layout() != bar_gui::Layout::Preview;
        if rotate_active && rotation_lines.abs() > 0.01 {
            // 2048 heading units per scroll-line = 11.25 deg/line;
            // a full revolution is ~32 lines. Negated so wheel-up
            // reads as clockwise (intuitive).
            let delta_heading = -rotation_lines * 2048.0;
            if let Some(idx) = app.map.selected_feature_idx {
                // Coalesce a continuous wheel-spin into one undo step;
                // start a fresh entry after a quiet gap.
                const ROTATE_GESTURE_GAP: std::time::Duration =
                    std::time::Duration::from_millis(500);
                let now = std::time::Instant::now();
                let new_gesture = core
                    .last_rotate_at
                    .map(|t| now.duration_since(t) >= ROTATE_GESTURE_GAP)
                    .unwrap_or(true);
                if new_gesture {
                    app.push_undo("Rotate feature");
                }
                core.last_rotate_at = Some(now);
                if let Some(f) = app.map.features.get_mut(idx) {
                    f.angle = wrap_heading(f.angle + delta_heading);
                    app.map.features_placement_dirty = true;
                }
            } else if app.selected_feature_type.is_some() {
                // Pending-placement angle is session-only state; it
                // isn't snapshotted by undo. No push_undo here.
                app.pending_placement_angle =
                    wrap_heading(app.pending_placement_angle + delta_heading);
            }
            // Eat the scroll: don't fall through to zoom even if the
            // user happened to scroll vertically with Ctrl held.
        } else if zoom_scroll_pixels.abs() > 0.1 {
            let factor = (-zoom_scroll_pixels * 0.0015).clamp(-0.5, 0.5);
            // Zoom-to-cursor: before applying the distance change, nudge the
            // camera target toward the world point under the cursor by the
            // same proportion as the zoom step. Geometrically, the camera
            // position interpolates toward the cursor pick as you scroll in
            // -- so the point under the cursor stays roughly under the
            // cursor across the zoom, instead of drifting toward the screen
            // centre. Skipped when the cursor isn't over the terrain (no
            // pick), preserving the prior "zoom toward target" feel offscreen.
            if let (Some(uv), Some(hm), Some(renderer)) = (
                cursor_uv,
                app.paint.heightmap.as_ref(),
                core.terrain_renderer.as_ref(),
            ) {
                let (height_scale, x_extent, z_extent) = renderer.mesh_extents();
                if let Some(pick) = pick_terrain(
                    &core.camera,
                    aspect,
                    uv,
                    hm,
                    x_extent,
                    z_extent,
                    height_scale,
                ) {
                    let to_pick = pick.world - core.camera.target;
                    core.camera.target += to_pick * (-factor);
                }
            }
            core.camera.zoom(factor);
            camera_changed = true;
        }
    }

    if camera_changed {
        // If the net effect of this frame's mutations would put the
        // camera underground, revert to the pre-frame state -- the
        // user's gesture does nothing instead of sliding the camera
        // along the terrain surface. Allow upward "escape" mutations
        // from a degenerate pre-below state so the user isn't stuck.
        if let (Some(renderer), Some(hm)) =
            (core.terrain_renderer.as_ref(), app.paint.heightmap.as_ref())
        {
            const TERRAIN_FLOOR_EPSILON: f32 = 0.005;
            let (height_scale, x_extent, z_extent) = renderer.mesh_extents();
            let floor_at = |x: f32, z: f32| -> Option<f32> {
                bar_render::terrain_y_at_world_xz(x, z, hm, x_extent, z_extent, height_scale)
                    .map(|y| y + TERRAIN_FLOOR_EPSILON)
            };
            let post_pos = core.camera.position();
            let post_below = floor_at(post_pos.x, post_pos.z)
                .map(|f| post_pos.y < f)
                .unwrap_or(false);
            if post_below {
                let pre_pos = pre_camera.position();
                let pre_below = floor_at(pre_pos.x, pre_pos.z)
                    .map(|f| pre_pos.y < f)
                    .unwrap_or(false);
                let escape_upward = pre_below && post_pos.y > pre_pos.y;
                if !escape_upward {
                    core.camera = pre_camera.clone();
                    camera_changed = false;
                }
            }
        }
    }

    if camera_changed {
        // If the look-at target was pushed outside the map's XZ
        // bounds (with a small overshoot so users can frame the
        // edges from just outside), revert this frame's mutations.
        // Constrains pan + target-snap; orbit / zoom don't move
        // target XZ so they're unaffected. Clamping target rather
        // than position keeps zoom-out-from-centre working: zoom
        // grows the camera's distance from target, which moves
        // position XZ outward, but target itself stays inside the
        // map. Allow escape inward from a degenerate pre-oob state.
        if let Some(renderer) = core.terrain_renderer.as_ref() {
            const TARGET_OOB_OVERSHOOT_FACTOR: f32 = 1.1;
            let (_, x_extent, z_extent) = renderer.mesh_extents();
            let bound_x = x_extent * TARGET_OOB_OVERSHOOT_FACTOR;
            let bound_z = z_extent * TARGET_OOB_OVERSHOOT_FACTOR;
            let post_t = core.camera.target;
            let post_oob = post_t.x.abs() > bound_x || post_t.z.abs() > bound_z;
            if post_oob {
                let pre_t = pre_camera.target;
                let pre_oob = pre_t.x.abs() > bound_x || pre_t.z.abs() > bound_z;
                let pre_dist = pre_t.x.abs().max(pre_t.z.abs());
                let post_dist = post_t.x.abs().max(post_t.z.abs());
                let escape_inward = pre_oob && post_dist < pre_dist;
                if !escape_inward {
                    core.camera = pre_camera.clone();
                    camera_changed = false;
                }
            }
        }
    }

    if camera_changed {
        if let (Some(ref mut renderer), Some(ref gpu)) = (&mut core.terrain_renderer, gpu_context) {
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

    // Placement ghost: when a feature type is queued for placement and
    // the cursor projects onto the terrain, surface a translucent
    // preview at that point. Cleared on any condition that suppresses
    // placement (UI hover, Preview layout, non-Pointer tool, cursor
    // off-map). Pure state write; the renderer consumes it on the
    // next feature-instance rebuild.
    app.placement_ghost = (|| -> Option<bar_project::recipe::PlacedFeature> {
        if app.paint.brush.tool != bar_gui::BrushTool::Pointer {
            return None;
        }
        if app.active_layout() == bar_gui::Layout::Preview {
            return None;
        }
        let feature_type = app.selected_feature_type.as_ref()?;
        let uv = cursor_uv?;
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
        let (map_w, map_h) = app.map.dimensions();
        let spring_x = (pick.world.x / x_extent + 1.0) * 0.5 * map_w.max(1) as f32 * 8.0;
        let spring_z = (pick.world.z / z_extent + 1.0) * 0.5 * map_h.max(1) as f32 * 8.0;
        Some(bar_project::recipe::PlacedFeature {
            feature_type: feature_type.clone(),
            x: spring_x,
            y: 0.0,
            z: spring_z,
            angle: app.pending_placement_angle,
            taken_damage: 0,
        })
    })();
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

    // Sculpt3D drives paint exclusively into FinalComposition's per-kind
    // layers. The 2D paint nodes (PaintedHeightmap / PaintedTexture) are
    // edited via the 2D inspector, never from this viewport.
    let fc_selected = app.paint.selected_fc_layer;

    let changed = if let Some(kind) = fc_selected {
        match kind {
            bar_gui::FCLayerKind::Heightmap => {
                app.apply_brush_to_fc_heightmap_layer(p.hm_x, p.hm_y, stroke_starting)
            }
            bar_gui::FCLayerKind::Color => app.apply_color_brush_to_fc_color_layer(p.hm_x, p.hm_y),
            bar_gui::FCLayerKind::Metalmap | bar_gui::FCLayerKind::Typemap => {
                app.apply_value_brush_to_fc_layer(kind, p.hm_x, p.hm_y)
            }
        }
    } else {
        false
    };

    if !changed {
        return;
    }

    // GPU upload dispatch: FC color layer updates go through the
    // albedo path (mirror in `paint.color_buffer`); FC heightmap
    // layer goes through the heightmap region upload. FC metalmap /
    // typemap layers don't have an instant-feedback path yet --
    // their changes only become visible on next eval, which is
    // acceptable since those overlays don't render in the sculpt3d
    // view anyway.
    let is_color = matches!(fc_selected, Some(bar_gui::FCLayerKind::Color));
    let is_value_only = matches!(
        fc_selected,
        Some(bar_gui::FCLayerKind::Metalmap | bar_gui::FCLayerKind::Typemap)
    );
    if is_value_only {
        // No GPU upload path; just bail (live buffer accumulates, flush
        // on mouse-up writes the asset, next eval reads it).
        return;
    }

    if is_color {
        if let (Some(ref gpu), Some(updated)) = (gpu_context, app.paint.color_buffer.clone()) {
            if let Some(ref mut renderer) = core.terrain_renderer {
                renderer.update_albedo(&gpu.device, &gpu.queue, &updated);
                let elapsed = core.started_at.elapsed().as_secs_f32();
                let smf = live_smf_lighting(app);
                let frame = core
                    .current_frame
                    .as_ref()
                    .map(|f| f.as_frame(elapsed, smf));
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
                let smf = live_smf_lighting(app);
                let frame = core
                    .current_frame
                    .as_ref()
                    .map(|f| f.as_frame(elapsed, smf));
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

/// Build feature instance transforms, grouped by feature type.
///
/// Returns `(model_groups, unknowns)`:
/// - `model_groups`: instances for feature types that have a loaded S3O model,
///   keyed by lowercase feature type name. Scale is `1/(pm*8)` (one Spring elmo
///   = one render unit).
/// - `unknowns`: instances for feature types without a loaded model, rendered
///   with the placeholder box. Scale is footprint-based.
///
/// `loaded_model_names` is the set of lowercase feature type names for which
/// `FeatureRenderer::load_mesh` has been called.
pub struct FeatureMapDims {
    pub w: u32,
    pub h: u32,
    pub min_h: f32,
    pub max_h: f32,
}

/// Constrain a Spring heading to its `(-32768, 32767]` representable range
/// by wrapping around the full-circle period (65536 units). Lets rotation
/// gestures accumulate freely without overflow or visible discontinuity.
pub fn wrap_heading(h: f32) -> f32 {
    const PERIOD: f32 = 65536.0;
    let mut v = h % PERIOD;
    if v > 32767.0 {
        v -= PERIOD;
    } else if v < -32768.0 {
        v += PERIOD;
    }
    v
}

pub fn build_feature_instances(
    features: &[PlacedFeature],
    dims: &FeatureMapDims,
    catalog: Option<&bar_engine::FeatureCatalog>,
    heightmap: Option<&bar_data::Heightmap>,
    loaded_model_names: &std::collections::HashSet<String>,
    selected_idx: Option<usize>,
    ghost: Option<&PlacedFeature>,
) -> (
    std::collections::HashMap<String, Vec<FeatureInstance>>,
    Vec<FeatureInstance>,
) {
    use glam::{Mat4, Quat, Vec3};

    let pw = (dims.w as f32 - 1.0).max(1.0);
    let ph = (dims.h as f32 - 1.0).max(1.0);
    let pm = pw.max(ph);
    let xe = (0.5 * pw / pm).min(0.5);
    let ze = (0.5 * ph / pm).min(0.5);
    let height_range = (dims.max_h - dims.min_h).abs().max(1.0);
    let hs = (height_range / (pm * 8.0)).max(0.005);
    // Uniform elmo-to-render scale: same factor as hs but without height_range.
    let elmo_scale = (1.0 / (pm * 8.0)).max(1e-6_f32);
    let default_footprint = 2.0_f32;

    let mut groups: std::collections::HashMap<String, Vec<FeatureInstance>> =
        std::collections::HashMap::new();
    let mut unknowns: Vec<FeatureInstance> = Vec::new();

    for (idx, f) in features.iter().enumerate() {
        let lower = f.feature_type.to_lowercase();
        let is_selected = selected_idx == Some(idx);

        let rx = (f.x / (pw * 8.0) - 0.5) * 2.0 * xe;
        let rz = (f.z / (ph * 8.0) - 0.5) * 2.0 * ze;

        let h_render = if let Some(hm) = heightmap {
            // Bilinear sample so the feature's base matches the *interpolated*
            // terrain surface between heightmap texels. The terrain mesh
            // vertex shader stores per-texel Y values, but the rasterizer
            // interpolates linearly across each quad -- a feature placed at
            // float coords (12.5, 12.5) sits on a fragment whose actual Y is
            // bilinear(Y[12,12], Y[13,12], Y[12,13], Y[13,13]). Nearest-texel
            // sampling here was making features hover above or sink into
            // their slopes by up to the per-texel height delta, which read
            // as the shadow detaching from the feature's visible base.
            let hx = (f.x / (pw * 8.0)).clamp(0.0, 1.0) * (hm.width().saturating_sub(1)) as f32;
            let hz = (f.z / (ph * 8.0)).clamp(0.0, 1.0) * (hm.height().saturating_sub(1)) as f32;
            let max_x = hm.width().saturating_sub(1);
            let max_z = hm.height().saturating_sub(1);
            let x0 = (hx.floor() as u32).min(max_x);
            let z0 = (hz.floor() as u32).min(max_z);
            let x1 = (x0 + 1).min(max_x);
            let z1 = (z0 + 1).min(max_z);
            let fx = hx - hx.floor();
            let fz = hz - hz.floor();
            let h00 = hm.get(x0, z0).unwrap_or(0.0);
            let h10 = hm.get(x1, z0).unwrap_or(0.0);
            let h01 = hm.get(x0, z1).unwrap_or(0.0);
            let h11 = hm.get(x1, z1).unwrap_or(0.0);
            let h0 = h00 * (1.0 - fx) + h10 * fx;
            let h1 = h01 * (1.0 - fx) + h11 * fx;
            (h0 * (1.0 - fz) + h1 * fz) * hs
        } else {
            hs * 0.5
        };
        let ry = if f.y.abs() < 0.01 {
            h_render
        } else {
            ((f.y - dims.min_h) / height_range) * hs
        };

        // SMF stores rotation as a float in the engine "heading" range
        // [-32768, 32767] (full circle = 65536 units). Recoil casts to int16
        // and computes angle_radians = heading * pi / 32768. Reproduce that
        // here. Note: bar-engine's own pipeline currently writes radians
        // through this same field; that's a separate bug, but for any feature
        // loaded from a real BAR map the value is heading-encoded.
        let rot = Quat::from_rotation_y(-f.angle * std::f32::consts::PI / 32768.0);

        let inst = if loaded_model_names.contains(&lower) {
            // Real S3O model: uniform scale in Spring elmos.
            let transform = Mat4::from_scale_rotation_translation(
                Vec3::splat(elmo_scale),
                rot,
                Vec3::new(rx, ry, rz),
            );
            let cols = transform.to_cols_array_2d();
            // Tint multiplies the sampled diffuse in the fragment shader. For a
            // real textured model the tint must be (1,1,1,1) so the texture's
            // own colors come through; the green/orange tints are placeholder
            // hints only. Selected features get a yellow highlight tint.
            let tint = if is_selected {
                [1.0, 1.0, 0.0, 1.0] // yellow = selected
            } else {
                [1.0, 1.0, 1.0, 1.0] // identity = pass texture through unchanged
            };
            let inst = FeatureInstance {
                col0: cols[0],
                col1: cols[1],
                col2: cols[2],
                col3: cols[3],
                tint,
            };
            groups.entry(lower).or_default().push(inst);
            continue;
        } else {
            // Placeholder box: footprint-based scale.
            let (fp_x, fp_z) = catalog
                .and_then(|cat| cat.features.get(&lower))
                .map(|def| (def.footprint_x.max(1) as f32, def.footprint_z.max(1) as f32))
                .unwrap_or((default_footprint, default_footprint));
            let sx = fp_x / pm;
            let sz = fp_z / pm;
            let sy = sx.max(sz);
            let transform = Mat4::from_scale_rotation_translation(
                Vec3::new(sx, sy, sz),
                rot,
                Vec3::new(rx, ry, rz),
            );
            let cols = transform.to_cols_array_2d();
            let tint = if is_selected {
                [1.0, 1.0, 0.0, 1.0] // yellow = selected
            } else if catalog
                .map(|c| c.is_known(&f.feature_type))
                .unwrap_or(false)
            {
                [0.2, 0.9, 0.2, 1.0] // green = catalog-known (no model yet)
            } else {
                [1.0, 0.5, 0.0, 1.0] // orange = unknown type
            };
            FeatureInstance {
                col0: cols[0],
                col1: cols[1],
                col2: cols[2],
                col3: cols[3],
                tint,
            }
        };
        unknowns.push(inst);
    }

    // Placement ghost: translucent preview at the cursor's terrain
    // projection. Uses the same instance pipeline as committed
    // features so it picks up the real model + lighting; tint alpha
    // is reduced so it reads as "preview, not committed". Loaded
    // models keep a white RGB (texture passes through with alpha
    // scaled); placeholders use the same green/orange palette as
    // committed unknowns but with the ghost alpha.
    if let Some(g) = ghost {
        let lower = g.feature_type.to_lowercase();
        let rx = (g.x / (pw * 8.0) - 0.5) * 2.0 * xe;
        let rz = (g.z / (ph * 8.0) - 0.5) * 2.0 * ze;
        let h_render = if let Some(hm) = heightmap {
            let hx = (g.x / (pw * 8.0)).clamp(0.0, 1.0) * (hm.width().saturating_sub(1)) as f32;
            let hz = (g.z / (ph * 8.0)).clamp(0.0, 1.0) * (hm.height().saturating_sub(1)) as f32;
            let max_x = hm.width().saturating_sub(1);
            let max_z = hm.height().saturating_sub(1);
            let x0 = (hx.floor() as u32).min(max_x);
            let z0 = (hz.floor() as u32).min(max_z);
            let x1 = (x0 + 1).min(max_x);
            let z1 = (z0 + 1).min(max_z);
            let fx = hx - hx.floor();
            let fz = hz - hz.floor();
            let h00 = hm.get(x0, z0).unwrap_or(0.0);
            let h10 = hm.get(x1, z0).unwrap_or(0.0);
            let h01 = hm.get(x0, z1).unwrap_or(0.0);
            let h11 = hm.get(x1, z1).unwrap_or(0.0);
            let h0 = h00 * (1.0 - fx) + h10 * fx;
            let h1 = h01 * (1.0 - fx) + h11 * fx;
            (h0 * (1.0 - fz) + h1 * fz) * hs
        } else {
            hs * 0.5
        };
        let ry = if g.y.abs() < 0.01 {
            h_render
        } else {
            ((g.y - dims.min_h) / height_range) * hs
        };
        let rot = Quat::from_rotation_y(-g.angle * std::f32::consts::PI / 32768.0);
        const GHOST_ALPHA: f32 = 0.5;
        if loaded_model_names.contains(&lower) {
            let transform = Mat4::from_scale_rotation_translation(
                Vec3::splat(elmo_scale),
                rot,
                Vec3::new(rx, ry, rz),
            );
            let cols = transform.to_cols_array_2d();
            groups.entry(lower).or_default().push(FeatureInstance {
                col0: cols[0],
                col1: cols[1],
                col2: cols[2],
                col3: cols[3],
                tint: [1.0, 1.0, 1.0, GHOST_ALPHA],
            });
        } else {
            let (fp_x, fp_z) = catalog
                .and_then(|cat| cat.features.get(&lower))
                .map(|def| (def.footprint_x.max(1) as f32, def.footprint_z.max(1) as f32))
                .unwrap_or((default_footprint, default_footprint));
            let sx = fp_x / pm;
            let sz = fp_z / pm;
            let sy = sx.max(sz);
            let transform = Mat4::from_scale_rotation_translation(
                Vec3::new(sx, sy, sz),
                rot,
                Vec3::new(rx, ry, rz),
            );
            let cols = transform.to_cols_array_2d();
            let known = catalog
                .map(|c| c.is_known(&g.feature_type))
                .unwrap_or(false);
            let tint = if known {
                [0.2, 0.9, 0.2, GHOST_ALPHA]
            } else {
                [1.0, 0.5, 0.0, GHOST_ALPHA]
            };
            unknowns.push(FeatureInstance {
                col0: cols[0],
                col1: cols[1],
                col2: cols[2],
                col3: cols[3],
                tint,
            });
        }
    }

    // Emit per-feature light markers for the SELECTED feature only.
    // BAR's deferred-rendering widget attaches point lights to certain
    // feature defs (most visible: `pilha_crystal_*`). BME doesn't run
    // the widget but renders a small coloured marker at each light's
    // position so map authors can see where lights will appear
    // in-engine. The marker reuses the placeholder-cube pipeline --
    // same instance struct, smaller scale, light-coloured tint.
    //
    // Gated on selection so a crystal-dense map (e.g. Azurite Shores)
    // isn't littered with constant marker cubes; the selected-feature
    // panel surfaces the same data textually for any feature.
    //
    // Heightmap sampling matches the first pass exactly so the marker
    // base sits on the same ry the feature does.
    const MARKER_SIZE_ELMOS: f32 = 8.0;
    let marker_scale = MARKER_SIZE_ELMOS * elmo_scale;
    let selected_feature = selected_idx.and_then(|i| features.get(i).map(|f| (i, f)));
    if let Some((_idx, f)) = selected_feature {
        let lower = f.feature_type.to_lowercase();
        let lights = bar_render::lights_for_feature_def(&lower);
        if lights.is_empty() {
            return (groups, unknowns);
        }
        let rx = (f.x / (pw * 8.0) - 0.5) * 2.0 * xe;
        let rz = (f.z / (ph * 8.0) - 0.5) * 2.0 * ze;
        let h_render = if let Some(hm) = heightmap {
            let hx = (f.x / (pw * 8.0)).clamp(0.0, 1.0) * (hm.width().saturating_sub(1)) as f32;
            let hz = (f.z / (ph * 8.0)).clamp(0.0, 1.0) * (hm.height().saturating_sub(1)) as f32;
            let max_x = hm.width().saturating_sub(1);
            let max_z = hm.height().saturating_sub(1);
            let x0 = (hx.floor() as u32).min(max_x);
            let z0 = (hz.floor() as u32).min(max_z);
            let x1 = (x0 + 1).min(max_x);
            let z1 = (z0 + 1).min(max_z);
            let fx = hx - hx.floor();
            let fz = hz - hz.floor();
            let h00 = hm.get(x0, z0).unwrap_or(0.0);
            let h10 = hm.get(x1, z0).unwrap_or(0.0);
            let h01 = hm.get(x0, z1).unwrap_or(0.0);
            let h11 = hm.get(x1, z1).unwrap_or(0.0);
            let h0 = h00 * (1.0 - fx) + h10 * fx;
            let h1 = h01 * (1.0 - fx) + h11 * fx;
            (h0 * (1.0 - fz) + h1 * fz) * hs
        } else {
            hs * 0.5
        };
        let ry = if f.y.abs() < 0.01 {
            h_render
        } else {
            ((f.y - dims.min_h) / height_range) * hs
        };
        for light in lights {
            let lx_render = rx + light.offset[0] * elmo_scale;
            let ly_render = ry + light.offset[1] * elmo_scale;
            let lz_render = rz + light.offset[2] * elmo_scale;
            let transform = Mat4::from_scale_rotation_translation(
                Vec3::splat(marker_scale),
                Quat::IDENTITY,
                Vec3::new(lx_render, ly_render, lz_render),
            );
            let cols = transform.to_cols_array_2d();
            unknowns.push(FeatureInstance {
                col0: cols[0],
                col1: cols[1],
                col2: cols[2],
                col3: cols[3],
                tint: [light.color[0], light.color[1], light.color[2], 1.0],
            });
        }
    }

    (groups, unknowns)
}

/// Build the per-feature `PickableFeature` entries the cursor picker tests
/// the camera ray against. Each entry carries the same world transform the
/// renderer uses to draw the feature, plus the model-space AABB:
/// - real S3O models: AABB queried from the feature renderer (the anchor-
///   shifted AABB stored when the mesh was uploaded);
/// - features without a loaded mesh: the unit placeholder cube AABB at the
///   default 2-elmo footprint, matching what `build_feature_instances`
///   draws when the catalog lookup misses.
///
/// Returned slice is index-aligned with `features`, so the picker can
/// return an index directly back to the caller.
pub fn build_pickable_features(
    features: &[PlacedFeature],
    dims: &FeatureMapDims,
    heightmap: Option<&bar_data::Heightmap>,
    renderer: &bar_render::TerrainRenderer,
) -> Vec<bar_render::PickableFeature> {
    use glam::{Mat4, Quat, Vec3};

    let pw = (dims.w as f32 - 1.0).max(1.0);
    let ph = (dims.h as f32 - 1.0).max(1.0);
    let pm = pw.max(ph);
    let height_range = (dims.max_h - dims.min_h).abs().max(1.0);
    let hs = (height_range / (pm * 8.0)).max(0.005);
    let elmo_scale = (1.0 / (pm * 8.0)).max(1e-6_f32);
    // Same fallback footprint `build_feature_instances` uses for catalog-
    // miss placeholders. Picker bounds match the rendered cube.
    let default_footprint = 2.0_f32;
    let placeholder_aabb_min = Vec3::new(-0.5, 0.0, -0.5);
    let placeholder_aabb_max = Vec3::new(0.5, 1.0, 0.5);

    let feat_renderer = renderer.feature_renderer();

    features
        .iter()
        .map(|f| {
            let lower = f.feature_type.to_lowercase();
            let rx = (f.x / (pw * 8.0) - 0.5) * 2.0 * (0.5 * pw / pm).min(0.5);
            let rz = (f.z / (ph * 8.0) - 0.5) * 2.0 * (0.5 * ph / pm).min(0.5);
            let h_render = if let Some(hm) = heightmap {
                let hx = (f.x / (pw * 8.0)).clamp(0.0, 1.0) * (hm.width().saturating_sub(1)) as f32;
                let hz =
                    (f.z / (ph * 8.0)).clamp(0.0, 1.0) * (hm.height().saturating_sub(1)) as f32;
                let max_x = hm.width().saturating_sub(1);
                let max_z = hm.height().saturating_sub(1);
                let x0 = (hx.floor() as u32).min(max_x);
                let z0 = (hz.floor() as u32).min(max_z);
                let x1 = (x0 + 1).min(max_x);
                let z1 = (z0 + 1).min(max_z);
                let fx = hx - hx.floor();
                let fz = hz - hz.floor();
                let h00 = hm.get(x0, z0).unwrap_or(0.0);
                let h10 = hm.get(x1, z0).unwrap_or(0.0);
                let h01 = hm.get(x0, z1).unwrap_or(0.0);
                let h11 = hm.get(x1, z1).unwrap_or(0.0);
                let h0 = h00 * (1.0 - fx) + h10 * fx;
                let h1 = h01 * (1.0 - fx) + h11 * fx;
                (h0 * (1.0 - fz) + h1 * fz) * hs
            } else {
                hs * 0.5
            };
            let ry = if f.y.abs() < 0.01 {
                h_render
            } else {
                ((f.y - dims.min_h) / height_range) * hs
            };
            let rot = Quat::from_rotation_y(-f.angle * std::f32::consts::PI / 32768.0);

            let (aabb_min, aabb_max, scale) =
                if let Some((mn, mx)) = feat_renderer.and_then(|fr| fr.mesh_aabb(&lower)) {
                    (mn, mx, Vec3::splat(elmo_scale))
                } else {
                    let sx = default_footprint / pm;
                    let sz = default_footprint / pm;
                    let sy = sx.max(sz);
                    (
                        placeholder_aabb_min,
                        placeholder_aabb_max,
                        Vec3::new(sx, sy, sz),
                    )
                };
            let transform =
                Mat4::from_scale_rotation_translation(scale, rot, Vec3::new(rx, ry, rz));
            bar_render::PickableFeature {
                transform,
                aabb_min,
                aabb_max,
            }
        })
        .collect()
}

// ── BC1 texture loading ───────────────────────────────────────────────────────

/// Load the compiled native-resolution BC1 texture into the viewport renderer.
/// Returns `true` on success.
/// Load the compiled BC1 texture into the Preview slot's terrain renderer.
/// Returns the native texture dimensions `(w, h)` on success, `None` on failure.
/// Walk `dir` recursively for a file whose basename matches `name`
/// (case-insensitive). Used to resolve mapinfo's bare filename
/// references (skybox, eventually detail textures) into actual disk
/// paths inside the `.barproj/passthrough/` tree.
fn find_file_in_dir(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    let needle = name.to_ascii_lowercase();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_ascii_lowercase() == needle)
                .unwrap_or(false)
            {
                return Some(path);
            }
        }
    }
    None
}

/// Result of loading the compiled BC1 albedo + heightmap into the
/// Preview layout's renderer. `heightmap` is returned separately so
/// the caller can install it on `app.paint.heightmap` -- without it,
/// terrain picking (zoom-to-cursor, orbit-snap, feature rendering)
/// silently no-ops in the Preview layout because `pick_terrain`
/// needs `app.paint.heightmap` to project screen rays onto the
/// surface.
pub struct LoadedBc1 {
    pub tex_dims: (u32, u32),
    pub heightmap: Option<bar_data::Heightmap>,
}

/// Output of the off-thread compiled-BC1 read. All file IO, BC1 assembly,
/// and heightmap decoding happen on the worker; the main thread takes
/// this struct and performs only the GPU uploads + state install in
/// `apply_compiled_bc1`.
pub struct Bc1LoadResult {
    pub tex_w: u32,
    pub tex_h: u32,
    pub bc1_bytes: Vec<u8>,
    /// Heightmap and the derived render-space parameters. `None` when
    /// the compiled package has no heightmap.bin -- the BC1 texture
    /// still uploads, but no terrain mesh is built.
    pub height: Option<Bc1HeightLoad>,
}

pub struct Bc1HeightLoad {
    pub heightmap: bar_data::Heightmap,
    pub height_scale: f32,
    pub x_extent: f32,
    pub z_extent: f32,
    pub water_y: f32,
    pub height_range: f32,
    pub elmo_per_render_xz: [f32; 2],
    pub grid_n: u32,
}

/// Read the compiled SMT + tile index + heightmap from the package on a
/// worker thread. Pure CPU; safe to call off the GUI thread. Returns
/// `None` if the package is missing required files or the fingerprint
/// is degenerate.
pub fn read_compiled_bc1_off_thread(
    project_dir: &std::path::Path,
    recipe_name: &str,
) -> Option<Bc1LoadResult> {
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
    let bc1_bytes = bar_data::assemble_bc1_linear(&tile_pool, &tile_indices, tiles_x, tiles_y);

    // Heightmap is optional -- the BC1 texture is still useful without it
    // (the renderer just outputs its clear color until eval populates a
    // mesh from the graph).
    let height = bar_engine::read_compiled_heightmap(&pkg).map(|hm| {
        let pw = (fp.map_x as f32).max(1.0);
        let ph = (fp.map_y as f32).max(1.0);
        let pm = pw.max(ph);
        let x_extent = (0.5 * pw / pm).min(0.5);
        let z_extent = (0.5 * ph / pm).min(0.5);
        let height_range = (fp.max_height - fp.min_height).abs().max(1.0);
        let height_scale = (height_range / (pm * 8.0)).max(0.005);
        let water_y = if fp.min_height < 0.0 {
            (-fp.min_height / height_range) * height_scale
        } else {
            -1.0
        };
        let elmo_per_render_xz = [pw * 4.0 / x_extent.max(1e-4), ph * 4.0 / z_extent.max(1e-4)];
        let grid_n = hm.width().max(hm.height()).min(2048);
        Bc1HeightLoad {
            heightmap: hm,
            height_scale,
            x_extent,
            z_extent,
            water_y,
            height_range,
            elmo_per_render_xz,
            grid_n,
        }
    });

    Some(Bc1LoadResult {
        tex_w,
        tex_h,
        bc1_bytes,
        height,
    })
}

/// Install a worker-produced BC1 load result into the renderer. Performs
/// only GPU uploads + `core.current_frame` install; the caller is
/// responsible for updating slot flags (`bc1_loaded`, `bc1_tex_dims`,
/// etc.) and propagating the heightmap to `app.paint.heightmap`.
pub fn apply_compiled_bc1(
    result: Bc1LoadResult,
    core: &mut ViewportCore,
    gpu: &GpuContext,
    water_color: [f32; 3],
) -> LoadedBc1 {
    if let Some(renderer) = core.terrain_renderer.as_mut() {
        renderer.upload_bc1_texture(
            &gpu.device,
            &gpu.queue,
            &result.bc1_bytes,
            result.tex_w,
            result.tex_h,
        );
        tracing::info!(
            tex_w = result.tex_w,
            tex_h = result.tex_h,
            "Preview BC1: native-resolution texture loaded"
        );
    }
    let mut loaded_hm: Option<bar_data::Heightmap> = None;
    if let Some(h) = result.height {
        if let Some(renderer) = core.terrain_renderer.as_mut() {
            renderer.update_heightmap(
                &gpu.device,
                &gpu.queue,
                &h.heightmap,
                TerrainUpdateParams {
                    height_scale: h.height_scale,
                    x_extent: h.x_extent,
                    z_extent: h.z_extent,
                    water_y: h.water_y,
                    water_color,
                    grid_n: h.grid_n,
                    height_range_elmos: h.height_range,
                    elmo_per_render_xz: h.elmo_per_render_xz,
                },
            );
            core.current_frame = Some(OwnedFrame {
                height_scale: h.height_scale,
                height_range_elmos: h.height_range,
                elmo_per_render_xz: h.elmo_per_render_xz,
                x_extent: h.x_extent,
                z_extent: h.z_extent,
                water_y: h.water_y,
                water_color,
                quality_high: true,
                tex_w: result.tex_w,
                tex_h: result.tex_h,
            });
            tracing::info!(
                w = h.heightmap.width(),
                h = h.heightmap.height(),
                "Preview BC1: terrain mesh loaded from compiled heightmap"
            );
        }
        loaded_hm = Some(h.heightmap);
    } else {
        tracing::warn!("Preview BC1: no compiled heightmap.bin -- terrain mesh not loaded");
    }

    LoadedBc1 {
        tex_dims: (result.tex_w, result.tex_h),
        heightmap: loaded_hm,
    }
}

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
pub struct ViewportCore {
    pub camera: Camera,
    pub terrain_renderer: Option<TerrainRenderer>,
    pub viewport_texture_id: Option<egui::TextureId>,
    pub current_frame: Option<OwnedFrame>,
    pub last_water_y: f32,
    pub last_water_color: [f32; 3],
    pub session_id: u64,
    pub started_at: Instant,
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
            skybox_loaded_for: None,
            detail_loaded_for: None,
            splat_loaded_for: None,
            sky_reflect_mod_loaded_for: None,
            specular_tex_loaded_for: None,
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

/// Load and upload the map-authored detail texture (mapinfo
/// `resources.detailTex`). Mirrors the skybox loader -- recursive
/// case-insensitive search in `passthrough/`, then the project root.
fn load_map_detail_texture(
    project_dir: Option<&std::path::Path>,
    detail_filename: &str,
    core: &mut ViewportCore,
    gpu: &GpuContext,
) {
    let Some(project_dir) = project_dir else {
        return;
    };
    // `project_dir` is the saved .barproj (textures packed under
    // `passthrough/`) OR the SD7 work_dir before save (textures still in
    // their archive-relative layout, typically `maps/<file>`). Try the
    // saved layout first; fall back to a recursive walk of the whole
    // project_dir so the work_dir layout is also covered. Without the
    // root fallback the renderer doesn't see splat/detail textures until
    // the user saves -- save copies them into `passthrough/` -- which
    // matches the symptom: "textures show up only after save+reimport".
    let path = find_file_in_dir(&project_dir.join("passthrough"), detail_filename)
        .or_else(|| find_file_in_dir(project_dir, detail_filename));
    let Some(path) = path else {
        tracing::warn!(
            file = detail_filename,
            "Detail texture not found in project; using default"
        );
        return;
    };
    let Some((rgba, w, h)) = load_2d_image(&path) else {
        tracing::warn!(
            file = detail_filename,
            "Failed to decode detail texture; using default"
        );
        return;
    };
    if let Some(renderer) = core.terrain_renderer.as_mut() {
        renderer.update_detail_texture(&gpu.device, &gpu.queue, &rgba, w, h);
        tracing::info!(file = detail_filename, w, h, "Detail texture loaded");
    }
}

/// Load and upload the map-authored sky reflection mask texture
/// (mapinfo `resources.skyReflectModTex`). Same resolution path as
/// the other terrain assets -- recursive case-insensitive walk in
/// `passthrough/`, decode via `load_2d_image`, upload. Quietly no-ops
/// when missing; the shader's mix factor stays zero and the SMF sky
/// reflection effect is off for the map.
pub fn sync_sky_reflect_mod(
    project_dir: Option<&std::path::Path>,
    filename: &str,
    core: &mut ViewportCore,
    gpu: &GpuContext,
) {
    let key = project_dir.map(|p| (p.to_path_buf(), filename.to_string()));
    if core.sky_reflect_mod_loaded_for == key {
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
    // Saved-layout (passthrough/) then full-project-recursive fallback
    // -- see `load_map_detail_texture` for the rationale.
    let path = find_file_in_dir(&project_dir.join("passthrough"), filename)
        .or_else(|| find_file_in_dir(project_dir, filename));
    let Some(path) = path else {
        tracing::warn!(
            file = filename,
            "skyReflectModTex not found in project; sky reflection disabled"
        );
        if let Some(renderer) = core.terrain_renderer.as_mut() {
            renderer.clear_sky_reflect_mod(&gpu.device, &gpu.queue);
        }
        core.sky_reflect_mod_loaded_for = key;
        return;
    };
    let Some((rgba, w, h)) = load_2d_image(&path) else {
        tracing::warn!(
            file = filename,
            "Failed to decode skyReflectModTex; sky reflection disabled"
        );
        if let Some(renderer) = core.terrain_renderer.as_mut() {
            renderer.clear_sky_reflect_mod(&gpu.device, &gpu.queue);
        }
        core.sky_reflect_mod_loaded_for = key;
        return;
    };
    if let Some(renderer) = core.terrain_renderer.as_mut() {
        renderer.update_sky_reflect_mod(&gpu.device, &gpu.queue, &rgba, w, h);
        tracing::info!(file = filename, w, h, "skyReflectModTex loaded");
    }
    core.sky_reflect_mod_loaded_for = key;
}

/// Load and upload the map-authored specular texture (mapinfo
/// `resources.specularTex`). Engine path: `SMF_SPECULAR_LIGHTING`. When
/// uploaded, the terrain fragment shader samples per-pixel specular
/// colour + exponent instead of using the global `groundSpecularColor`
/// uniform -- which is what stops maps like Ascendancy (with a non-zero
/// global spec colour) from washing out the entire sun-facing terrain.
pub fn sync_specular_tex(
    project_dir: Option<&std::path::Path>,
    filename: &str,
    core: &mut ViewportCore,
    gpu: &GpuContext,
) {
    let key = project_dir.map(|p| (p.to_path_buf(), filename.to_string()));
    if core.specular_tex_loaded_for == key {
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
    let path = find_file_in_dir(&project_dir.join("passthrough"), filename)
        .or_else(|| find_file_in_dir(project_dir, filename));
    let Some(path) = path else {
        tracing::warn!(
            file = filename,
            "specularTex not found in project; per-pixel specular disabled"
        );
        if let Some(renderer) = core.terrain_renderer.as_mut() {
            renderer.clear_specular_tex(&gpu.device, &gpu.queue);
        }
        core.specular_tex_loaded_for = key;
        return;
    };
    let Some((rgba, w, h)) = load_2d_image(&path) else {
        tracing::warn!(
            file = filename,
            "Failed to decode specularTex; per-pixel specular disabled"
        );
        if let Some(renderer) = core.terrain_renderer.as_mut() {
            renderer.clear_specular_tex(&gpu.device, &gpu.queue);
        }
        core.specular_tex_loaded_for = key;
        return;
    };
    if let Some(renderer) = core.terrain_renderer.as_mut() {
        renderer.update_specular_tex(&gpu.device, &gpu.queue, &rgba, w, h);
        tracing::info!(file = filename, w, h, "specularTex loaded");
    }
    core.specular_tex_loaded_for = key;
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
    if core.splat_loaded_for == key {
        return;
    }
    let Some(project_dir) = project_dir else {
        return;
    };

    // The distribution texture is required -- without it there's no
    // per-pixel cofactor and the splat path has no meaning. If it's
    // empty / missing / fails to decode we disable the whole path.
    //
    // The 4 detail-normals are each independently optional. Maps may
    // reference fewer than 4 channels, and shipped archives sometimes
    // reference a file they don't actually contain (Ascendency's
    // mapinfo lists `Ice_1k_dnts.tga` as channel 4 but the .sd7 doesn't
    // ship it). The engine tolerates per-channel misses by treating
    // that channel as zero-contribution; we match by substituting a
    // 1x1 mid-grey mip chain which yields 0 after the shader's
    // `*2-1` decode.
    let distr_name = &names[4];
    if distr_name.is_empty() {
        if let Some(renderer) = core.terrain_renderer.as_mut() {
            renderer.clear_splat_textures(&gpu.device, &gpu.queue);
        }
        core.splat_loaded_for = key;
        return;
    }

    let resolve_and_decode = |name: &str| -> Option<Vec<(Vec<u8>, u32, u32)>> {
        if name.is_empty() {
            return None;
        }
        // Saved-layout (passthrough/) then full-project-recursive
        // fallback. See `load_map_detail_texture` for why the unsaved
        // SD7 work_dir path also needs to be searchable here.
        let path = find_file_in_dir(&project_dir.join("passthrough"), name)
            .or_else(|| find_file_in_dir(project_dir, name))?;
        // For DDS sources this returns the file's pre-baked mip pyramid;
        // for non-DDS sources we get a single base mip.
        load_2d_image_with_mips(&path)
    };

    let distr = match resolve_and_decode(distr_name) {
        Some(m) => m,
        None => {
            tracing::warn!(
                file = %distr_name,
                "Splat distribution texture missing or failed to decode; advanced splat disabled"
            );
            if let Some(renderer) = core.terrain_renderer.as_mut() {
                renderer.clear_splat_textures(&gpu.device, &gpu.queue);
            }
            core.splat_loaded_for = key;
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

    let arr: [Vec<(Vec<u8>, u32, u32)>; 5] = [
        dn[0].take().unwrap_or_else(default_mip),
        dn[1].take().unwrap_or_else(default_mip),
        dn[2].take().unwrap_or_else(default_mip),
        dn[3].take().unwrap_or_else(default_mip),
        distr,
    ];

    if let Some(renderer) = core.terrain_renderer.as_mut() {
        renderer.update_splat_textures(&gpu.device, &gpu.queue, arr);
        tracing::info!("Splat detail textures loaded (with mip chains)");
    }
    core.splat_loaded_for = key;
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
    if core.detail_loaded_for == key {
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
    load_map_detail_texture(project_dir, detail_filename, core, gpu);
    core.detail_loaded_for = key;
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
    if core.skybox_loaded_for == key {
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
    load_map_skybox(project_dir, skybox_filename, core, gpu);
    core.skybox_loaded_for = key;
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
                                    angle: 0.0,
                                    taken_damage: 0,
                                });
                                app.map.features_placement_dirty = true;
                            }
                        }
                    }
                }
            } else {
                // Selection mode: pick the nearest feature.
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
                                let pw = (map_w as f32 - 1.0).max(1.0);
                                let ph = (map_h as f32 - 1.0).max(1.0);
                                let sx = (pick.world.x / x_extent + 1.0) * 0.5 * pw * 8.0;
                                let sz = (pick.world.z / z_extent + 1.0) * 0.5 * ph * 8.0;
                                // Tightened from 200 elmos: a typical feature
                                // footprint is 8-16 elmos (1-2 heightmap pixels),
                                // and 200 made selection pick anything within
                                // ~12 features of the click. 24 elmos lets the
                                // user click within 1-1.5 feature widths -- close
                                // to "must land on the feature" while still
                                // forgiving of a couple-pixel cursor jitter.
                                // True per-feature footprint thresholds would
                                // need the catalog plumbed through to this fn.
                                let threshold = 24.0_f32;
                                let prev = app.map.selected_feature_idx;
                                let best = app
                                    .map
                                    .features
                                    .iter()
                                    .enumerate()
                                    .filter_map(|(i, f)| {
                                        let dx = f.x - sx;
                                        let dz = f.z - sz;
                                        let d = dx * dx + dz * dz;
                                        if d < threshold * threshold {
                                            Some((i, d))
                                        } else {
                                            None
                                        }
                                    })
                                    .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                                app.map.selected_feature_idx = best.map(|(i, _)| i);
                                if app.map.selected_feature_idx != prev {
                                    app.map.features_placement_dirty = true;
                                }
                            }
                        }
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
                    app.map.features_placement_dirty = true;
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
        } else if feature_type.is_none() {
            let delta = drag_delta_after_start(egui::PointerButton::Primary);
            core.camera.orbit(delta.x * 0.01, delta.y * 0.01);
            camera_changed = true;
        }
    }
    if sculpt_active && response.drag_stopped_by(egui::PointerButton::Primary) {
        if let Some(kind) = app.paint.selected_fc_layer {
            app.end_brush_stroke_on_fc_layer(kind);
        } else {
            app.end_brush_stroke();
        }
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
        core.camera.pan_xz(delta.x * speed, -delta.y * speed);
        camera_changed = true;
    }

    if response.hovered() {
        let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.1 {
            let factor = (-scroll * 0.0015).clamp(-0.5, 0.5);
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

pub fn build_feature_instances(
    features: &[PlacedFeature],
    dims: &FeatureMapDims,
    catalog: Option<&bar_engine::FeatureCatalog>,
    heightmap: Option<&bar_data::Heightmap>,
    loaded_model_names: &std::collections::HashSet<String>,
    selected_idx: Option<usize>,
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

    (groups, unknowns)
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

/// Look for the map-authored skybox DDS file and, if found, upload it
/// as a cubemap to the renderer. The mapinfo `atmosphere.skyBox` field
/// is just a filename like `"cleardesert.dds"`; we resolve it against
/// likely locations under the .barproj (passthrough/, or as a direct
/// child for legacy maps). Quietly no-ops when the file isn't there
/// -- the shader will keep using procedural ModernSky in that case.
pub fn load_map_skybox(
    project_dir: Option<&std::path::Path>,
    skybox_filename: &str,
    core: &mut ViewportCore,
    gpu: &bar_compute::GpuContext,
) {
    let Some(project_dir) = project_dir else {
        return;
    };
    // Mapinfo's `skyBox = "..."` is just a filename; in the engine's VFS
    // that resolves recursively across the archive. Our `.barproj` puts
    // archive contents under `passthrough/`, and maps drop their skybox
    // in different subdirs (Aurelia: `passthrough/maps/cleardesert.dds`,
    // others: at `passthrough/` root, etc.). So we walk the passthrough
    // tree looking for a case-insensitive filename match, then fall back
    // to a full project_dir walk -- catches the SD7 work_dir layout
    // (textures at `<work_dir>/maps/<file>`) that's in play between
    // import and first save. See `load_map_detail_texture`.
    let path = find_file_in_dir(&project_dir.join("passthrough"), skybox_filename)
        .or_else(|| find_file_in_dir(project_dir, skybox_filename));
    let Some(path) = path else {
        tracing::warn!(
            file = skybox_filename,
            "Skybox DDS not found in project; using procedural sky"
        );
        return;
    };
    let cubemap = match bar_data::load_dds_cubemap(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                file = skybox_filename,
                err = %e,
                "Failed to decode skybox DDS; using procedural sky"
            );
            return;
        }
    };
    if let Some(renderer) = core.terrain_renderer.as_mut() {
        renderer.update_skybox(&gpu.device, &gpu.queue, &cubemap);
        tracing::info!(
            file = skybox_filename,
            w = cubemap.width,
            h = cubemap.height,
            "Skybox cubemap loaded"
        );
    }
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

pub fn load_compiled_bc1(
    project_dir: Option<&std::path::Path>,
    recipe_name: &str,
    core: &mut ViewportCore,
    gpu: &GpuContext,
    water_color: [f32; 3],
) -> Option<LoadedBc1> {
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
        "Preview BC1: native-resolution texture loaded"
    );

    // Upload the compiled heightmap as the terrain mesh so the BC1 texture
    // has geometry to project onto. Without this the renderer just outputs
    // its clear color.
    let mut loaded_hm: Option<bar_data::Heightmap> = None;
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
        // Elmo XZ conversion: render-XZ is [-extent, extent], world is
        // [-pw*4, pw*4] elmos (heightmap pixels are 8 elmos apart).
        let elmo_per_render_xz = [pw * 4.0 / x_extent.max(1e-4), ph * 4.0 / z_extent.max(1e-4)];
        let grid_n = hm.width().max(hm.height()).min(2048);
        // `water_color` here is the map-authored `water.basecolor` passed in
        // by the caller; the Preview pipeline reads it from `app.smf_lighting()`
        // at the BC1-load site so editing the Water tab affects this layout
        // too (the bind-group reupload still relies on the layout being
        // re-entered or a recompile triggering reload).
        renderer.update_heightmap(
            &gpu.device,
            &gpu.queue,
            &hm,
            TerrainUpdateParams {
                height_scale,
                x_extent,
                z_extent,
                water_y,
                water_color,
                grid_n,
                height_range_elmos: height_range,
                elmo_per_render_xz,
            },
        );
        core.current_frame = Some(OwnedFrame {
            height_scale,
            height_range_elmos: height_range,
            elmo_per_render_xz,
            x_extent,
            z_extent,
            water_y,
            water_color,
            quality_high: true,
            tex_w,
            tex_h,
        });
        tracing::info!(
            w = hm.width(),
            h = hm.height(),
            "Preview BC1: terrain mesh loaded from compiled heightmap"
        );
        loaded_hm = Some(hm);
    } else {
        tracing::warn!("Preview BC1: no compiled heightmap.bin -- terrain mesh not loaded");
    }

    Some(LoadedBc1 {
        tex_dims: (tex_w, tex_h),
        heightmap: loaded_hm,
    })
}

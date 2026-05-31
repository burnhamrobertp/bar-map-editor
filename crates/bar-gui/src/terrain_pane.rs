//! Shared 3D-rendering primitive for the editor's terrain panes.
//!
//! Both the main Sculpt3D / Preview viewport and the Layout
//! edit-view's live preview render a heightmap into an egui pane
//! through `bar_render::TerrainRenderer`. Without a primitive they
//! independently construct the renderer, drive `update_heightmap` +
//! `render`, register the offscreen view with egui, and read pointer
//! input -- which is how the orbit-direction / jerkiness bug class
//! emerged. `TerrainPane` is the contracted primitive each consumer
//! drives.
//!
//! The pane owns three things and nothing else: a `TerrainRenderer`,
//! a `Camera`, and the registered egui `TextureId`. Per-frame scene
//! data (lighting, water, lava) flows through `PreviewFrame`; the
//! pane does not store it. Optional scene-data uploads (BC1, coastmap,
//! features, ...) are separate `update_*` methods that are no-ops if
//! the host doesn't call them, so a consumer that doesn't have a piece
//! of data simply doesn't upload it -- no `if preview { ... }` branches.
//!
//! Extension axes (host calls in this order):
//!
//! 1. `present` -- allocate the egui rect, paint the texture, return
//!    a `Response`.
//! 2. Tools -- host-defined `fn(&Response, ...) -> ToolFlow`. First one
//!    returning `Consumed` short-circuits the rest.
//! 3. `apply_default_camera_input` -- canonical orbit / pan / zoom.
//!    Runs only when every tool passed.
//! 4. Overlays -- host-defined `fn(&Painter, Rect, &Camera, ...)` that
//!    paint screen-space adornments. Read the post-input camera.
//! 5. `render` -- record the render pass into the offscreen target.
//!
//! See `docs/terrain-pane-plan.md` for the full design.

use bar_data::{ColorBuffer, Heightmap};
use bar_render::{Camera, PreviewFrame, TerrainRenderer, TerrainUpdateParams};
use eframe::egui;

/// Quality preset applied at pane construction. Determines which
/// render passes the underlying `TerrainRenderer` runs; never
/// reconfigured at the call site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneQuality {
    /// Full sculpt-grade quality. Shadows, planar reflection and
    /// refraction, grass, advanced map / model shading, edge
    /// extension on the mesh. What Sculpt3D and the Preview layout
    /// use.
    Full,
    /// Lit-geometry preview. No shadows, no reflections, no grass,
    /// no edge extension, neutral lighting. What the Layout
    /// edit-view's live preview uses.
    Lit,
}

/// Result of a host-defined tool's `process` step. The host chains
/// tools with short-circuit semantics: the first one returning
/// `Consumed` prevents later tools and the default camera input from
/// seeing the gesture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolFlow {
    Consumed,
    Passed,
}

/// One terrain-rendering pane: a `TerrainRenderer` instance, the
/// camera that views it, and the egui `TextureId` the registered
/// offscreen view binds to. Constructed once per consumer (Sculpt
/// viewport, Layout preview, etc.); resized when the host's UI rect
/// changes; updated each frame via the `update_*` / `render` methods.
pub struct TerrainPane {
    renderer: TerrainRenderer,
    pub camera: Camera,
    texture_id: Option<egui::TextureId>,
    quality: PaneQuality,
    width: u32,
    height: u32,
}

impl TerrainPane {
    /// Build a pane at the given offscreen resolution + quality. The
    /// underlying renderer is constructed with the quality preset's
    /// flags applied; the host never touches `set_low_quality` etc.
    /// directly.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        size: u32,
        quality: PaneQuality,
    ) -> Self {
        let mut renderer = TerrainRenderer::new(device, queue, format);
        apply_quality(&mut renderer, quality);
        renderer.resize(device, size, size);
        Self {
            renderer,
            camera: Camera::default(),
            texture_id: None,
            quality,
            width: size,
            height: size,
        }
    }

    /// Resize the offscreen render target. Re-uploads happen lazily
    /// the next time `update_heightmap` / `render` runs.
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if width == self.width && height == self.height {
            return;
        }
        self.renderer.resize(device, width, height);
        self.width = width;
        self.height = height;
    }

    /// Construction-time quality preset. The pane stores this so
    /// future internal logic can branch on it without re-derivation;
    /// host code should not introspect it.
    #[inline]
    pub fn quality(&self) -> PaneQuality {
        self.quality
    }

    // ── Runtime quality overrides ───────────────────────────────────
    //
    // The `PaneQuality` preset applied at construction is a sensible
    // baseline; individual subsystems can still be toggled per-frame
    // to reflect user preferences (e.g. a display setting that hides
    // grass) or runtime constraints (e.g. forcing low quality on a
    // software adapter). These are pass-throughs to the underlying
    // renderer; the host applies whatever its context dictates.

    pub fn set_low_quality(&mut self, on: bool) {
        self.renderer.set_low_quality(on);
    }
    pub fn set_grass_visible(&mut self, visible: bool) {
        self.renderer.set_grass_visible(visible);
    }
    pub fn set_advanced_map_shading(&mut self, enabled: bool) {
        self.renderer.set_advanced_map_shading(enabled);
    }
    pub fn set_advanced_model_shading(&mut self, enabled: bool) {
        self.renderer.set_advanced_model_shading(enabled);
    }

    // ── Scene-data uploads ──────────────────────────────────────────
    //
    // Each is independent. A consumer that doesn't have a piece of
    // data (e.g. the Lit preview never has BC1 textures) simply
    // doesn't call the corresponding method. The underlying renderer
    // falls back to its default for any unset channel.

    /// Upload the heightmap that drives the terrain mesh + normal
    /// map. Required for any non-empty render.
    pub fn update_heightmap(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        hm: &Heightmap,
        params: TerrainUpdateParams,
    ) {
        self.renderer.update_heightmap(device, queue, hm, params);
    }

    /// Upload a BC1-encoded ground texture. Used by Sculpt3D for the
    /// compiled-SMT preview path; the Lit preview ignores ground
    /// texture entirely.
    pub fn upload_bc1_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
        tex_w: u32,
        tex_h: u32,
    ) {
        self.renderer
            .upload_bc1_texture(device, queue, bytes, tex_w, tex_h);
    }

    /// Upload the coastmap (shore-foam distance field) as RGBA bytes
    /// matching the format the underlying renderer expects.
    pub fn update_coastmap(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[u8],
        w: u32,
        h: u32,
    ) {
        self.renderer.update_coastmap(device, queue, rgba, w, h);
    }

    /// Upload a ground albedo / colour buffer.
    pub fn update_albedo(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, cb: &ColorBuffer) {
        self.renderer.update_albedo(device, queue, cb);
    }

    /// Upload the metal-spot map.
    pub fn update_metalmap(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, hm: &Heightmap) {
        self.renderer.update_metalmap(device, queue, hm);
    }

    /// Upload the terrain type map.
    pub fn update_typemap(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, hm: &Heightmap) {
        self.renderer.update_typemap(device, queue, hm);
    }

    /// Mutable access to the underlying renderer for upload paths the
    /// pane has not yet exposed as named methods. Used during the
    /// viewport migration (Phase C1) to avoid blocking on adding every
    /// possible passthrough up front; named methods get added as real
    /// consumers need them.
    pub fn renderer_mut(&mut self) -> &mut TerrainRenderer {
        &mut self.renderer
    }

    /// Immutable view onto the underlying renderer for callers that
    /// need to query state (e.g. `output_view`, `mesh_extents`).
    pub fn renderer(&self) -> &TerrainRenderer {
        &self.renderer
    }

    // ── Frame protocol ──────────────────────────────────────────────

    /// Record the render pass into the offscreen target using the
    /// pane's own camera. `frame` carries per-frame state (lighting,
    /// water, lava, time) that the host constructs from its own data;
    /// pass `None` to render the "no scene" path.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: Option<&PreviewFrame>,
    ) {
        self.renderer.render(device, queue, &self.camera, frame);
    }

    /// Same as `render` but uses an externally-owned camera instead
    /// of `self.camera`. Used by the Sculpt viewport during the C1
    /// migration slice, where the camera still lives on `ViewportCore`
    /// (it gets consolidated onto the pane in C2). Other consumers
    /// should call `render` and mutate `pane.camera` directly.
    pub fn render_with_camera(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        frame: Option<&PreviewFrame>,
    ) {
        self.renderer.render(device, queue, camera, frame);
    }

    /// Register the offscreen render output with egui as a native
    /// texture, or refresh the existing binding if one was already
    /// registered. Called from a host context with access to the
    /// egui-wgpu render state (bar-app's runner). Idempotent --
    /// safe to call every render, including before the first one.
    pub fn bind_egui_texture(&mut self, render_state: &eframe::egui_wgpu::RenderState) {
        let Some(view) = self.renderer.output_view() else {
            return;
        };
        let mut egui_rend = render_state.renderer.write();
        if let Some(tex_id) = self.texture_id {
            egui_rend.update_egui_texture_from_wgpu_texture(
                &render_state.device,
                view,
                wgpu::FilterMode::Linear,
                tex_id,
            );
        } else {
            let tex_id = egui_rend.register_native_texture(
                &render_state.device,
                view,
                wgpu::FilterMode::Linear,
            );
            self.texture_id = Some(tex_id);
        }
    }

    /// Paint the currently-bound texture into an egui rect and return
    /// the input `Response` for downstream tool / camera-input use.
    /// `sense` controls what kind of input the pane reports: pass
    /// `Sense::click_and_drag()` for an interactive pane and
    /// `Sense::hover()` for a read-only thumbnail.
    ///
    /// Must be called before tools, default camera input, and any
    /// overlays each frame -- those all need the rect / Response.
    pub fn paint(
        &mut self,
        ui: &mut egui::Ui,
        size: egui::Vec2,
        sense: egui::Sense,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(size, sense);
        if let Some(tex_id) = self.texture_id {
            ui.painter_at(rect).image(
                tex_id,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        response
    }

    /// True iff the pane has rendered at least one frame and the
    /// resulting offscreen texture is bound to egui -- i.e. `paint`
    /// will draw something instead of an empty rect. Hosts use this
    /// to decide between drawing the pane and a "rendering..."
    /// placeholder.
    pub fn has_texture(&self) -> bool {
        self.texture_id.is_some()
    }

    /// The currently-bound egui texture id, if any. Hosts that paint
    /// the pane themselves (rather than calling `paint`) read this to
    /// build their own `egui::Image`. Returns `None` before the first
    /// `bind_egui_texture` call.
    pub fn texture_id(&self) -> Option<egui::TextureId> {
        self.texture_id
    }

    /// Canonical orbit / pan / zoom against this pane's camera. The
    /// host calls this only when every tool returned `Passed` --
    /// tools that claim a gesture (sculpt brush, sun gizmo, feature
    /// drag) prevent the camera from also moving.
    ///
    /// Returns whether the camera changed this frame. Simple panes
    /// like the Layout preview use this single entry point; hosts
    /// that gate each gesture independently (e.g. the Sculpt
    /// viewport, which has its own RMB orbit and ctrl+scroll feature
    /// rotation) call the per-gesture free functions in this module
    /// directly.
    pub fn apply_default_camera_input(
        &mut self,
        response: &egui::Response,
        ctx: &egui::Context,
    ) -> bool {
        let a = apply_orbit_primary(&mut self.camera, response);
        let b = apply_pan_middle(&mut self.camera, response);
        let c = apply_zoom_scroll(&mut self.camera, response, ctx);
        a || b || c
    }
}

// ── Per-gesture camera input ─────────────────────────────────────────
//
// These free functions are the single source of truth for the
// editor's terrain-camera math: orbit on primary drag, pan on middle
// drag, zoom on scroll. They operate on any `&mut Camera`, not just
// `pane.camera`, so a host that owns its camera externally (e.g. the
// Sculpt viewport, which stores `core.camera` and gates each gesture
// against its own tool state) can share the math without restructuring
// where the camera lives.
//
// `TerrainPane::apply_default_camera_input` is a convenience that
// chains all three against `self.camera`; hosts that want per-gesture
// gating call these directly.
//
// Sensitivity constants:
//   * orbit: 0.01 rad/px on both axes -- same value the original
//     viewport used.
//   * pan:   `camera.distance * 0.0015` world units per pixel of
//     drag (right and forward).
//   * zoom:  factor = clamp(-scroll * 0.0015, -0.5, 0.5);
//     `distance *= 1 + factor`. Scroll-up shrinks the orbit (zoom in).
//
// Drag-start suppression: egui's first drag delta crosses the whole
// recognition threshold; using it produces a single-frame jump. Each
// helper drops that first frame.

fn drag_delta_after_start(response: &egui::Response, button: egui::PointerButton) -> egui::Vec2 {
    if response.drag_started_by(button) {
        egui::Vec2::ZERO
    } else if response.dragged_by(button) {
        response.drag_delta()
    } else {
        egui::Vec2::ZERO
    }
}

/// Apply LMB-drag orbit to `camera`. Returns whether the camera moved.
pub fn apply_orbit_primary(camera: &mut Camera, response: &egui::Response) -> bool {
    orbit_math(
        camera,
        drag_delta_after_start(response, egui::PointerButton::Primary),
    )
}

/// Apply MMB-drag pan to `camera`. Returns whether the camera moved.
pub fn apply_pan_middle(camera: &mut Camera, response: &egui::Response) -> bool {
    pan_math(
        camera,
        drag_delta_after_start(response, egui::PointerButton::Middle),
    )
}

/// Apply scroll-wheel zoom to `camera`. Returns whether it changed.
/// Skipped unless the cursor is hovering the pane's response rect.
pub fn apply_zoom_scroll(
    camera: &mut Camera,
    response: &egui::Response,
    ctx: &egui::Context,
) -> bool {
    if !response.hovered() {
        return false;
    }
    zoom_math(camera, ctx.input(|i| i.smooth_scroll_delta.y))
}

/// Apply a quality preset's flags to a freshly-constructed renderer.
/// Called only by `TerrainPane::new`; the host never reaches in.
fn apply_quality(r: &mut TerrainRenderer, quality: PaneQuality) {
    match quality {
        PaneQuality::Full => {
            r.set_low_quality(false);
            r.set_grass_visible(true);
            r.set_advanced_map_shading(true);
            r.set_advanced_model_shading(true);
        }
        PaneQuality::Lit => {
            r.set_low_quality(true);
            r.set_grass_visible(false);
            r.set_advanced_map_shading(false);
            r.set_advanced_model_shading(false);
        }
    }
}

// ── Pure math layer ─────────────────────────────────────────────────
//
// These three functions are the testable math behind the public
// `apply_orbit_primary` / `apply_pan_middle` / `apply_zoom_scroll`
// helpers above. They take primitive deltas (no egui types) so they
// can be unit-tested without faking a `Response` or `Context`.

fn orbit_math(camera: &mut Camera, drag: egui::Vec2) -> bool {
    if drag == egui::Vec2::ZERO {
        return false;
    }
    camera.orbit(drag.x * 0.01, drag.y * 0.01);
    true
}

fn pan_math(camera: &mut Camera, drag: egui::Vec2) -> bool {
    if drag == egui::Vec2::ZERO {
        return false;
    }
    let speed = camera.distance * 0.0015;
    camera.pan_xz(-drag.x * speed, drag.y * speed);
    true
}

fn zoom_math(camera: &mut Camera, scroll: f32) -> bool {
    if scroll.abs() <= 0.1 {
        return false;
    }
    let factor = (-scroll * 0.0015).clamp(-0.5, 0.5);
    camera.distance = (camera.distance * (1.0 + factor)).clamp(0.2, 8.0);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_4;

    fn fresh_camera() -> Camera {
        Camera::default()
    }

    #[test]
    fn no_orbit_input_leaves_camera_untouched() {
        let mut cam = fresh_camera();
        assert!(!orbit_math(&mut cam, egui::Vec2::ZERO));
        assert_eq!(cam.azimuth, FRAC_PI_4);
    }

    #[test]
    fn orbit_positive_dx_increases_azimuth() {
        // Drag right -> azimuth grows. Same sign convention as the
        // main viewport's `Camera::orbit(delta.x * 0.01, ...)`.
        let mut cam = fresh_camera();
        let before = cam.azimuth;
        assert!(orbit_math(&mut cam, egui::vec2(100.0, 0.0)));
        assert!(cam.azimuth > before);
        assert!((cam.azimuth - (before + 100.0 * 0.01)).abs() < 1e-5);
    }

    #[test]
    fn orbit_positive_dy_increases_elevation() {
        let mut cam = fresh_camera();
        let before = cam.elevation;
        assert!(orbit_math(&mut cam, egui::vec2(0.0, 100.0)));
        assert!(cam.elevation > before);
    }

    #[test]
    fn orbit_clamps_elevation_within_safe_range() {
        // Many huge upward drags should still leave elevation strictly
        // below pi/2 (Camera::orbit clamps internally).
        let mut cam = fresh_camera();
        for _ in 0..200 {
            orbit_math(&mut cam, egui::vec2(0.0, 9999.0));
        }
        assert!(cam.elevation < std::f32::consts::FRAC_PI_2);
        assert!(cam.elevation > -std::f32::consts::FRAC_PI_2);
    }

    #[test]
    fn zoom_scroll_up_shrinks_distance() {
        // Positive `scroll` (wheel up) -> smaller distance.
        let mut cam = fresh_camera();
        let before = cam.distance;
        assert!(zoom_math(&mut cam, 100.0));
        assert!(cam.distance < before);
    }

    #[test]
    fn zoom_scroll_down_grows_distance() {
        let mut cam = fresh_camera();
        let before = cam.distance;
        assert!(zoom_math(&mut cam, -100.0));
        assert!(cam.distance > before);
    }

    #[test]
    fn zoom_dead_zone_ignores_tiny_scrolls() {
        let mut cam = fresh_camera();
        let before = cam.distance;
        assert!(!zoom_math(&mut cam, 0.05));
        assert_eq!(cam.distance, before);
    }

    #[test]
    fn zoom_clamps_to_min_and_max_distance() {
        let mut cam = fresh_camera();
        for _ in 0..200 {
            zoom_math(&mut cam, 9999.0);
        }
        assert!((cam.distance - 0.2).abs() < 1e-4);

        let mut cam = fresh_camera();
        for _ in 0..200 {
            zoom_math(&mut cam, -9999.0);
        }
        assert!((cam.distance - 8.0).abs() < 1e-4);
    }

    #[test]
    fn pan_right_drag_moves_target() {
        // The viewport's middle-drag convention: drag the cursor
        // right, the world slides right under the cursor (so the
        // camera target moves LEFT in world space). Verifies the sign
        // on `pan_xz(-dx * speed, ...)`.
        let mut cam = fresh_camera();
        let before = cam.target.x;
        assert!(pan_math(&mut cam, egui::vec2(100.0, 0.0)));
        assert_ne!(cam.target.x, before);
    }

    #[test]
    fn pan_zero_delta_does_not_change_target() {
        let mut cam = fresh_camera();
        let before = cam.target;
        assert!(!pan_math(&mut cam, egui::Vec2::ZERO));
        assert_eq!(cam.target, before);
    }
}

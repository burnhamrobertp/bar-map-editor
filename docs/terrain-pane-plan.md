# TerrainPane: shared 3D-rendering primitive

## Context

Two places in the editor drive a `bar_render::TerrainRenderer` to fill
an egui pane with a 3D view of a heightmap:

- **`crates/bar-app/src/viewport.rs`** -- the main Sculpt3D / Preview
  viewport. Full quality: shadows, planar reflection / refraction,
  grass, BC1 ground texture, coastmap, water + lava, sun-direction
  lighting from `MapSettings`, feature instances, sculpt brush, sun
  gizmo, terrain picking for placement.
- **`crates/bar-app/src/runner.rs::maybe_render_layout_preview`** +
  **`crates/bar-gui/src/panels/properties/layout/mod.rs::draw_layout_preview_pane`**
  -- the Layout edit-view's live preview. Lit geometry only: no
  shadows, no reflections, no grass, neutral lighting, no scene data
  beyond a single heightmap.

Each independently:

1. constructs a `TerrainRenderer`,
2. drives `update_heightmap` + `render`,
3. binds `output_view()` to an `egui::TextureId`,
4. reads pointer input and updates a `Camera`.

Steps 1-3 already drifted (the preview's params synthesis is its own
hand-rolled subset of what the viewport does). Step 4 caused the
rotation-direction + jerkiness bug class. Sharing utility functions is
not enough -- the shape we need is a *primitive with a contract* that
both contexts use the same way, plus a defined seam for the different
things each one needs to add.

## Decisions

1. **Location:** `pub mod terrain_pane;` inside `bar-gui`. bar-app
   already depends on bar-gui; no new crate boundary.
2. **Extension shape:** **functions**, not traits. Each tool is a plain
   `fn(response, ctx, &mut state) -> ToolFlow`; each overlay is
   `fn(painter, rect, camera, &state) -> ()`. The host composes them in
   the order it chooses. No `Box<dyn Tool>`, no registry, no dynamic
   dispatch.
3. **Migration sequence:** Phase A (primitive + tests) -> Phase B
   (Layout preview migration) -> Phase C (Sculpt viewport migration,
   split into C1 / C2 / C3). Each phase is its own commit.
4. **Quality presets:** `PaneQuality::Full` and `PaneQuality::Lit` at
   launch. No `Custom(..)` escape hatch until a real consumer needs
   one.

## Architecture

### The primitive

```rust
pub struct TerrainPane {
    renderer: Option<TerrainRenderer>,
    pub camera: Camera,
    texture_id: Option<egui::TextureId>,
    quality: PaneQuality,
}

pub enum PaneQuality {
    /// Full sculpt-grade: shadows + planar reflection + refraction +
    /// grass + edge extension + advanced shading. Used by Sculpt3D
    /// and the Preview layout.
    Full,
    /// Lit-geometry preview: no shadows / reflection / grass / edge
    /// extension; neutral lighting. Used by the Layout edit-view
    /// preview.
    Lit,
}

pub enum ToolFlow { Consumed, Passed }
```

Quality is fixed at construction. Scene data is uploaded per-frame via
separate `update_*` methods; a method the host doesn't call simply
doesn't contribute (no `if preview { ... }` branches anywhere).

### Public API

```rust
impl TerrainPane {
    // Lifecycle
    pub fn new(device, queue, format, size: u32, quality: PaneQuality) -> Self;
    pub fn resize(&mut self, device, w: u32, h: u32);

    // Scene-data uploads. Each is independent; not calling one just
    // means the corresponding contribution is absent from the render.
    pub fn update_heightmap(&mut self, device, queue, hm, params);
    pub fn update_bc1_texture(&mut self, device, queue, bytes, w, h);
    pub fn update_coastmap(&mut self, device, queue, samples, w, h);
    pub fn set_lighting(&mut self, smf: SmfLighting);
    pub fn set_water(&mut self, params: WaterParams);
    pub fn set_lava(&mut self, params: LavaParams);
    pub fn update_features(&mut self, instances: &[FeatureInstance]);

    // Frame protocol -- split between contexts:
    //   * bind_egui_texture runs where the render_state lives (bar-app
    //     runner) right after `render`.
    //   * paint runs where the UI is built (bar-gui) each frame.
    pub fn bind_egui_texture(&mut self, render_state: &RenderState);
    pub fn paint(&mut self, ui: &mut Ui, size: Vec2, sense: Sense) -> egui::Response;
    pub fn has_texture(&self) -> bool;  // placeholder-vs-paint branching
    pub fn render(&mut self, device, queue, frame: Option<&PreviewFrame>);

    // Canonical camera input. Host calls only if no tool consumed.
    pub fn apply_default_camera_input(&mut self, response: &Response, ctx: &Context) -> bool;
}
```

### Tools and overlays

These are not types on the pane. They're conventions for functions the
host calls.

```rust
// Tool signature (host-defined per tool):
fn sculpt_brush_tool(
    response: &egui::Response,
    ctx: &egui::Context,
    state: &mut SculptState,
) -> ToolFlow;

// Overlay signature (host-defined per overlay):
fn sun_direction_overlay(
    painter: &egui::Painter,
    rect: egui::Rect,
    camera: &Camera,
    state: &SunState,
);
```

The host wires them together in its update loop. The pane only enforces
the *protocol order* below.

## Layering rules

These are the rules a host must follow. Violating them produces a
visible failure (no render, or a one-frame-late camera), not a subtle
one.

1. **`present` first.** Allocates the egui rect + Response, paints the
   currently-bound texture. Nothing else can run without the Response
   it produces.
2. **Tools before default camera-input.** The host calls its tools in
   order; the first one returning `Consumed` prevents later tools and
   `apply_default_camera_input` from seeing the gesture.
3. **`apply_default_camera_input` after tools.** Runs only when every
   tool passed.
4. **Overlays after camera-input, before render.** Overlays may read
   the just-updated camera (e.g. the sun gizmo projects sun direction
   onto screen using the post-orbit camera). Overlays do not mutate the
   pane.
5. **`render` last.** After all state mutations for the frame have
   settled.

## Consumer skeletons

**Layout preview** (Phase B). The host is split: bar-app's runner
drives the render + texture binding (it owns the device, queue,
render_state); bar-gui's editor draws the pane + handles camera input.

```rust
// In bar-app's runner.rs (each frame, only when dirty / node changed):
let hm = eval_layout_node_in_isolation(...);
pane.update_heightmap(device, queue, &hm, lit_preview_params());
pane.render(device, queue, Some(&lit_preview_frame()));
pane.bind_egui_texture(&render_state);

// In bar-gui's edit view (every frame):
let response = pane.paint(ui, size, Sense::click_and_drag());
let changed = pane.apply_default_camera_input(&response, ctx);
if changed { mark_preview_dirty(); }
```

**Sculpt viewport** (Phase C):
```rust
let response = pane.paint(ui, size, Sense::click_and_drag());

let mut flow = ToolFlow::Passed;
if flow == Passed { flow = sun_gizmo_tool(&response, ctx, &mut core.sun_drag); }
if flow == Passed { flow = sculpt_brush_tool(&response, ctx, &mut sculpt_state, ...); }
if flow == Passed { flow = feature_drag_tool(&response, ctx, &mut feature_drag, ...); }
if flow == Passed { flow = feature_place_tool(&response, ctx, &mut place_state, ...); }
if flow == Passed { pane.apply_default_camera_input(&response, ctx); }

// Scene updates -- viewport calls whichever apply this frame
pane.set_lighting(app.smf_lighting().into());
pane.update_heightmap(device, queue, hm, full_quality_params());
pane.update_bc1_texture(device, queue, &bc1, tex_w, tex_h);
pane.update_features(&instances);
// ... etc.

// Overlays
sun_direction_overlay(&painter, rect, &pane.camera, &core.sun);
metal_spot_overlay   (&painter, rect, &pane.camera, &app.map);
ghost_feature_overlay(&painter, rect, &pane.camera, &place_state);
if app.viewport_debug.show_camera_readout {
    camera_readout_overlay(&painter, rect, &pane.camera);
}

pane.render(device, queue, Some(&preview_frame));
```

## Migration phases

### Phase A: build the primitive

- Land `bar_gui::terrain_pane::{TerrainPane, PaneQuality, ToolFlow}` +
  this doc.
- `apply_default_camera_input` delegates to `Camera::orbit` /
  `Camera::pan_xz` with the **same** constants the current viewport
  uses (`drag_delta_after_start` suppression, `* 0.01` orbit
  sensitivity, `distance * 0.0015` pan speed, `-scroll * 0.0015` zoom
  factor clamped to `[-0.5, 0.5]`).
- Unit tests cover orbit direction (positive `dx` increases azimuth),
  drag-start suppression (first-frame zero delta), pan_xz direction
  (drag right moves world right under cursor), zoom sign (scroll up
  shrinks distance).
- No consumer migrated yet.

### Phase B: migrate the Layout preview

- `BarEditorApp::layout_preview` holds a `TerrainPane` (PaneQuality::Lit)
  instead of the loose `camera` + `texture_id` + the runner's separate
  `layout_preview_renderer`.
- `runner.rs::maybe_render_layout_preview` does the single-node eval
  then calls `pane.update_heightmap` + `pane.render`. No direct
  `TerrainRenderer` interaction in bar-app for the preview.
- `bar-gui::layout::draw_layout_preview_pane` does
  `pane.paint` + `pane.apply_default_camera_input` and is otherwise
  a thin wrapper. The runner calls `pane.bind_egui_texture` after
  each `render` so the texture id is fresh by the time the GUI
  paints.

### Phase C: migrate the Sculpt viewport

Three sub-slices, each shippable on its own.

- **C1: renderer + texture binding.** Replace `viewport.rs`'s
  `TerrainRenderer::new` / `resize` / `update_heightmap` / `render` /
  `register_native_texture` / `update_egui_texture_from_wgpu_texture`
  with calls to `pane.*` equivalents. Keeps every input branch and
  overlay paint exactly where they are. Validates that the pane
  supports the viewport's wgpu needs (BC1 upload, coastmap, normal
  map, water/lava, lighting, features).
- **C2: camera input.** Replace the `else if feature_type.is_none()`
  orbit/pan/zoom block + middle-drag pan + scroll zoom with
  `pane.apply_default_camera_input`. Viewport keeps its tool-precedence
  gating (sun gizmo / sculpt brush / feature drag take priority);
  only the math behind the default-camera branch becomes shared.
- **C3 (optional, deferred):** Lift sun-direction gizmo, metal-spot
  overlay, ghost-feature preview, camera-readout overlay into named
  functions matching the overlay signature. Pure refactor; no behavior
  change. Pays off when a future context wants a subset of overlays
  without re-implementing them.

## Risk surface

- **C1 is the load-bearing slice.** The viewport has a lot of layered
  state (sculpt strokes, feature placement / dragging / picking, sun
  gizmo, ghost previews). The migration must preserve every input
  branch's existing semantics. Approach: tightly scope C1 to "replace
  the four lines that touch the renderer" without touching anything
  else. Each input branch keeps the same gating, just calls
  `pane.update_*` instead of `core.terrain_renderer.as_mut().unwrap().update_*`.
- **Scene-data API breadth.** The pane needs methods for every kind
  of upload the viewport currently does. Phase A enumerates these
  from `viewport.rs`; if any is missed, C1 surfaces it and adds the
  method. Cheap to extend.
- **Lighting parity.** The viewport reads `app.smf_lighting()` each
  frame; the preview uses `SmfLighting::default()`. The pane's
  `set_lighting` is optional -- not calling it leaves the default.
  Tested explicitly in Phase A: a pane that never receives
  `set_lighting` renders identically to one that received the default.

## What this does NOT do

- It doesn't unify the *application logic* (sculpt brush behaviour,
  feature placement, etc.). Those stay in the viewport as host-level
  code calling pane primitives.
- It doesn't refactor the `bar_render` crate. `TerrainRenderer`,
  `Camera`, and friends stay where they are; the pane is a *user* of
  them.
- It doesn't change shader output. Phase A only moves code around the
  same render path the viewport already runs.

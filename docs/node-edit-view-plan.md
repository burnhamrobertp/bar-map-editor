# Node edit view + live preview plan

## Context

The in-properties-panel canvas (see `docs/node-canvas-editor-plan.md`,
now partly shipped for `LayoutGenerator` and `SplineLayout`) works but
is cramped: the side panel can't comfortably hold a canvas, a control
sidebar, *and* a preview. The editing UX for placed 2D geometry wants
more room and a live look at the resulting heightmap.

This plan covers the next evolution:

1. **A dedicated node-edit view** -- single-click a node for a
   read-only preview in the side panel; double-click (or an "Edit"
   button) descends *into* a full-area bespoke editor, mirroring how
   the canvas already descends into subgraphs.
2. **A live, simplified, real-time heightmap preview** inside that
   edit view -- untextured, unfeatured, low-res, updating as the
   author drags handles.
3. **Free-drawn shapes** -- arbitrary closed/open hand-drawn outlines
   in addition to the fixed primitives, via a fill mode on the spline
   path.
4. **A decision**: merge `LayoutGenerator` + `SplineLayout` into one
   node, or keep them separate. This must be settled first because the
   edit-view UX is built on top of whichever data model wins.

## Decision to settle first: merge or keep separate

Both nodes produce a heightmap from placed 2D geometry, share the same
`symmetry` enum and `mode` (ridge / valley / mask) machinery, and
would share an identical editor (canvas + handles + sidebar + live
preview). The only real difference is the per-item geometry:
`LayoutGenerator` items are ellipse / rectangle / ridge primitives;
`SplineLayout` is one Catmull-Rom curve.

### Option A -- merge into a single `Layout` node (recommended)

One node whose data is an ordered list of **items**, each tagged with
a kind:

- `Ellipse` / `Rectangle` / `Ridge` -- the existing primitives
  (position, size, rotation, falloff).
- `Spline` -- a control-point sequence with open/closed + fill flags.

Shared per-item: `height`/amplitude, `falloff`. Shared per-node:
`symmetry`, output `mode`, optional `mask` input.

Pros:
- One editor, one set of symmetry / mode / preview code.
- Authors compose a layout from mixed primitives + freehand strokes in
  one node instead of wiring several.
- The "enter the node" edit view has a single home.

Cons:
- Unified data model: items become a tagged list. `ParamValue` can't
  cleanly express "list of heterogeneous items" today (it has
  `Spline(Vec<[f32;2]>)` but not a general item list). Likely needs a
  new `ParamValue::Layout(Vec<LayoutItem>)` variant or a JSON-encoded
  string param.
- Bigger migration: existing `LayoutGenerator` / `SplineLayout`
  recipes (including the committed demo `.barproj`s) break. Acceptable
  pre-release per the no-backwards-compat rule, but the demo
  generators must be rewritten.

### Option B -- keep two nodes, share the editor infrastructure

Leave `LayoutGenerator` and `SplineLayout` as separate node types but
factor the edit-view + live-preview into shared code both invoke.

Pros:
- No data-model upheaval; existing recipes/demos keep working.
- Smaller immediate change.

Cons:
- Two palette entries that do conceptually the same thing -- the exact
  "why are there two of these" confusion the node-consolidation audit
  (`docs/TODO.md` era) tried to reduce.
- The "mixed primitives + freehand in one place" authoring story
  needs both nodes wired together with a combiner.

**Recommendation: Option A (merge).** The UX coherence and the
single-editor maintenance win outweigh the one-time data-model +
demo-rewrite cost, and pre-release is exactly when to absorb that. The
rest of this plan assumes a merged `Layout` node; where a step is
merge-specific it's called out.

## The node-edit view

### How "enter" works today (the pattern to mirror)

The canvas already supports descending into a scope:
`crates/bar-gui/src/editor/canvas.rs` defines `CanvasView { Main,
SubGraph(u64) }`; `CanvasState` holds `tabs: Vec<CanvasView>` +
`active_tab`. Double-clicking a group calls `open_or_activate_tab(...)`
(`crates/bar-gui/src/panels/canvas/render.rs`), the canvas filters to
that scope's nodes, and a tab-bar entry (`panels/canvas/tabs.rs`) acts
as the breadcrumb / back-out affordance.

Important: that mechanism is a **filtered graph view**, not a bespoke
editor. The node-edit view is a *new* kind of view -- the area shows a
custom layout editor, not a filtered node graph. But it should reuse
the same tab/navigation shell so "enter / leave" feels consistent.

### Proposed shape

- Add a `CanvasView::NodeEdit(NodeId)` variant alongside `Main` /
  `SubGraph`.
- Single-click a Layout node: side panel shows a **read-only preview
  thumbnail** + an "Edit" button (and the existing quick params).
- Double-click the node, or click "Edit": `open_or_activate_tab(
  CanvasView::NodeEdit(id))`. A tab appears; the central area switches
  from the node graph to the bespoke layout editor for that node.
- The layout editor area is split: **canvas on the left** (the
  existing `properties_canvas` widget, now full-size), **live preview
  on the right**, **item sidebar** below or beside.
- Back out via the tab's close button (same as subgraph tabs). No new
  breadcrumb UI needed.

### Dispatch

`crates/bar-gui/src/panels/canvas/render.rs` currently matches
`current_view()` to decide what to draw. Add a `NodeEdit(id)` arm that
calls a new `draw_layout_editor(id)` instead of the graph renderer.
The existing `Main` / `SubGraph` arms are unchanged.

## Live simplified preview

### Render model

Mirror `FeatureThumbnailRenderer`
(`crates/bar-render/src/thumbnail.rs`): allocate a small offscreen
`Rgba8Unorm` target (e.g. 192² or 256²), render the heightmap with the
existing `TerrainRenderer` in a stripped-down configuration, and
surface it to egui.

Two ways to get the pixels onto egui, in increasing efficiency:

1. **Readback + `ctx.load_texture`** (what the thumbnail renderer does
   today, see `runner.rs`): render to target, copy to a staging
   buffer, read back `Vec<u8>`, upload as an egui texture. Simple, a
   little wasteful (GPU->CPU->GPU roundtrip). Fine for a 256² preview
   that only re-renders on edit.
2. **`egui_wgpu::Renderer::register_native_texture`**: register the
   offscreen wgpu texture view directly as an egui `TextureId`, no
   CPU roundtrip. More efficient; needs access to the egui-wgpu
   render state (we already hold it in `AppRunner::render_state`).

Start with (1) for simplicity; move to (2) only if the preview feels
laggy during drags.

### Stripped-down configuration

The renderer already has the switches:
- `set_low_quality(true)` -- skips shadow pass, planar reflection,
  feature instancing (added earlier this cycle).
- `set_grass_visible(false)`, `set_advanced_map_shading(false)`,
  `set_advanced_model_shading(false)`.
- Feed a flat / neutral albedo so the preview is untextured -- just
  lit geometry showing the heightmap's shape.

### When to re-render

Re-evaluate only the edited node's subtree at preview resolution and
re-render when:
- A gesture mutates the node (handle drag, add, delete), or
- A sidebar param changes (mode, amplitude, width, symmetry, etc.).

Evaluating one Layout node at 128²-256² is cheap (the headless CLI
preview proves the path). Debounce to once per frame; don't re-render
on stationary frames.

### Where it lives

A new `LayoutPreviewRenderer` in `bar-render` (sibling to
`thumbnail.rs`) owning the offscreen target + a `TerrainRenderer`
configured for low-quality. The edit view calls
`render_layout_preview(node_eval_result) -> egui::TextureId` each
frame the preview is dirty.

## Free-drawn shapes (fill mode)

A *closed* spline already draws a free-form outline. To make it a
filled region:

- Add a `fill: bool` (or extend `mode`) so a closed spline rasterises
  its **interior** (point-in-polygon test against the sampled curve
  polygon) rather than only the distance-to-curve band.
- Ridge-fill: interior = `amplitude`, edges feathered by `falloff`.
- Mask-fill: interior = 1.0, edges feathered.
- Open splines keep the current distance-to-curve behaviour (paths /
  ridgelines).

Implementation: after sampling the Catmull-Rom curve to a polygon
(already done in `apply_spline_layout`), a standard even-odd /
winding point-in-polygon test per pixel, combined with the existing
edge-distance falloff for the feather. Modest addition to the
executor function.

In the merged `Layout` node this is just the `Spline` item kind with
`closed = true` + `fill = true`.

## Files to change

### Data model (merge path)

- `crates/bar-graph/src/node.rs` -- new `NodeType::Layout` (or repurpose
  `LayoutGenerator`); new `ParamValue::Layout(Vec<LayoutItem>)` variant
  (or a documented JSON-string encoding) carrying the heterogeneous
  item list. Retire `SplineLayout` + the old indexed shape params.
- `crates/bar-graph/src/defaults.rs`, `param_spec.rs` -- defaults,
  ranges, param_choices, all-types list (variant-count bump).
- Update exhaustive `ParamValue` matches (grep `ParamValue::`; the
  hashing in `engine.rs`, `param_spec::ParamKind::of`, the generic
  panel skip in `properties/mod.rs`).

### Executor

- `crates/bar-engine/src/executor.rs` -- one `apply_layout()` that
  dispatches per item kind (primitive falloff / spline distance /
  spline fill), reusing `expand_symmetric_placements`. Fold in the
  fill-mode point-in-polygon test.
- Tests in `tests/node_coverage.rs` for each item kind + fill +
  symmetry.

### GUI

- `crates/bar-gui/src/editor/canvas.rs` -- `CanvasView::NodeEdit(NodeId)`.
- `crates/bar-gui/src/panels/canvas/render.rs` -- dispatch the new
  view to `draw_layout_editor`; double-click + "Edit" button trigger
  `open_or_activate_tab`.
- `crates/bar-gui/src/panels/canvas/tabs.rs` -- label the NodeEdit tab
  (e.g. node label) so the breadcrumb reads sensibly.
- `crates/bar-gui/src/panels/properties/layout_editor.rs` (new) -- the
  full-area editor: canvas + sidebar + live preview, replacing the
  cramped in-panel `layout_generator.rs` / `spline_layout.rs` (those
  shrink to a read-only thumbnail + "Edit" button in the side panel).
- `crates/bar-gui/src/panels/properties/properties_canvas.rs` --
  unchanged in spirit; it already emits the gestures the editor needs.

### Renderer

- `crates/bar-render/src/layout_preview.rs` (new) -- offscreen
  low-quality heightmap preview renderer, modelled on `thumbnail.rs`.
- Wiring in `crates/bar-app/` to own the preview renderer and feed it
  the node's eval result (mirrors how `feature_thumbs` is owned in
  `AppRunner`).

### Demos / docs

- Rewrite `crates/bar-project/examples/build_symmetric_demos.rs` and
  `build_spline_demos.rs` for the merged node (or add a
  `build_layout_demos.rs` superseding both).
- Update `docs/node-canvas-editor-plan.md` to point here as the
  successor for the editing UX.

## Phasing

The whole thing is large; ship in reviewable slices:

1. **Merge the nodes** (data model + executor + tests + demo
   rewrites). No UX change yet -- the existing in-panel canvas drives
   the merged node. Validates the data model before building UX on it.
2. **Fill mode** for closed splines/freehand. Small, lands on the
   merged executor.
3. **Live preview renderer** (`layout_preview.rs`) + show it in the
   existing side panel first (no edit view yet). De-risks the
   offscreen-render piece independently.
4. **Node-edit view** -- the `CanvasView::NodeEdit` tab + full-area
   editor that hosts the canvas + the (already-working) live preview +
   sidebar. Side panel becomes read-only thumbnail + "Edit".

Each phase is independently testable and revertible.

## Open questions / risks

- **`ParamValue::Layout` vs JSON string.** A typed variant is cleaner
  but ripples through every `ParamValue` match and the serde format.
  A JSON-string param is contained but stringly-typed. Lean typed,
  but size the ripple first (grep `ParamValue::`).
- **Preview camera.** The live preview needs a sensible fixed camera
  (top-down? 3/4 ortho?). Top-down reads closest to the 2D canvas;
  a slight tilt conveys height better. Probably a gentle 3/4 ortho,
  no user camera control in v1.
- **Preview vs canvas coordinate alignment.** Author edits in the 2D
  canvas (normalised top-down); the preview shows a 3D-ish render.
  Keeping them visually correlated (same orientation) matters or the
  author gets disoriented. Top-down or near-top-down preview keeps
  them aligned.
- **Re-eval cost on large maps.** Preview resolution is fixed low
  (128-256), independent of the project's output resolution, so this
  stays cheap regardless of map size.
- **Tab lifetime.** If the author deletes a node while its NodeEdit
  tab is open, the tab must close gracefully (same concern subgraph
  tabs already handle -- check how they cope with group deletion).

## Future work

- Multi-select + group-move of items in the canvas.
- Per-item Bezier handles (sharper road corners) once Catmull-Rom
  proves too smooth.
- Snap-to-grid in the canvas.
- Preview camera orbit toggle for authors who want to inspect height.

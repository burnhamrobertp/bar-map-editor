# 3D-viewport painting

What's already in place, what's being added, and the road ahead.

## Where we start

The 3D viewport already supports **heightmap sculpting**. The wiring:

- `bar-render::pick_terrain` ray-casts the cursor against the terrain
  mesh and returns world-space + heightmap-pixel coordinates.
- `BarEditorApp::is_sculpt_input_active` toggles primary-drag from
  camera-orbit to brush input.
- `bar-app::AppWrapper::apply_sculpt_dab_at_cursor` calls
  `pick_terrain` per drag-event, calls `app.apply_brush_at_heightmap`,
  bumps the inspector's heightmap revision, and re-renders.
- Dabs are persisted onto a `Sculpt` node in the graph so upstream
  re-evals replay them — sculpts compose with parameter changes.

What's **not** yet in place:

- Painting colour onto a `PaintedTexture` from the 3D view.
- Painting a metal map (density per pixel).
- Painting a type map (terrain-type id per pixel).
- Visual brush indicator on the 3D mesh.

## Architecture

The unifying abstraction is a **brush target**: where do dabs go?

```rust
enum BrushTarget {
    Heightmap,   // existing — Sculpt node
    Color,       // PaintedTexture node, RGB grid
    Metalmap,    // PaintedHeightmap with metal role, u8 grid
    Typemap,     // PaintedHeightmap with typeid role, u8 grid
}
```

When the user is in a paint mode AND the cursor hovers the 3D
viewport, a primary drag picks the terrain → gets `(u, v)` in
heightmap-normalised space → routes the dab to the active target's
write-handler. Each handler knows how to apply a brush to its own
pixel grid.

Per-target adapter — **overlay nodes** that take an upstream value
on their input, replay recorded dabs on top, and emit the composite.
Same shape as the existing `Sculpt` node:

| target     | node type                | input  | output | per-dab effect                              |
| ---------- | ------------------------ | ------ | ------ | ------------------------------------------- |
| Heightmap  | `Sculpt` (existing)      | Hm     | Hm     | append `{u, v, ru, s, f, t}` to `dabs`      |
| Color      | `TextureSculpt`          | Color  | Color  | append `{u, v, ru, r, g, b}` to `dabs`      |
| Metalmap   | `MetalSculpt` (planned)  | Hm     | Hm     | append `{u, v, ru, density}` to `dabs`      |
| Typemap    | `TypeSculpt` (planned)   | Hm     | Hm     | append `{u, v, ru, type_id}` to `dabs`      |

Each is **strictly additive overlay** — they don't replace the
upstream pipeline, they composite on top. Reorder upstream noise,
swap an AutoTexture biome, change a metal generator's threshold —
the painted strokes flow through and overlay the new procedural
output. Same composition story as the heightmap `Sculpt`.

The editor's "ensure" path locates the existing source feeding the
right Bundler port (`texture`, `metalmap`, `typemap`) and inserts
the overlay node between source and Bundler. If that source is
already an overlay-node of the matching kind, reuse it. The user
sees the overlay node land on the canvas with a single edge
re-routed; their procedural pipeline is untouched.

## Phasing

### Phase A — Brush target selector (this stage)

- Add `BrushTarget` enum on `BrushState`.
- Inspector toolbar gains a target picker: Heightmap / Color
  / Metalmap / Typemap (Color only enabled in this phase; the other
  two render as disabled with a "soon" tooltip).
- 3D viewport's input dispatch reads the active target and routes
  accordingly. Heightmap target still calls
  `apply_brush_at_heightmap`; the others go through the per-target
  handler.

### Phase B — Color painting end-to-end (this stage)

- New `NodeType::TextureSculpt`: 1 Color input, 1 Color output, a
  `dabs` JSON-string param. Executor reads upstream Color, replays
  every recorded dab, returns the composite. Empty input → no output
  (defensive — colour brushwork only makes sense as overlay).
- `apply_color_brush_at_heightmap(hx, hy)` records a dab onto the
  TextureSculpt; auto-inserts a TextureSculpt between the existing
  `Bundler.texture` source and the Bundler when one isn't already
  there. The ensure pattern is identical to
  `ensure_sculpt_node_for_bundler` for the heightmap side.
- Each dab is stored in normalised UV + normalised radius so a
  resolution change reflows the strokes naturally.
- The graph re-evaluates whenever the dabs param changes (preview
  cache key advances). Upstream goes through TextureSculpt; output
  feeds the Bundler.

### Phase C — Metalmap painting (shipped)

- New `NodeType::MetalSculpt`: 1 Heightmap input, 1 Heightmap output,
  `dabs` JSON-string param. Executor stamps `value` (metal density
  in [0, 1]) into every pixel inside the brush footprint.
- `apply_metal_brush_at_heightmap` records a normalised-UV dab onto
  the MetalSculpt; `ensure_metal_sculpt_for_bundler` either reuses
  an existing MetalSculpt feeding `Bundler.metalmap` or splices one
  between source and Bundler. When no upstream exists, drops a
  `Constant(0.0) → MetalSculpt → Bundler.metalmap` chain so painting
  works from a clean canvas.
- Brush UI exposes a `Density` slider in the inspector toolbar when
  the active target is Metalmap.
- Renderer overlay (subtle red/orange tint where density > 0): not
  yet implemented; Phase E candidate.

### Phase D — Typemap painting (shipped)

- New `NodeType::TypeSculpt`: same shape and executor path as
  MetalSculpt — value-stamp dabs into the brush footprint.
- `apply_type_brush_at_heightmap` + `ensure_type_sculpt_for_bundler`
  mirror the metal variant. Same Bundler chain auto-insertion.
- Brush UI exposes a `Type id (×255)` slider — quantised 0..1 maps
  to the eight terrain types BAR uses; the export pipeline scales
  to u8 at SMF write time.
- Per-type-id colour overlay on the 3D terrain: not yet; Phase E
  candidate.

### Phase E — Real-time visualisation (shipped)

Three pieces ship together:

- **Brush ring on the 3D mesh.** The `CameraUniform` gained a
  `brush_cursor: vec4` slot (xy = world XZ, z = radius, w = active
  flag). `terrain.wgsl`'s fragment shader paints a translucent
  amber annulus on the surface within the brush radius and a
  faint inner disc, so the user sees where the brush will stamp
  before they click. The cursor follows the pointer every frame
  while sculpt mode is active and the cursor is over the viewport;
  disappears as soon as the cursor leaves.
- **Live colour cache.** `BarEditorApp` gained
  `inspector_color_buffer: Option<ColorBuffer>` mirroring the
  existing `inspector_heightmap` pattern. The eval result populates
  it on each high-res pass; colour brush dabs stamp into it
  in-place AND append to the TextureSculpt's `dabs`. The 3D
  viewport pushes the live cache through `frame.texture` per-stroke
  so the user sees the colour land immediately, before the
  background eval re-runs.
- **Live metal / type cache + visualisation.**
  `inspector_metalmap` and `inspector_typemap` mirror the colour
  cache for those layers. Auto-created on first paint (zero-filled
  to the inspector heightmap's dim) so users don't need a
  pre-existing pipeline to start painting. While the brush target
  is Metalmap or Typemap, `bar-app` synthesises a tinted colour
  buffer from the cache and feeds it through `frame.texture` —
  metal renders as a red/orange density gradient, type as a
  per-id colour palette. The actual metalmap/typemap data flows
  through the graph at export time; the visualisation is purely
  for authoring feedback.

## What this stage ships

- Phase A + Phase B.
- Heightmap sculpting unchanged (still works).
- Colour painting via 3D pick → PaintedTexture write.
- Inspector toolbar exposes the target picker.
- Phases C / D / E remain to do, with the per-target adapter
  pattern in place so each is a localised addition.

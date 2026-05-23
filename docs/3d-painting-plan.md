# 3D-viewport painting

What's already in place, what's being added, and the road ahead.

## Where we start

The 3D viewport already supports **heightmap sculpting**. The wiring:

- `bar-render::pick_terrain` ray-casts the cursor against the terrain mesh and returns world-space + heightmap-pixel coordinates.
- `BarEditorApp::is_sculpt_input_active` toggles primary-drag from camera-orbit to brush input.
- `bar-app::AppWrapper::apply_sculpt_dab_at_cursor` calls `pick_terrain` per drag-event, calls `app.apply_brush_at_heightmap`, bumps the inspector's heightmap revision, and re-renders.
- Dabs are persisted onto a `Sculpt` node in the graph so upstream re-evals replay them — sculpts compose with parameter changes.

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

When the user is in a paint mode AND the cursor hovers the 3D viewport, a primary drag picks the terrain → gets `(u, v)` in heightmap-normalised space → routes the dab to the active target's write-handler. Each handler knows how to apply a brush to its own pixel grid.

Per-target adapter — **overlay nodes** that take an upstream value on their input, replay recorded dabs on top, and emit the composite. Same shape as the existing `Sculpt` node:

| target     | node type                | input  | output | per-dab effect                              |
| ---------- | ------------------------ | ------ | ------ | ------------------------------------------- |
| Heightmap  | `Sculpt` (existing)      | Hm     | Hm     | append `{u, v, ru, s, f, t}` to `dabs`      |
| Color      | `TextureSculpt`          | Color  | Color  | append `{u, v, ru, r, g, b}` to `dabs`      |
| Metalmap   | `MetalSculpt` (planned)  | Hm     | Hm     | append `{u, v, ru, density}` to `dabs`      |
| Typemap    | `TypeSculpt` (planned)   | Hm     | Hm     | append `{u, v, ru, type_id}` to `dabs`      |

Each is **strictly additive overlay** — they don't replace the upstream pipeline, they composite on top. Reorder upstream noise, swap an AutoTexture biome, change a metal generator's threshold — the painted strokes flow through and overlay the new procedural output. Same composition story as the heightmap `Sculpt`.

The editor's "ensure" path locates the existing source feeding the right Bundler port (`texture`, `metalmap`, `typemap`) and inserts the overlay node between source and Bundler. If that source is already an overlay-node of the matching kind, reuse it. The user sees the overlay node land on the canvas with a single edge re-routed; their procedural pipeline is untouched.

## Phasing

### Phase A — Brush target selector (this stage)

- Add `BrushTarget` enum on `BrushState`.
- Inspector toolbar gains a target picker: Heightmap / Color / Metalmap / Typemap (Color only enabled in this phase; the other two render as disabled with a "soon" tooltip).
- 3D viewport's input dispatch reads the active target and routes accordingly. Heightmap target still calls `apply_brush_at_heightmap`; the others go through the per-target handler.

### Phase B — Color painting end-to-end (this stage)

- New `NodeType::TextureSculpt`: 1 Color input, 1 Color output, a `dabs` JSON-string param. Executor reads upstream Color, replays every recorded dab, returns the composite. Empty input → no output (defensive — colour brushwork only makes sense as overlay).
- `apply_color_brush_at_heightmap(hx, hy)` records a dab onto the TextureSculpt; auto-inserts a TextureSculpt between the existing `Bundler.texture` source and the Bundler when one isn't already there. The ensure pattern is identical to `ensure_sculpt_node_for_bundler` for the heightmap side.
- Each dab is stored in normalised UV + normalised radius so a resolution change reflows the strokes naturally.
- The graph re-evaluates whenever the dabs param changes (preview cache key advances). Upstream goes through TextureSculpt; output feeds the Bundler.

### Phase C — Metalmap painting

**Planned:** `NodeType::MetalSculpt` graph node storing dabs as a JSON-string param; `ensure_metal_sculpt_for_bundler` auto-splicing it into the graph.

**Actual implementation:** Brush strokes write directly into `SculptState.metal_overlay` (an `Option<Heightmap>` in session state). Persisted as `sculpt-metal.png` sidecar alongside the `.barproj` on every save; reloaded on open. No graph node is created. The `Density` slider is present in the Sculpt3D sidebar. Renderer overlay not yet implemented (pending embedded viewport -- Phase F).

### Phase D — Typemap painting

**Planned:** `NodeType::TypeSculpt` graph node; `ensure_type_sculpt_for_bundler`.

**Actual implementation:** Same pattern as Phase C -- strokes go into `SculptState.type_overlay`, persisted as `sculpt-type.png`. No graph node. The `Value` slider is present in the Sculpt3D sidebar. Per-type-id colour overlay not yet implemented (pending Phase F).

### Phase E — Real-time visualisation (partially shipped)

- **Brush ring on 3D mesh**: shipped. `CameraUniform.brush_cursor` vec4, amber annulus in `terrain.wgsl`.
- **Live colour/metal/type caches**: shipped. `metalmap` / `typemap` on `BarEditorApp::paint` (the `PaintSession` sub-state); synthesised tint while those targets are active.
- **Embedded viewport**: not shipped. The 3D renderer is a floating `egui::Window` launched from the standard layout. The Sculpt3D layout's central panel is a placeholder.

### Phase F — Embedded Sculpt3D viewport (next)

Wire `draw_viewport_on` (currently a floating window in `bar-app/src/main.rs`) into the central panel of the `sculpt3d` layout. Input routing (sculpt drag vs camera orbit) already works in the floating window and transfers as-is. The sidebar controls and brush dispatch are complete. This is the remaining step to deliver the single-screen sculpting experience this plan was designed around.

## What shipped vs. what remains

| Phase | Status |
|-------|--------|
| A -- brush target selector | shipped |
| B -- colour painting end-to-end | shipped |
| C -- metalmap painting | shipped (sidecar model, not graph node) |
| D -- typemap painting | shipped (sidecar model, not graph node) |
| E -- real-time visualisation | partially shipped (caches + brush ring; no embedded viewport) |
| F -- embedded Sculpt3D viewport | **up next** |

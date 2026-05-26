# Spline layout plan

## Context

BME's procedural toolkit covers shapes (`LayoutGenerator` ellipses, rectangles,
ridges) and broad masks (`SlopeMap`, `HeightSelect`, mask family) but has no
way to author **curved linear features**: rivers, road corridors, plateau
edges, ridge lines, dam locations. Today the author has to approximate them
with masks plus noise, which is lossy and tedious.

A new `SplineLayout` node closes this gap. The node takes an ordered set of
control points, fits a Catmull-Rom spline through them, and emits a heightmap
where each pixel's value is determined by its perpendicular distance to the
curve plus a falloff curve.

## Scope

In v1:
- New `ParamValue::Spline` variant carrying ordered 2D control points.
- New `SplineLayout` `NodeType` variant with the params and rendering math.
- Canvas-based point-and-click editing in the Sculpt3D viewport: click to
  add, drag to move, right-click to delete. Active when a SplineLayout node
  is selected and the brush tool is `SplineEdit`.
- `symmetry` param matching the `LayoutGenerator` enum (see
  `docs/symmetric-layout-plan.md`).

Out of v1:
- Cubic Bezier with explicit tangent handles. Catmull-Rom is the v1 default
  because it passes through control points (no separate tangents to author).
- Width-along-spline variation (varying width per control point).
- Tangent-aligned shape extrusion (place trees / S3O models along a spline).
- Spline-to-spline boolean ops.
- A separate 2D top-down spline editor panel (deferred behind the in-viewport
  flow; revisit only if 3D editing proves unwieldy).

## Architecture

### A. `ParamValue::Spline` variant

```rust
pub enum ParamValue {
    Float(f32),
    Int(i32),
    UInt(u32),
    Bool(bool),
    String(String),
    Vec2([f32; 2]),
    /// Ordered list of 2D control points in normalised [0..1, 0..1]
    /// coords. Catmull-Rom spline runs through them in order.
    Spline(Vec<[f32; 2]>),
}
```

Why a dedicated variant rather than JSON-in-string:

- The `Vec2` precedent already establishes vector-shaped param values.
- Editors (the canvas point-and-click flow) need typed access without
  re-parsing JSON on every frame.
- The graph engine's param diffing already works on `ParamValue` -- a
  typed Spline gets dirty-tracked correctly without string-equality
  pitfalls (whitespace, key ordering).

Serde: the `Vec<[f32; 2]>` serialises naturally as a JSON array of
two-element arrays.

### B. `NodeType::SplineLayout`

| Aspect | Value |
|---|---|
| Category | Layout (alongside `LayoutGenerator`) |
| Inputs | `mask: Mask` (optional, multiplies output) |
| Outputs | `output: Heightmap` |
| Params | `points`, `mode`, `amplitude`, `width`, `falloff`, `closed`, `symmetry` |

Param details:

| Name | Type | Default | Description |
|---|---|---|---|
| `points` | `Spline` | empty | Control points in normalised coords. |
| `mode` | `String` | `"ridge"` | `"ridge"` (raise), `"valley"` (carve), `"mask"` (0..1 only). |
| `amplitude` | `Float` | `0.5` | Peak / trough magnitude in [0..1]. Range `[0.0, 1.0]`. |
| `width` | `Float` | `0.05` | Perpendicular falloff radius, normalised to image width. Range `[0.001, 0.5]`. |
| `falloff` | `Float` | `0.5` | Shape parameter for the perpendicular falloff. `0` = boxy, `1` = soft. |
| `closed` | `Bool` | `false` | Closed loop vs open polyline. |
| `symmetry` | `String` | `"none"` | Same enum as `LayoutGenerator`'s upcoming `symmetry` param. |

### C. Catmull-Rom interpolation + closest-point math

Standard centripetal Catmull-Rom segment between control points `P1` and
`P2`, given the surrounding `P0` and `P3`:

```
C(t) = 0.5 * ( (2*P1)
             + (-P0 + P2) * t
             + (2*P0 - 5*P1 + 4*P2 - P3) * t^2
             + (-P0 + 3*P1 - 3*P2 + P3) * t^3 )
```

For open splines the endpoint pseudo-points are reflections (`P0 = P1 + (P1 - P2)`).
For closed splines they wrap around.

Per-pixel evaluation:

1. Sample the curve at `N` evenly-spaced `t` values per segment (`N=32`
   is enough for a 512px-wide map; scale with map width if needed).
2. For each output pixel `(x, y)`, find the nearest sample point.
3. Compute the perpendicular distance `d` (in normalised units).
4. Apply the falloff: `weight = smoothstep(width, width * (1 - falloff), d)`.
5. Output value depends on `mode`:
   - `ridge`: `weight * amplitude`
   - `valley`: `-weight * amplitude` (clamped to `[0, 1]` after compositing with any background)
   - `mask`: `weight`

Optimisation deferred to v2: build a coarse spatial grid of which sample
points cover which output pixels, skip distant ones. v1's brute-force
inner loop is fine for typical control-point counts (<32) and map sizes
(<=2048px squared).

### D. Canvas-based editing UX (in Sculpt3D viewport)

This is most of the implementation cost. The flow:

1. **Node selection**: when a `SplineLayout` node is selected in the
   node graph and the user switches to Sculpt3D, a new `BrushTool::SplineEdit`
   variant becomes available. Toolbar surfaces an icon for it.
2. **Active editing**: while `SplineEdit` is the active tool and a
   SplineLayout node is "in focus" (last-selected SplineLayout):
   - Render an overlay showing the spline as a continuous curve plus
     each control point as a draggable handle (small disc on the ground
     plane).
   - Click on empty ground: ray-cast against the ground plane to get
     world coords, normalise to [0..1, 0..1], append as a new control
     point.
   - Click on an existing handle: begin drag. Drag updates the point's
     coords (re-normalised). Drop ends the drag.
   - Right-click a handle: delete the point.
   - Esc / brush-tool switch: exit edit mode. The spline stays; just
     the overlay handles disappear.
3. **Persistence**: every add / move / delete writes the updated
   `Spline` value back to the node's `params` and marks the node dirty,
   triggering re-evaluation.
4. **Undo**: each operation captures a snapshot (matches the existing
   `crate::undo::Snapshot` flow used for brush strokes), so the user
   can undo a misplaced point without disrupting the whole spline.

### E. Renderer / overlay path

The spline overlay paints on top of the existing viewport. Pattern
matches `crates/bar-gui/src/overlays/metal_spots.rs` (egui-shape-based)
and `sun.rs` (gizmo geometry).

For v1 the overlay is egui Shape primitives projected from world-coords
to screen-coords using the live camera matrix:

- Catmull-Rom samples (already computed by the rasteriser; share the
  buffer) become a polyline `Shape`.
- Each control point becomes a filled circle + outline disc; the
  outline thickens on hover.
- The active drag point gets a different highlight colour.

No new GPU shader work. If the overlay becomes a performance bottleneck
on dense splines (>1000 samples), revisit with a dedicated line-strip
pass; not expected in v1.

## Files to change

### `crates/bar-graph/`

- `src/node.rs`
  - Extend `pub enum ParamValue` with `Spline(Vec<[f32; 2]>)`.
  - Add `NodeType::SplineLayout` variant.
  - Add port definitions for `SplineLayout` in `default_ports()`.
- `src/defaults.rs`
  - Default params block for `SplineLayout` (empty `points`,
    `mode="ridge"`, etc.).
  - Range entry for `SplineLayout`'s float params (`amplitude`, `width`,
    `falloff`).
  - `param_choices` entries for `mode` and `symmetry`.
- `src/param_spec.rs`
  - Register `SplineLayout` in the all-node-types list.

### `crates/bar-engine/`

- `src/executor.rs`
  - New `apply_spline_layout()` function implementing the Catmull-Rom
    + closest-point math.
  - New `get_spline(params, key) -> Option<&[[f32; 2]]>` accessor.
  - `NodeType::SplineLayout` match arm in the main executor switch.
- `tests/node_coverage.rs`
  - Behavioural tests (see "Test plan").

### `crates/bar-gui/`

- `src/panels/palette.rs`
  - Add `("Spline Layout", NodeType::SplineLayout)` to the layout
    section.
- `src/panels/canvas/style.rs`
  - Add `NodeType::SplineLayout` to the layout-category match arm.
- `src/paint/session.rs`
  - Add `BrushTool::SplineEdit` variant + its `label()` arm.
  - Track the currently-focused `SplineLayout` node id on the
    `PaintSession`.
- `src/overlays/spline.rs` (new file)
  - `paint_spline_overlay()` -- projection + egui Shape emission.
  - Hit-test helper `nearest_control_point(world_xy, points, radius) -> Option<usize>`.
- `src/overlays/mod.rs`
  - `pub mod spline;`

### `crates/bar-app/`

- `src/viewport.rs`
  - In the click handler: if active tool is `SplineEdit`, dispatch to
    new `handle_spline_click()` / `handle_spline_drag()` functions.
  - Ground-plane ray-cast helper (extract from existing brush flow
    or share with feature-placement code).
  - Per-frame: when a SplineLayout is focused, call the overlay
    painter from `crates/bar-gui/src/overlays/spline.rs`.

### `crates/bar-graph/src/engine.rs` (or wherever param diff lives)

- Update any exhaustive matches on `ParamValue` to handle the new
  `Spline` variant (most will fall through to a no-op or default).

## Test plan

### Headless (in `crates/bar-engine/tests/node_coverage.rs`)

- `spline_layout_empty_points_emits_zero`: no control points -> output
  is all zero.
- `spline_layout_two_points_ridge_lifts_along_line`: points at
  (0.2, 0.5) and (0.8, 0.5), `mode=ridge`, `amplitude=1.0`,
  `width=0.05` -> peak value along `y=0.5` at midpoint, near-zero at
  `y=0.0` and `y=1.0`.
- `spline_layout_valley_mode_inverts_sign`: same setup, `mode=valley`
  -> output sign / magnitude inverted relative to the ridge case.
- `spline_layout_mask_mode_emits_zero_to_one`: same setup, `mode=mask`
  -> values in `[0, 1]` regardless of `amplitude`.
- `spline_layout_width_controls_perpendicular_falloff`: same two
  points, wider `width` value -> the off-axis pixels at `y=0.5+dy`
  read brighter than with the narrow width.
- `spline_layout_three_point_curve_bends`: control points at corners
  of a triangle -> peak appears at the curved midpoint, not the
  straight-line midpoint between endpoints.
- `spline_layout_symmetry_mirror_x_doubles_spline`: single off-centre
  spline with `symmetry=mirror_x` -> output symmetric across `x=0.5`.

### Serialisation

- `param_value_spline_round_trips`: build a `ParamValue::Spline(...)`,
  serialise via serde, deserialise, assert equality.

### UX (manual, post-implementation)

- Add a `SplineLayout` node to a project, switch to Sculpt3D, select
  `SplineEdit` tool. Click in the viewport: a control point appears
  on the ground plane. Click again elsewhere: a second point appears,
  spline connects them.
- Drag a point: spline updates live.
- Right-click a point: it disappears.
- Save project, reload: spline is preserved.
- Switch back to NodeGraph: the node shows its (read-only) spline
  preview in its properties panel.

## Open questions / risks

- **`ParamValue::Spline` ripple cost**: every exhaustive match on
  `ParamValue` across the codebase needs a new arm. Most will be `_ =>
  None` style or trivial. Pre-implementation, grep for `ParamValue::`
  to size the actual blast radius.
- **Coordinate normalisation under non-square maps**: `width` is
  declared "normalised to image width", but BAR maps are usually
  rectangular. If `width` is interpreted relative to width-only, the
  spline gets stretched on tall maps. Decide whether `width` is in
  normalised-x-units, normalised-y-units, or pixel-equivalents at
  evaluation time. Likely answer: use the *smaller* of width and
  height as the normalisation reference so a circular falloff stays
  circular regardless of aspect.
- **Catmull-Rom endpoint behaviour for open splines**: centripetal
  Catmull-Rom with reflected pseudo-endpoints can produce surprising
  overshoot at the first/last segments. Switch to "natural" endpoint
  tangents (tangent equal to the direction toward the next point) if
  authors complain.
- **Editing UX selection state**: the "focused SplineLayout" needs a
  clear source of truth. Likely: most-recently-selected SplineLayout
  in the node-graph panel, persisted across layout switches. If
  multiple SplineLayout nodes are in a project, only one is editable
  at a time.

## Future work

- **Cubic Bezier with tangent handles** -- doubles the per-point param
  count but unlocks sharp directional control. Revisit if authors find
  Catmull-Rom too smooth for road corridors.
- **Width-along-spline** -- per-point `width_override` so a river can
  taper. Storage shape: `Vec<[f32; 3]>` (`[x, y, width]`) or a
  parallel `widths: Vec<f32>` param.
- **Tangent-aligned feature extrusion** -- place trees / props at
  intervals along a spline, aligned to its tangent. Belongs in the
  feature subsystem (`docs/feature-rendering-plan.md`) more than the
  layout subsystem; cross-link when it lands.
- **2D top-down editor panel** -- a flat XY canvas for spline editing
  outside the 3D viewport. Only worth building if the in-viewport
  3D-projected flow proves clumsy in practice.
- **Spline boolean ops** -- compute intersections / unions of two
  splines (e.g. tributary rivers joining a main river). Deferred until
  there's a concrete map design that needs it.

# Node canvas editor plan

## Context

Two existing / planned node types need 2D spatial editing that the
default indexed-slider properties panel can't serve well:

- **`LayoutGenerator`** today exposes 8 indexed shape slots with 7-8
  sliders each. At `shape_count = 8` the panel surfaces 56+ widgets.
  It's also misleading -- the `shape_count` slider is labelled "Shapes"
  which reads as if it's an input field, and the per-frame undo push
  during slider drags pegs the UI thread on non-trivial maps. Authors
  used to World Machine's 2D top-down layout view find BME's panel
  unusably slow.
- **`SplineLayout`** is the new node planned for curved linear features
  (rivers, roads, ridge lines). It needs ordered control points and a
  Catmull-Rom curve through them. Numerical-only inputs would be
  technically possible but functionally unusable.

Both nodes need the same UX: a 2D normalised [0..1] canvas embedded
in the properties panel, with click-to-add / drag-to-move /
right-click-to-delete interaction, plus a sidebar holding the
selected-item's non-spatial params and any top-level node params.

## Scope

In v1:

- A shared `properties_canvas` widget that handles the canvas chrome
  (pan/zoom, grid background, hit-test plumbing) and emits typed
  events for each gesture.
- A `LayoutGenerator` panel rewrite that consumes the widget. Replaces
  the existing 56-slider grid with a canvas + sidebar.
- A new `ParamValue::Spline` variant.
- A new `NodeType::SplineLayout` with Catmull-Rom evaluation and a
  panel that uses the same widget for control-point editing.
- The per-frame undo bug in the existing LayoutGenerator panel is
  fixed at the same time -- the new panel uses the `field_edit_in_progress`
  snapshot pattern (snapshot on drag start, push on drag stop, one
  undo entry per gesture).

Out of v1:

- Animating handles / hover preview of resize cursors.
- Multi-select (shift-click) for batch move / delete.
- Spline tangent handles (Catmull-Rom passes through points; no
  explicit tangents to author).
- Width-along-spline variation.
- Boolean spline ops.

## Architecture

### Shared canvas widget

Location: `crates/bar-gui/src/panels/properties/properties_canvas.rs`.

The widget is intentionally not generic over item type. It takes:

- A 2D rect to paint into (the calling panel allocates the space).
- Pan/zoom state stored by the calling panel.
- A callback the panel uses to render its own items (the panel knows
  how to draw an ellipse vs a Catmull-Rom curve).
- A callback / event channel for emitted gestures.

What the shared widget owns:

- The pixel <-> normalised coordinate transform (with pan + zoom).
- Background grid rendering at 0.1 and 0.5 step lines.
- Pan via middle-drag.
- Zoom via scroll.
- Empty-space left-click detection (translated to normalised coords).
- Drag-start / drag-move / drag-end tracking when a handle was hit.
- A typed `CanvasGesture` enum the panel matches on.

What the calling panel owns:

- The data (shape list / spline points).
- How to draw each item.
- How to hit-test handles (which handle types exist, where they live
  in canvas space).
- The mapping from `CanvasGesture` to `ParamValue` mutations.
- Undo integration (one snapshot per gesture).

```rust
pub struct CanvasState {
    pub pan: egui::Vec2,        // canvas-px offset
    pub zoom: f32,              // canvas-px per normalised unit
    pub selected: Option<usize>,
    pub drag: Option<DragInProgress>,
}

pub enum CanvasGesture {
    /// Left-click on empty canvas. `pos` is normalised [0..1].
    AddAt { pos: [f32; 2] },
    /// Left-press on an existing handle.
    HandlePressed { item: usize, handle: HandleId, pos: [f32; 2] },
    /// Mid-drag while a handle is held. Panel decides what the new
    /// position means (move centre, resize, rotate).
    HandleDragged { item: usize, handle: HandleId, pos: [f32; 2] },
    /// Drag released.
    HandleReleased { item: usize, handle: HandleId },
    /// Right-click on a handle.
    HandleDeleted { item: usize },
    /// Click on an existing handle without a drag -- panel uses this
    /// to update its selected-item state.
    HandleSelected { item: usize },
}
```

The panel's per-frame loop:

```rust
let mut state = ... // persisted via egui Memory or panel state
let mut handles: Vec<HandleSpec> = collect_handles_from_my_data();
let gestures: Vec<CanvasGesture> =
    properties_canvas::draw(ui, rect, &mut state, &handles, |painter, xform| {
        // Panel draws its items here using `xform` to project
        // normalised coords to pixels.
        draw_my_items(painter, xform, &my_data, state.selected);
    });
for g in gestures {
    apply_gesture_to_my_data(g);
}
```

### Coordinate convention

All item data stays in normalised [0..1, 0..1] (matches the existing
LayoutGenerator and the planned SplineLayout). Aspect-ratio
distortion for non-square maps is handled at evaluation time, not
authoring time -- the canvas always shows a square `[0..1, 0..1]` so
the author works in map-relative coords regardless of the map's
actual aspect.

### Undo integration

The shared widget never touches undo. Each panel wraps its gesture
application in the `field_edit_in_progress` pattern:

- `HandlePressed` -> snapshot the current state to `field_edit_in_progress`
- `HandleDragged` -> mutate params; no undo push
- `HandleReleased` -> push the held snapshot via `push_undo` if it
  exists
- `AddAt` / `HandleDeleted` -> atomic mutations: `push_undo` immediately

Result: one undo entry per drag, not N. Fixes the existing
LayoutGenerator drag-perf issue as a side effect.

## LayoutGenerator panel rewrite

Replaces `crates/bar-gui/src/panels/properties/layout_generator.rs`
entirely.

### Layout

```
+------------------- properties panel -------------------+
| symmetry: [none v]              shape_count: 3         |
|                                                        |
| +------------- 2D canvas (square) ------------+        |
| |                  o                          |        |
| |              .-' | '-.                      |        |
| |             /    |    \                     |        |
| |             |    o    |                     |        |
| |             \         /                     |        |
| |              `------'                       |        |
| +---------------------------------------------+        |
|                                                        |
| Selected: Shape 1 (ellipse)                            |
| type:    [ellipse v]                                   |
| height:  [=========o========] 0.5                      |
| falloff: [=====o============] 0.3                      |
+--------------------------------------------------------+
```

- Canvas takes most of the panel height. It's a square aspect-locked
  area inside whatever rectangular panel egui gives us.
- Shapes draw as their actual silhouette (ellipse / rectangle / ridge
  line) using their `rx_i / ry_i / angle_i`.
- Handles per shape:
  - Centre dot (move).
  - Four corner dots at +/-rx, +/-ry (resize). Corner movement updates
    rx and ry independently so non-uniform stretching works.
  - One rotation handle offset outside the shape along the local +x
    axis (rotate).
- Selected shape draws with a highlight (different stroke colour).
- Click empty space -> append a new ellipse shape at the click
  position with default size; `shape_count` increments.
- Right-click a shape's centre handle -> delete the shape; subsequent
  shape indices shift down so there are no gaps.
- Sidebar below the canvas shows: type dropdown, height slider,
  falloff slider for the selected shape. The two top-level fields
  (`symmetry` and `shape_count` display) sit above the canvas.

### Constraint: 8 shape cap

The current node still caps at 8 shapes. The canvas refuses to add a
9th shape (no-op + a brief toast or just silent). When the cap moves
in the future, the canvas already supports unbounded -- it's a node
constraint, not a UI constraint.

## SplineLayout: new node

### `ParamValue::Spline` variant

```rust
pub enum ParamValue {
    Float(f32),
    Int(i32),
    UInt(u32),
    Bool(bool),
    String(String),
    Vec2([f32; 2]),
    /// Ordered list of 2D control points in normalised [0..1, 0..1].
    Spline(Vec<[f32; 2]>),
}
```

### Node spec

| Aspect | Value |
|---|---|
| Category | Layout |
| Inputs | `mask: Mask` (optional) |
| Outputs | `output: Heightmap` |
| Params | `points`, `mode`, `amplitude`, `width`, `falloff`, `closed`, `symmetry` |

Params (defaults):

- `points`: `Spline(vec![])`
- `mode`: `"ridge"` (other: `"valley"`, `"mask"`)
- `amplitude`: 0.5 (range 0.0..1.0)
- `width`: 0.05 (range 0.001..0.5; normalised to min(w, h))
- `falloff`: 0.5 (range 0.0..1.0)
- `closed`: false
- `symmetry`: `"none"` (same enum as LayoutGenerator)

### Catmull-Rom math

Centripetal Catmull-Rom between consecutive control points. Open
spline endpoints use reflected pseudo-points; closed spline wraps.

```
C(t) = 0.5 * ( (2*P1)
             + (-P0 + P2) * t
             + (2*P0 - 5*P1 + 4*P2 - P3) * t^2
             + (-P0 + 3*P1 - 3*P2 + P3) * t^3 )
```

Per-pixel evaluation:

1. Sample the curve at `N=32` evenly-spaced t per segment.
2. For each output pixel, find the nearest sample's distance.
3. Apply smoothstep falloff: `weight = smoothstep(width, width * (1 - falloff), d)`.
4. Map to output by `mode`.

Symmetry expansion happens before rasterisation: each control point
is multiplied across the symmetry axes, producing additional virtual
splines that are rasterised in turn.

For v1 the symmetry implementation reuses
`expand_symmetric_placements` (the helper added for LayoutGenerator)
on each control point and rasterises the duplicate spline once per
symmetric orbit. Cost is linear in the symmetry multiplier (2x for
mirrors, 4x for rotate_90); fine for typical control-point counts.

### SplineLayout panel

```
+------------------- properties panel -------------------+
| mode: [ridge v]    symmetry: [none v]   closed: [ ]    |
|                                                        |
| +------------- 2D canvas (square) ------------+        |
| |       o                                     |        |
| |        \                                    |        |
| |         o                                   |        |
| |          \____ o                            |        |
| |                                             |        |
| |              o ____ o                       |        |
| +---------------------------------------------+        |
|                                                        |
| amplitude: [=========o========] 0.5                    |
| width:     [=========o========] 0.05                   |
| falloff:   [=========o========] 0.5                    |
+--------------------------------------------------------+
```

- Canvas shows the Catmull-Rom curve as a polyline through sampled t
  values plus each control point as a draggable disc.
- Click empty space -> append a point at the end of the list.
- Drag a point -> move it.
- Right-click a point -> delete it.
- No resize / rotate handles (splines have no per-point size or
  rotation).
- Sidebar params are top-level (no "selected point" sidebar -- each
  point is just a position).

## Files to change

### `crates/bar-graph/src/`

- `node.rs`
  - Add `ParamValue::Spline(Vec<[f32; 2]>)` variant.
  - Add `NodeType::SplineLayout` variant.
  - Add port definitions for `SplineLayout` in `default_ports()`.
- `defaults.rs`
  - `default_params` block for `SplineLayout`.
  - Range entry for SplineLayout floats.
  - `param_choices` entries for `mode` and `symmetry`.
- `param_spec.rs`
  - Add `SplineLayout` to the all-node-types list. Bump
    `EXPECTED_VARIANT_COUNT` to 60.

### `crates/bar-engine/`

- `src/executor.rs`
  - `apply_spline_layout()` -- Catmull-Rom + closest-point + falloff
    + symmetry.
  - `get_spline(params, key) -> Option<&[[f32; 2]]>` accessor.
  - `NodeType::SplineLayout` match arm.
- `tests/node_coverage.rs`
  - Behavioural tests (see test plan).

### `crates/bar-gui/`

- `src/panels/palette.rs` -- add `("Spline Layout", NodeType::SplineLayout)`.
- `src/panels/canvas/style.rs` -- categorise `SplineLayout` as layout.
- `src/panels/properties/properties_canvas.rs` (new) -- shared 2D
  canvas widget.
- `src/panels/properties/layout_generator.rs` -- rewrite to use the
  canvas widget. Adds the symmetry dropdown.
- `src/panels/properties/spline_layout.rs` (new) -- panel for the
  new node, using the same canvas widget.
- `src/panels/properties/mod.rs` -- dispatch SplineLayout to its
  panel; LayoutGenerator dispatch unchanged.

## Test plan

### Headless tests (in `crates/bar-engine/tests/node_coverage.rs`)

- `spline_layout_empty_points_emits_zero`
- `spline_layout_two_points_ridge_raises_along_line`
- `spline_layout_valley_mode_inverts_sign`
- `spline_layout_mask_mode_in_unit_range`
- `spline_layout_width_controls_perpendicular_falloff`
- `spline_layout_three_point_curve_bends`
- `spline_layout_symmetry_mirror_x_doubles_curve`
- `param_value_spline_round_trips_via_serde`

### Manual UX tests (post-implementation)

For LayoutGenerator:

- Add a node. The canvas appears with one default shape at centre.
- Click empty space -> a new shape spawns; `shape_count` ticks up.
- Drag a shape's centre handle -> shape moves smoothly; one undo entry
  per drag.
- Drag a corner handle -> shape resizes asymmetrically.
- Drag the rotation handle -> shape rotates.
- Right-click a centre handle -> shape deletes; subsequent indices
  collapse.
- Change `symmetry` to `mirror_x` -> the canvas shows the original
  shapes plus their mirrors (faded outline).
- Add 8 shapes; verify the 9th add attempt is silently rejected.

For SplineLayout:

- Add a node. Canvas shows an empty 2D area.
- Click empty space three times -> three control points appear,
  connected by a Catmull-Rom curve.
- Drag a point -> curve updates live.
- Right-click a point -> point deletes.
- Toggle `closed` -> the curve closes / opens.
- Change `mode` from `ridge` to `valley` -> the heightmap preview
  flips sign.

## Open questions / risks

- **`ParamValue::Spline` ripple cost.** Every exhaustive match on
  `ParamValue` needs a new arm. Pre-implementation, grep for
  `ParamValue::` to size the actual blast radius; most arms will be
  trivial no-ops.
- **Coordinate normalisation under non-square maps.** Spline `width`
  is relative to "image width" but BAR maps are usually rectangular.
  Use `min(map_w, map_h)` as the normalisation reference so a
  circular falloff stays circular.
- **Drawing the rotation handle without occluding shape detail.**
  Use a minimum pixel offset (radius_px + 12) so the rotation handle
  stays separable from the centre handle on small shapes.
- **Symmetry preview** in the LayoutGenerator canvas. v1 shows
  authored shapes plus their derived mirrors with thinner outlines;
  revisit if it reads as noisy.
- **Pan/zoom state persistence**: per-node? Per-session? v1 is
  per-session, stored on the panel state. Per-node remembering would
  require persistent panel storage which is bigger scope.

## Future work

- Width-along-spline (per-point width override).
- Cubic Bezier with explicit tangents.
- Spline boolean ops.
- Multi-select + group-move in both canvases.
- Snap-to-grid (configurable 0.05 / 0.1 / 0.25 step).
- Hover-preview cursors over resize handles.
- Asymmetric vs uniform-scale drag (shift modifier).

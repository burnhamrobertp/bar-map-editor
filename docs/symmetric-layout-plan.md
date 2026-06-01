# Symmetric layout plan

## Context

BAR maps are overwhelmingly symmetric: 1v1 mirror, 4-corner radial, 90-degree
rotational. Today BME has a single `Mirror` node that performs a *replace*
mirror (the canonical half wins, the other half is discarded), and no support
in `LayoutGenerator` for placing a shape's mirrored copies automatically.

Two small, independent improvements close the gap. They ship as separate
commits so each can be reverted in isolation if needed, and neither blocks
the other.

## Scope

In v1:
- `symmetry` param on `LayoutGenerator` that multiplies each placed shape
  across the chosen axis / rotation.
- New `average_*` modes on `Mirror` that blend the two halves at symmetric
  pixel pairs instead of replacing one with the other.

Out of v1:
- Graph-wide `SymmetryRoot` that propagates symmetry to every downstream
  sampler. Architecturally heavy; defer unless the per-node fixes prove
  insufficient.
- Sculpt-mode symmetric brush (mirrored strokes during hand-painting).
  Lives outside the node graph and is tracked separately.

## Architecture

### A. `symmetry` param on `LayoutGenerator`

One new top-level param on the existing node. Value is an enum-via-string
to match the rest of the codebase:

| Value          | Behaviour |
|----------------|-----------|
| `none`         | Default. Each shape entry produces one copy (current behaviour). |
| `mirror_x`     | Each shape entry produces 2 copies: original + reflection across `x=0.5`. |
| `mirror_y`     | Each shape entry produces 2 copies: original + reflection across `y=0.5`. |
| `mirror_xy`    | 4 copies: original + 3 mirrors (x, y, both). |
| `rotate_180`   | 2 copies: original + 180-degree rotation about (0.5, 0.5). |
| `rotate_90`    | 4 copies: original + 90 / 180 / 270 degree rotations. |

The transform applies in normalised [0..1, 0..1] coords, mirroring the
shape's `x_i` / `y_i` / `angle_i` per copy. For mirrors the per-shape
`angle` is negated; for 90-degree rotations the angle is incremented by
90 / 180 / 270.

### B. Averaging modes on `Mirror`

Extend the existing `mode` param's vocabulary. The existing replace
modes (`mirror_x`, `mirror_y`, etc.) stay; new `average_*` variants
emit the per-pixel mean of the two source positions, preserving
information from both halves:

| Existing mode  | New averaging mode | Behaviour |
|----------------|--------------------|-----------|
| `mirror_x`     | `average_x`        | Output at (x, y) = mean(input(x, y), input(W-1-x, y)). |
| `mirror_y`     | `average_y`        | Same idea across the Y axis. |
| `mirror_xy`    | `average_xy`       | Mean of the four-fold pixel partners. |
| `rotate_180`   | `average_180`      | Mean of (x, y) and (W-1-x, H-1-y). |
| `rotate_90_4way` | `average_90_4way`  | Mean of the four 90-degree-related pixel positions. |

## Files to change

### LayoutGenerator symmetry

- `crates/bar-graph/src/defaults.rs`
  - Add `("symmetry", ParamValue::String("none".to_string()))` to the
    `LayoutGenerator` defaults block (around line 24's special handling).
  - Add the `symmetry` entry to `param_choices` so the GUI surfaces a
    dropdown.
- `crates/bar-engine/src/executor.rs`
  - `apply_layout_generator()` (around line 3037): read `symmetry` param,
    iterate over the symmetric-copy positions per shape, composite each.
  - Helper `fn expand_symmetric(x: f32, y: f32, angle: f32, mode: &str) -> Vec<(f32, f32, f32)>`
    returning the (x, y, angle) tuples for each copy.

### Mirror averaging

- `crates/bar-graph/src/defaults.rs`
  - Extend the `Mirror` entry in `param_choices` to list the new modes.
- `crates/bar-engine/src/executor.rs`
  - `apply_mirror()`: switch on the mode string. Existing replace branches
    stay; add new branches that compute the mean across the symmetric
    pixel pair(s).

### No port / connectivity changes

Both changes are param-only. The graph engine, port-type system, and
node-graph UI need no modification.

## Test plan

In `crates/bar-engine/tests/node_coverage.rs`:

- `layout_generator_mirror_x_symmetry_duplicates_shape`: place a single
  off-centre shape with `symmetry=mirror_x`; assert the output is
  symmetric about `x=0.5` (`h.get(x, y) == h.get(W-1-x, y)` within
  tolerance) and that the original off-centre peak still appears.
- `layout_generator_rotate_90_produces_four_peaks`: place one shape at
  (0.7, 0.5) with `symmetry=rotate_90`; assert peaks appear at the four
  rotated positions.
- `mirror_average_x_preserves_information_from_both_halves`: input is
  `gen(|u, _| u)` (a left-to-right ramp); with `mode=average_x` the
  output mean should equal the input mean (both halves contribute, mean
  is preserved) and the output should be symmetric.
- `mirror_average_xy_collapses_to_mean_of_four`: build an input with
  four distinct values at the four symmetric positions; assert the
  output at all four positions equals the mean.

## Open questions / risks

- **Existing `Mirror` mode names** (`mirror_x`, `rotate_90_4way`) are
  already in saved recipes. Adding more enum values is safe; renaming
  any is not. Plan preserves existing names verbatim.
- **`LayoutGenerator`'s `shape_count` cap** (currently 1-8) interacts
  with symmetry: `symmetry=rotate_90` on 8 shapes produces 32 composited
  shapes per evaluation. This is fine for the rasteriser cost on
  reasonable resolutions; if the cap is ever raised, revisit.
- **Angle handling under symmetry** has multiple plausible conventions
  (mirror an angle: negate vs add-180). The negation form matches what
  "the shape looks like a mirror image" intuitively means; documented
  via tests.

## Future work

- **Graph-wide `SymmetryRoot`**: a node that, when present, signals
  downstream sampling to fold all coords into the canonical region.
  Lets noise + erosion outputs be inherently symmetric without an
  explicit `Mirror` wired in. Architecturally heavy; defer.
- **`symmetry` on `SplineLayout`**: once `SplineLayout` lands
  (`docs/spline-layout-plan.md`), reuse the same enum so river / road
  splines can be mirrored automatically.
- **Sculpt-mode symmetric brush**: independent from the node graph;
  lives in `docs/3d-painting-plan.md` follow-up scope.

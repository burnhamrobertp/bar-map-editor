# Symmetric layout demos

Six minimal `.barproj` projects showing the two new symmetric-layout
features in action: the `symmetry` param on `LayoutGenerator` and the
`average_*` modes on `Mirror`.

## How to use them

- **In the GUI**: open BME, then `File -> Open Project`, pick any of the
  `*.barproj` directories below. Switch to the **Preview** layout (or
  use the Inspector heightmap view) to see the rendered heightmap.
- **From the CLI**: render directly to a PNG without launching the GUI.
  ```bash
  cargo run -p bar-cli --quiet -- run \
      examples/symmetric-layout-demos/2-layout-symmetry-mirror-x.barproj/recipe.json \
      --target raw-layers \
      -o /tmp/symx
  ```
  The output PNG sits at `/tmp/symx/<recipe>_heightmap.png`.

## The demos

All `LayoutGenerator` demos share the same three off-centre ellipse
placements -- the only difference between them is the `symmetry` param.
Comparing the four layout demos side-by-side makes the multiplier
behaviour of each mode obvious.

| Demo | What it shows |
|---|---|
| `1-layout-symmetry-none.barproj` | Baseline: three off-centre ellipses with no symmetry expansion. |
| `2-layout-symmetry-mirror-x.barproj` | Same shapes mirrored across the vertical centre. Six visible blobs in a left/right symmetric arrangement -- canonical 1v1 BAR symmetry. |
| `3-layout-symmetry-mirror-xy.barproj` | Same shapes in 4-quadrant symmetry: original plus three reflections. Twelve blobs total, mirror-symmetric across both axes. Useful for 2v2 corner-symmetric maps. |
| `4-layout-symmetry-rotate-90.barproj` | Same shapes in 4-corner radial symmetry: original plus three 90-degree rotations about the centre. Produces a flower / spoke pattern -- useful for 4-player FFA layouts. |
| `5-mirror-replace-x.barproj` | RidgedNoise -> Mirror with the original `mirror_x` (replace) mode. The right half is **discarded** and replaced with a copy of the left. Existing behaviour pre-symmetric-layout work. |
| `6-mirror-average-x.barproj` | Same RidgedNoise input, but Mirror now uses the new `average_x` mode. Each output pixel is the **mean** of the left/right pair, so information from both halves survives. Compare visually to demo 5 -- the average looks softer and carries detail from the original right half that's missing in the replace version. |

## What to compare

- **Demos 1 -> 4**: how a single shape entry can be duplicated into
  many placements via `symmetry`. Watch the bottom-left ellipse get
  copied to symmetric positions in each mode.
- **Demos 5 vs 6**: the trade-off between *replace mirror* (the
  canonical half wins, the other is lost) and *average mirror* (both
  halves contribute, output is the blend). Compelling for sculpt
  workflows where the author edits both halves and doesn't want the
  Mirror node to throw any of that work away.

## Regenerating these demos

If the schema of `Recipe` changes and the recipe.json files stop
loading, regenerate from the source:

```bash
cargo run -p bar-project --example build_symmetric_demos
```

That re-writes all six `.barproj` directories in this folder.

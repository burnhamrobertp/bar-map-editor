# SplineLayout demos

Seven `.barproj` projects exercising the new `SplineLayout` node and
the shared 2D properties-panel canvas editor.

## How to use them

- **In the GUI**: `File -> Open Project`, pick any `*.barproj`
  below. Select the `SplineLayout` node to see the 2D canvas editor.
  Click to add a point, drag a point to move it, right-click to
  delete. The heightmap updates as you edit.
- **From the CLI**: render a PNG without launching the GUI.
  ```bash
  cargo run -p bar-cli --quiet -- run \
      examples/spline-layout-demos/4-closed-atoll.barproj/recipe.json \
      --target raw-layers \
      -o /tmp/atoll
  ```

## The demos

| Demo | What it shows |
|---|---|
| `1-ridge-curved.barproj` | Four-point S-curve. Ridge mode raises a thin elevation band along the Catmull-Rom curve through the points. The simplest case to scrub for understanding the node. |
| `2-valley-river.barproj` | Same shape in valley mode. Baseline elevation sits at `amplitude`; channel pixels sit at zero. Carved into a flat plane rather than blended with anything. |
| `3-mask-corridor.barproj` | Mask mode emits 0..1 weight along the spline. Useful as a selector that downstream nodes (TextureWeightmap, AutoTexture slope masks) can consume. |
| `4-closed-atoll.barproj` | Closed-loop curve. Five points connect head-to-tail; the curve produces a continuous ridge ring -- an atoll or crater rim. |
| `5-mirror-rivers.barproj` | Single river spline plus `symmetry=mirror_x`. The author edits one river in the left half; the right-side mirror is generated automatically. Canonical 1v1 BAR shape. |
| `6-rotate-radial.barproj` | Short ridge with `symmetry=rotate_90`. The single authored spline produces four radial channels at 90 degrees. 4-player FFA shape. |
| `7-river-in-noise.barproj` | RidgedNoise + SplineLayout (valley) composited via `Multiply`. Shows how a valley spline carves a river through procedural noise rather than sitting on a flat plane. The valley pixels go to zero; the noise terrain stays everywhere else. |

## What to try in the GUI

- **Add a point**: left-click empty canvas. A new point appears at
  the click; the Catmull-Rom curve through all points updates live.
- **Move a point**: left-drag any control point. One undo entry per
  drag (snapshot at drag-start, push at drag-stop).
- **Delete a point**: right-click a control point.
- **Toggle `closed`**: see the curve close head-to-tail.
- **Switch `mode`**: ridge / valley / mask change how the output is
  composited at each pixel.
- **Adjust `width`**: the falloff radius widens or narrows; the
  curve's perpendicular reach changes.
- **Change `symmetry`**: pick `mirror_x` / `rotate_90` etc. to see
  the same control points duplicated across the symmetric orbit.

## Regenerating these demos

If the schema changes and the recipe.json files stop loading:

```bash
cargo run -p bar-project --example build_spline_demos
```

Rewrites all seven `.barproj` directories in place.

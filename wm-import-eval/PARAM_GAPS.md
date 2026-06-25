# Per-parameter WM -> bar-editor gap

Values aggregated across every device instance in all 6 corpus maps (both WM param formats decoded). 'mapped' = a bar param carries it; blank bar column = gap.


## APRL -> PerlinNoise
_283 instances; 5/19 WM params have a bar equivalent_

| WM param | typical value | bar param | note |
|----------|---------------|-----------|------|
| Scale | 0.00098..2.37841 (med 0.03125) | frequency | inverse sense (WM scale vs bar frequency) |
| Persistence | 0.2..0.71875 (med 0.5) | persistence |  |
| Lacunarity | 2.05132 | lacunarity |  |
| Octaves | 0..9 (med 0) | octaves |  |
| Seed | 3..64956 (med 30691) | seed |  |
| Style | 0..8 (med 0) | **gap** | 8 noise styles (Basic/Ridged/Billowy/Smooth/Sharp/Flat/Terraced/+presets); bar has 5 'character' presets on Perlin only |
| Steepness | 0.12875..0.995 (med 0.5) | **gap** | per-noise steepness shaping |
| Elevation | 0.005..0.9 (med 0.5) | **gap** | vertical placement |
| Offset | 0..0.5 (med 0) | **gap** |  |
| Gain | 0.1875..1 (med 0.5) | **gap** |  |
| Shapeguide Power | 1..6 (med 1) | **gap** | shape-guide input weighting |
| Distortion Power | 0.1 | **gap** | built-in domain distortion (bar: separate Warp node) |
| Persistence Guide | 0.5..1 (med 0.5) | **gap** | spatially-varying persistence |
| Multiscale Power | 0..1 (med 0) | **gap** | multiscale synthesis |
| Multiscale Lead-in | 0..1 (med 1) | **gap** | multiscale synthesis |
| Multiscale Type | 0..2 (med 0) | **gap** | multiscale synthesis |
| Specify Height Range | 0..0.5 (med 0.5) | **gap** | output range remap built into the generator |
| Minimum | 0..0.0125 (med 0) | **gap** | output min (bar: separate Clamp) |
| Maximum | 0.00625..0.2 (med 0.2) | **gap** | output max (bar: separate Clamp) |

## CMB2 -> Combine (Blend node)
_763 instances; 2/2 WM params have a bar equivalent_

| WM param | typical value | bar param | note |
|----------|---------------|-----------|------|
| Method | 0..12 (med 2) | Blend.mode | [CLOSED] now an 11-way mode enum on the Combine node (add/subtract/multiply/divide/average/screen/power/difference/max/min/blend) |
| Strength | 0..1 (med 1) | Blend.factor | blend amount; factor lerps a->op(a,b) |

## CLMP -> Clamp
_323 instances; 4/5 WM params have a bar equivalent_

| WM param | typical value | bar param | note |
|----------|---------------|-----------|------|
| Range1 | 0..8.73215e+08 (med 0) | min |  |
| Range2 | 0..1 (med 0.85) | max |  |
| Type | 0..2 (med 0) | **gap** | Rescale/Expand enum -- partially covered by mode |
| Normalize | 0 | Clamp.mode | [CLOSED] mode=normalize |
| Soft Clipping | 0..1 (med 0) | Clamp.mode | [CLOSED] mode=soft_clip |

## BLUR -> Blur
_247 instances; 2/5 WM params have a bar equivalent_

| WM param | typical value | bar param | note |
|----------|---------------|-----------|------|
| Radius | 0..0.08984 (med 0.00297) | radius | WM radius is normalized 0..1; bar radius is in pixels |
| Blur method | 0..1 (med 0) | **gap** | Approximate (fast) / Precise enum |
| Specify radius in | 0 | **gap** | radius unit: percent vs meters |
| Direction | 0 | **gap** | directional / motion blur angle; bar blur is isotropic |
| Isolate masked areas | 0..2 (med 2) | mask | mask input (present) |

## ERD2 -> HydraulicErosion
_14 instances; 4/17 WM params have a bar equivalent_

| WM param | typical value | bar param | note |
|----------|---------------|-----------|------|
| Amount | 20..200 (med 50) | iterations | WM 'Amount' (duration) loosely ~ bar iterations |
| Hardness | 0..0.90476 (med 0.547615) | **gap** | rock hardness / resistance |
| Capacity | 0.30159..0.85714 (med 0.64286) | capacity_factor | [CLOSED] surfaced |
| Filter Type | 0..2 (med 1) | **gap** | erosion filter kernel |
| Filter Strength | 0.01587..0.71429 (med 0.198415) | **gap** |  |
| Method | 0..1 (med 0) | **gap** | erosion algorithm variant |
| Seed | 8453..61568 (med 22630) | seed | [CLOSED] surfaced |
| River Depth | 0.00992..0.09325 (med 0.0496) | **gap** | channel incision depth |
| River Bias | 0..0.5 (med 0) | **gap** |  |
| Multiscale Enable | -8.58994e+08..3.84154e+06 (med 0) | **gap** | multi-resolution erosion |
| Multiscale Bias | -0.18995..0.54362 (med 0.22623) | **gap** |  |
| Multiscale Synthesis | 0..1 (med 1) | **gap** |  |
| Scale Independence | 1 | **gap** | resolution-independent result |
| Preserve Edges | 0..149108 (med 1) | **gap** |  |
| Use Original Mask Style | -256..3.79162e+06 (med 0) | **gap** |  |
| Use Active Masking | 0 | mask | mask input (present) |
| Hardness Map Behavior | 0..3.21229e+06 (med 1) | **gap** | per-pixel hardness input |

## HSEL -> HeightSelect
_169 instances; 5/5 WM params have a bar equivalent_

| WM param | typical value | bar param | note |
|----------|---------------|-----------|------|
| Minimum | 0..1 (med 0.08333) | low |  |
| Maximum | 0..1 (med 0.54167) | high |  |
| Falloff | 0..1 (med 0.03125) | falloff |  |
| Falloff type | 0..2 (med 0) | falloff_type | [CLOSED] linear/smooth |
| Invert Selection | 0..1 (med 0) | invert | [CLOSED] invert toggle added |

## SSEL -> SlopeSelect (new native node)
_166 instances; 5/5 WM params have a bar equivalent_

| WM param | typical value | bar param | note |
|----------|---------------|-----------|------|
| Minimum | 0..90 (med 0) | min_slope | [CLOSED] degrees |
| Maximum | 0..90 (med 19) | max_slope | [CLOSED] degrees |
| Falloff | 0..90 (med 8) | falloff | [CLOSED] degrees |
| Falloff type | 0..2 (med 1) | falloff_type | [CLOSED] linear/smooth |
| Invert Selection | 0..1 (med 0) | invert | [CLOSED] |

## TRCE -> Terrace
_26 instances; 2/4 WM params have a bar equivalent_

| WM param | typical value | bar param | note |
|----------|---------------|-----------|------|
| Number of Terraces | 3..80 (med 37) | step_count |  |
| Terrace Method | 0..2 (med 0) | **gap** | Simple/Sharp/Smooth edge enum |
| Terrace Shape | 0.01..0.99 (med 0.99) | smoothing | loosely ~ smoothing |
| Terrace Layering | 1..5 (med 2) | **gap** | layering mode |

## BIGA -> BiasGain
_120 instances; 2/2 WM params have a bar equivalent_

| WM param | typical value | bar param | note |
|----------|---------------|-----------|------|
| Bias | 0.0005..0.9 (med 0.5) | bias |  |
| Gain | 0.04733..0.9995 (med 0.6) | gain |  |

## EWTH -> ThermalErosion
_9 instances; 2/5 WM params have a bar equivalent_

| WM param | typical value | bar param | note |
|----------|---------------|-----------|------|
| Talus Repose Angle | 33..52.5938 (med 33) | talus_angle |  |
| Talus Production | 0.14062..1 (med 0.9) | **gap** | material production rate |
| Fracture Size | 0.00566..0.01 (med 0.01) | **gap** |  |
| Talus Size | 0.0002..0.001 (med 0.00051) | **gap** |  |
| Simulation Length | 14..66 (med 50) | iterations | loosely ~ iterations |

## WARP -> Warp / Displacement
_45 instances; 1/3 WM params have a bar equivalent_

| WM param | typical value | bar param | note |
|----------|---------------|-----------|------|
| Strength | 0.005..0.01 (med 0.007) | strength |  |
| Direction | 0..270 (med 0) | **gap** | displacement direction/angle |
| Edge Handling | 0..1 (med 1) | **gap** | wrap/clamp at edges |

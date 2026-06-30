# World Machine .tmd -> bar-editor: capability gap analysis

Goal: take real World Machine projects, recreate them in bar-editor, and from
the friction learn what configurability map-makers expect that bar-editor lacks.

Six real BAR maps were used, all authored in World Machine and found in
`~/Downloads` (five by Peter Sarkozy -- a prolific BAR mapper -- and one by
Angelwing):

| Map | Author | Devices | Distinct types | Water level |
|-----|--------|--------:|---------------:|------------:|
| ATG2 ("Altered Terrain Generator") | Peter Sarkozy | 549 | 47 | 0.015 |
| Aethermoor Creek ("Angel Pass") | Angelwing | 614 | 47 | 0.14 |
| onyx2 | Peter Sarkozy | 588 | 47 | 0.078 |
| BSR ("Blindside") | Peter Sarkozy | 561 | 47 | 0.015 |
| cells | Peter Sarkozy | 582 | 48 | 0.015 |
| canis_river | Peter Sarkozy | 737 | 43 | 0.24 |
| **corpus total** | | **3631** | **52** | |

All six are large, production-grade graphs (not toy projects), 549-737 devices
each, drawn from the same ~50-device vocabulary -- strong evidence this is the
*typical* BAR/WM workflow, not one author's idiosyncrasy. The device mix and
coverage are strikingly consistent across all six (TL;DR table).

---

## TL;DR

> **Note:** the numbers below are the gap **as originally found**. A first
> gap-closing pass has since landed (see `CHANGES.md`): device-level "good"
> coverage rose 34% -> 69% and core parameter-level fidelity 31% -> 47%. The
> analysis below is the baseline that drove those changes.

bar-editor can express the **shape** of these graphs but not their **depth**.

Device coverage, weighted by how often each device is actually used, is
near-identical across all six maps (~34% good / ~56% partial / ~9% gap):

| Map | good | partial | gap |
|-----|-----:|--------:|----:|
| ATG2 | 32% | 57% | 11% |
| Aethermoor Creek | 34% | 57% | 9% |
| onyx2 | 34% | 57% | 9% |
| BSR | 33% | 56% | 11% |
| cells | 32% | 57% | 11% |
| canis_river | 39% | 54% | 7% |
| **corpus (3631 devices)** | **34%** | **56%** | **9%** |

By distinct device type (52 across the corpus): **20 good, 19 partial, 13 gap.**

Two layers of "misleading-ness" to see past. First, the ~90% "good or partial"
hides that **partial** is where the pain is: the most-used device (WM Combiner,
763 instances) is "partial" only because bar fragments it into six node types
and lacks several blend modes. Second -- and bigger -- device-level coverage
ignores *parameters*: decoding the actual settings (below) shows bar reproduces
only ~31% of the parameters the core devices expose. The reasons a map-maker
reaches for World Machine -- deep erosion, parametric scalars, rich selectors,
one-node combiners -- are exactly the partial/gap items.

What bar-editor does *better*: it is BAR-native (direct SD7/SMF/SMT export,
mapinfo.lua, metalmap/typemap/start-positions, feature placement, Test-in-BAR),
its Layout node (splines + symmetry) is arguably superior to WM's for symmetric
BAR maps, and it offers hand sculpt/paint layers WM has no concept of.

---

## Method and the tool

`wm-import-eval/` contains a from-scratch reader for the undocumented WM
`TMDFile2` format and the translator:

- `tmd_parse.py` -- the binary format. A `.tmd` is a tree of named `WM` blocks
  (`b"WM" + name_len:u8 + name + size:u64le + payload`). Payloads are either
  child blocks or raw data; some containers carry a u32 count or a
  `FourCC+version[+count]` prefix before their children. Devices live under
  `DEVICEWORLD > Devices > Device2`. Each `Device2` has an `ID` (a 4-char
  FourCC type code, e.g. `BLUR`, `CMB2`), a `basic` field (canvas position +
  name), an `id`, and a `params` blob (`"PAR2"` typed parameters).
- `tmd_model.py` -- turns the tree into device records and extracts the ASCII
  embedded in each `params` blob. Those strings are WM's own tooltips and enum
  labels, so device identities and their option sets are **confirmed, not
  guessed** (e.g. Combiner's method list, Erosion's parameter names).
- `mapping.py` -- the device -> bar-node table with status + rationale.
- `translate.py` -- emits, per map, a `*.coverage.json` (every device mapped)
  and a runnable `*.barproj` recreation, lifting name/author/water/sun from the
  `.tmd`.
- `bar_schema.py` -- validates recipes against bar-graph's ports/params,
  transcribed from `crates/bar-graph/src/node.rs` + `defaults.rs`.

### Caveats (honest limits of this pass)

- **Param values now decoded** (see "Parameter-level fidelity" below and
  `PARAM_GAPS.md`). Concretely, the corpus splits across **two different
  parameter serializations** -- a vivid example of the fragility ROADMAP.md cites:
  - Older WM (ATG2, onyx2, BSR): a flat `PAR2` blob; each param is a fixed record
    starting with marker `50 31 07`, value at +3, name at +59. `par2_decode.py`.
  - Newer WM (Aethermoor, cells, canis_river): no `PAR2` at all; a self-describing
    `ParmGroup` tree of `WM` blocks (`P4FULL` records with explicit `name` /
    `type` / `val` / `min` / `max`). `parmgroup.py`.
  The corpus splits 3/3 across the two families; both are handled and
  `compare.py` aggregates across both. The param *sets* are identical between
  versions (same devices), so the gap holds either way. Float values, names, and
  counts are reliable; a few int/enum/bool fields keep their value in a side slot
  -- enough for the gap analysis, not yet a lossless importer.
- **Wiring not fully reconstructed.** The `Links` block encodes edges as
  device-id pairs but with a variable record stride; partial decode only.
  bar/WM port arities differ anyway, so a 1:1 auto-wire would not yield a valid
  bar graph.
- **Recreations are representative, not 1:1.** A faithful rebuild of a
  549-device graph is neither achievable (see gaps) nor the point. Each
  `.barproj` is a coherent BAR terrain pipeline built from the bar nodes that
  correspond to the map's dominant devices, with identity/water/sun from the
  source. Both **load, validate, evaluate, and export a real BAR map** -- see
  "The recreations" below.

---

## Where the gaps are (ranked by map-maker impact)

Frequencies below are total instances across the 6-map corpus.

### 1. No scalar / parameter graph  (S_GN 35, S_AR 6, I_GN 6)
WM scalars are first-class graph values wired *into device parameters*. One
"Boulder Size" or "Metalspot Distortion" scalar drives many devices; tune once,
everything downstream updates. bar-editor parameters are per-node literals with
no way to share or derive them. This is the deepest structural difference -- it
is what makes WM graphs parametric and reusable. Highest-leverage gap.

### 2. The Combiner is one re-modeable device; bar fragments it  (CMB2 763)
The single most-used device. WM Combiner = one node with a **Method** enum
(Add / Subtract / Multiply / Average / Screen / Power / Max / Min) + Strength
(blend amount) + a mask input. bar splits these across `Add`, `Subtract`,
`Multiply`, `Max`, `Min`, `Blend` and is **missing Average, Screen, Power,
Divide**. Changing a combine mode in WM is a dropdown; in bar it is
delete-node + add-node + rewire. Consolidating combine into one node with a
mode enum (and adding the missing modes) would touch the most-used element of
every graph.

### 3. Erosion is shallow  (ERD2 14, EWTH 9)
Erosion is the headline reason to use WM. WM Erosion exposes Amount, Hardness,
Capacity, Filter Type, Filter Strength, **River Depth**, geological Method, and
Seed; Thermal Weathering exposes Talus Production / Repose Angle / Fracture Size
/ Talus Size / Simulation Length. bar `HydraulicErosion` has iterations +
erosion_rate + deposition_rate; `ThermalErosion` has iterations + talus_angle.
The quality ceiling for erosion-driven terrain is set here.

### 4. Selectors are thinner  (HSEL 169, SSEL 166)
Selection drives every mask; these two total 335 instances corpus-wide. WM
Select Height / Select Slope each do Min/Max range + Falloff + **Falloff type
(curve)** + **Invert** + **Equalization** in one node. bar `HeightSelect` =
low/high/falloff only; there is **no slope-range selector at all** (must chain
`SlopeMap` -> `HeightSelect`), no invert toggle, no falloff curve, no
equalization.

### 5. Clamp lacks Normalize / Soft-Clip  (CLMP 323)
2nd/3rd most-used device. WM Clamp/Restrict offers hard clamp + **Normalize** +
**Soft Clipping** + Type modes in one node. bar `Clamp` is hard min/max;
Normalize is a separate node; there is no soft (smooth) clip.

### 6. No equation / expression node  (EQUA 35)
Arbitrary per-pixel math over inputs -- the escape hatch for anything the fixed
node set can't express. No bar equivalent.

### 7. No channel split / merge  (PRCC 31, PRCS 21)
Split an image into R/G/B or H/S/B and recombine -- used to author
typemap / splat-distribution by packing masks into texture channels. No bar
equivalent. (bar's `TextureWeightmap` covers the splat-blend side but not
general channel ops.)

### 8. Macro system depth  (MACR 327)
2nd most-used "device" (~55 per map). WM macros are reusable subgraphs with
exposed named knobs, instanced from a library. bar has subgraph nesting +
`SubgraphInput/Output` boundaries + "macro presets". The open question is knob
parity: if bar macros cannot expose tunable parameters and be reused across
projects from a shared library, this heavily-used workflow is only partly
served.

### 9. No render / lightmap bake  (PRLM 52)
WM bakes raytraced lightmaps and colored renders to texture (Shadows,
Illumination Model, Direct / Indirect / Raytraced lighting). bar lights at
runtime via its Recoil shader ports. Different model -- bar's runtime lighting
likely covers most needs -- but map-makers bake AO/lightmaps into the BAR
diffuse, which bar has no node for.

### 10. No Switch and no Checkpoint  (SWCH 45, CHKP 57)
`Switch` = N-way manual input selector for A/B testing graph variants.
`Checkpoint` = pin/cache a node's output to skip recompute. Neither exists in
bar. (Progressive preview partly mitigates Checkpoint.)

### 11. Cumulative method/curve enums and presets
Smaller individually, common in aggregate: Blur method (Approximate/Precise),
Terrace method (Simple/Sharp/smooth), Expander Open/Close (bar has only
Expand/Shrink), and **Terrain-Transform presets** (MTRN 101: Canyonize,
Glaciate, Cubic Midlands, Midland Plateau, Smooth HV) -- named geomorphic
shapers bar has no fast path for.

---

## Parameter-level fidelity (decoded)

Device-level coverage (~90% good/partial) flatters bar-editor. Decoding the
actual `PAR2` parameters of the core filter/generator devices shows the real
fidelity is far lower: **of the 72 parameters these 11 device types expose, bar
reproduces 22 (~31%)**. Values in `PARAM_GAPS.md` are medians across every
instance in the corpus (both WM param formats decoded). Uses = corpus totals.

| WM device (corpus uses) | bar node | WM params | bar reproduces |
|-------------------------|----------|----------:|---------------:|
| APRL Advanced Perlin (283) | PerlinNoise | 19 | 5 |
| CMB2 Combiner (763) | Add/Sub/Mul/Max/Min/Blend | 2 | 1 (Method enum is the gap) |
| CLMP Clamp (323) | Clamp | 5 | 2 |
| BLUR Blur (247) | Blur | 5 | 2 |
| ERD2 Erosion (14) | HydraulicErosion | 17 | 2 |
| HSEL Select Height (169) | HeightSelect | 5 | 3 |
| SSEL Select Slope (166) | (none native) | 5 | 0 |
| TRCE Terrace (26) | Terrace | 4 | 2 |
| BIGA Bias/Gain (120) | BiasGain | 2 | 2 |
| EWTH Thermal (9) | ThermalErosion | 5 | 2 |
| WARP Displacement (45) | Warp | 3 | 1 |

The standouts:
- **Erosion (ERD2): 2 of 17.** bar exposes ~iterations + erosion/deposition
  rate; WM exposes Hardness, Capacity, Filter Type/Strength, Method, Seed, River
  Depth/Bias, Multiscale x3, Scale Independence, Preserve Edges, per-pixel
  hardness. This is the widest single gap and the main reason WM exists.
- **Advanced Perlin (APRL): 5 of 19.** Beyond freq/octaves/lacunarity/
  persistence/seed, WM has an 8-way **Style** enum (Basic/Ridged/Billowy/Smooth
  Ridged/Smooth Billowy/Sharp Ridged/Flat Middle/Terraced), Steepness,
  Elevation, Offset, Gain, built-in distortion, spatially-varying persistence
  (Persistence Guide), multiscale synthesis, and an output range remap. bar's
  closest analog is the 5 "character" presets, and only on Perlin.
- **Combiner (CMB2): the one gap is the important one.** `Method` ranges 0..11
  in the data -- WM offers ~12 combine ops (incl. Average, Screen, Power,
  Difference, Divide); bar has 6 split across node types.
- **Slope select (SSEL): 0 of 5.** No native slope-range selector; the WM values
  are in **degrees** (Maximum med 21 deg, Falloff med 9 deg) with a falloff-curve
  type and invert -- none of which survive a SlopeMap->HeightSelect chain.

### Tuning notes (value semantics for matching WM)
- **Noise Scale is normalized and inverse to frequency.** WM APRL `Scale` med
  0.03 (range 0.001..1) is feature size in world fraction; bar `frequency` is
  cycles. A faithful import maps `frequency ~= 1/Scale` (and clamps to bar's
  0.1..32 slider range, which truncates WM's low-frequency end).
- **Blur Radius is normalized** (med 0.003, i.e. 0.3% of map) -- bar `radius` is
  in pixels, so import must multiply by resolution.
- **Angles are in degrees** (SSEL slope, EWTH Talus Repose med 33 deg, WARP
  Direction 0..270) -- bar's slope/aspect params are mixed (SelectAspect uses
  degrees; talus is 0..1). Normalize on import.
- **Terrace count** ranges 3..80 (med ~23) -- bar's `step_count` slider caps at
  32, below what these maps use.

---

## What recreates cleanly (20 device types)

Direct, faithful-enough mappings -- bar is on par or better:

- Noise: Advanced/Basic Perlin -> `PerlinNoise`; Voronoi -> `Voronoi`.
- Generators: Constant Height -> `Constant`; Gradient / Radial Gradient ->
  `Gradient`; File Input -> `FileInput`.
- Filters: Blur -> `Blur`; Bias/Gain -> `BiasGain`; Invert -> `Invert`;
  Terrace -> `Terrace`; Flip -> `Mirror`.
- Analysis: Select Convexity -> `SelectConvexity` (exact: Exposed/Recessed/
  Transition); Select-by-Direction -> `SelectAspect`; Select Height ->
  `HeightSelect`.
- Maps: Normal-Map Maker -> `NormalMap`; Texture Weightmap -> `TextureWeightmap`
  (with priority/weighted blend -- a strong match).
- Output: Output -> `FinalComposition`.
- **Layout Generator -> `Layout`**: bar's spline + primitive + mirror/rotate
  symmetry composition is a strong match and well-suited to symmetric BAR maps
  (up to 20 per map).

---

## The recreations

One `<map>.barproj` per corpus map (6 total) -- 13 nodes / 20 connections each,
**all six passing `bar-cli validate`**. Each reproduces the BAR terrain spine the
source graphs share:

```
PerlinNoise + RidgedNoise -> Blend -> HydraulicErosion -> Clamp
   -> SlopeMap, AutoTexture, NormalMap, SpecularMap, GrassMap,
      Layout (metal), HeightSelect (type) -> FinalComposition
```

with `min_height` / sun direction / author / water taken from each `.tmd`.

Confirmed end-to-end with the real `bar-cli` (built from this tree):

```
bar-cli validate           -> all 6 valid (13 nodes, 20 connections, 13-step eval order)
bar-cli run --target spring-smf  (ATG2, Aethermoor exported as the 2 worked examples)
  ATG2             -> maps/atg2.smf + atg2.smt + mapinfo.lua  (51s, 120k-iter erosion ran)
  Aethermoor_Creek -> maps/aethermoor_creek.smf + .smt + mapinfo.lua  (51s)
```

Rendered heightmap + diffuse side-outputs in `out/` show genuine terrain (Perlin
base + ridged ridges + erosion) with the temperate-biome texture applied by
elevation/slope. The recreations do **not** reproduce each source's 549-737
devices (impossible given the gaps above) -- `*.coverage.json` is the complete,
per-device record, per map, of what would and would not carry over.

---

## Recommendations (prioritized)

Status tags reflect the implementation in `CHANGES.md` (verified via
`bar-cli` + `cargo test`). DONE = shipped this pass; the rest remain.

**P0 -- structural, highest leverage**
1. **[DONE]** Consolidate combine into one node with a Method enum + the missing
   modes (Average, Screen, Power, Divide). The `Blend` node ("Combine") now has
   an 11-way `mode` with WM "Strength" semantics.
2. **[PARTIAL]** Deepen erosion. Surfaced 7 previously-hidden CPU params
   (capacity/inertia/evaporation/gravity/radius/lifetime/seed). Still to do:
   distinct Hardness/River-Depth/Method and GPU-path (shallow-water) parity.
3. Scalar parameters as graph values (a `Scalar` source + scalar math, wired
   into node params). Enables parametric graphs; the biggest architectural lift.

**P1**
4. **[DONE]** New `SlopeSelect` node (slope range in degrees) + `invert` and
   `falloff_type` added to `HeightSelect`.
5. **[DONE]** Clamp: `normalize` and `soft_clip` modes.
6. Equation / expression node.
7. Confirm (or build) macro knob exposure + a cross-project macro library.
8. Noise shaping on the generators (APRL is 5/19): a Style enum folding
   Ridged/Billowy/Smooth/Sharp/Terraced into one node, plus Steepness / Elevation
   / Offset / Gain / built-in distortion.

**Import-fidelity note (slider ranges that truncate WM): [DONE]** noise
`frequency` widened 0.1..32 -> 0.1..128 and Terrace `step_count` 32 -> 80 to
cover the corpus's effective values. Remaining range work travels with each
future param addition.

**P2**
8. Channel split / merge nodes (R/G/B, H/S/B).
9. Switch node; explicit Checkpoint/cache node.
10. Method enums on Blur / Terrace; Open/Close on the morphological nodes.
11. Terrain-transform preset node (Canyonize / Glaciate / ...).

---

## Appendix: full device mapping

`good` = faithful; `partial` = maps but loses configurability; `gap` = no
equivalent. 52 device types across the corpus: 20 good, 19 partial, 13 gap.

| FourCC | WM device | bar node | Status |
|--------|-----------|----------|--------|
| CMB2 | Combiner | Add/Subtract/Multiply/Max/Min/Blend | partial |
| MACR | Macro | Subgraph nesting + presets | partial |
| CLMP | Clamp / Restrict | Clamp | partial |
| APRL | Advanced Perlin | PerlinNoise | good |
| BLUR | Blur | Blur | good |
| HSEL | Select Height | HeightSelect | good |
| SSEL | Select Slope | SlopeMap + HeightSelect | partial |
| FILI | File Input | FileInput | good |
| CHOS | Chooser | MaskSelect | partial |
| PRBO | Bitmap Output | FinalComposition channel | partial |
| MTRN | Terrain Transform | Curve / BiasGain | partial |
| BIGA | Bias / Gain | BiasGain | good |
| IVRT | Invert | Invert | good |
| CHKP | Checkpoint | (none) | gap |
| COG2 | Channel combiner gen2 (unconfirmed) | (none) | gap |
| PRLM | Render / Lightmap | (none) | gap |
| LAYG | Layout Generator | Layout | good |
| SWCH | Switch | (none) | gap |
| TRCE | Terrace | Terrace | good |
| OUTP | Output | FinalComposition | good |
| PRCC | Channel Splitter | (none) | gap |
| PRCS | Channel Combiner | (none) | gap |
| EQUA | Equation | (none) | gap |
| OLVW | Overlay / Colorizer | LayerBlend / ColorRamp | partial |
| WARP | Displacement | Warp / Displacement | partial |
| S_GN | Scalar Generator | (none) | gap |
| CELL | Voronoi | Voronoi | good |
| CHGT | Constant Height | Constant | good |
| EXPA | Expander | MaskExpand / MaskShrink | partial |
| ERD2 | Erosion | HydraulicErosion | partial |
| PULL | Pull-up / Override | Max / Min / MaskApply | partial |
| ANOI | Add Noise | PerlinNoise + Add | partial |
| SELU | Select Color | (none) | gap |
| FLIP | Flip | Mirror | good |
| SELF | Select by Direction | SelectAspect | good |
| CGRD | Radial Gradient | Gradient | good |
| PRNO | Normal-Map Maker | NormalMap | good |
| BPRL | Basic Perlin | PerlinNoise | good |
| NORM | Texture Weightmap | TextureWeightmap | good |
| S_AR | Scalar Arithmetic | (none) | gap |
| I_GN | Integer Generator | (none) | gap |
| SELC | Select Convexity | SelectConvexity | good |
| GRAD | Gradient | Gradient | good |
| EWTH | Thermal Weathering | ThermalErosion | partial |
| C_DG | Distortion Generator | Warp | partial |
| COLG | Color Generator | PaintedTexture / ColorRamp | partial |
| HSPL | Height Splitter | HeightSelect (xN) | partial |
| C_GN | Coordinate/Transform Gen | Transform / Mirror | partial |
| lvls | Levels | BiasGain / Curve | partial |
| CURV | Curve | Curve | good |
| COER | Coast Erosion | (none) | gap |
| SCVW | Scene View (view-only) | (none) | gap |

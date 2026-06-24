# Gap-closing changes: WM -> bar-editor

Implementation of the highest-leverage gaps from `REPORT.md` / `PARAM_GAPS.md`.
Each change is fully wired: param defaults + validator schema (derived from
`default_params`), GUI widget + value range (data-driven from
`param_float_range`/`param_uint_range`/`param_choices`), and executor behaviour.
Value ranges are set to WM's *effective* range translated into bar's units.

## What changed (by gap, ranked by corpus usage)

### 1. Universal Combine modes  (CMB2, 763 uses -- the most-used device)
The `Blend` node (palette label now "Combine") gained a **`mode`** enum:
`blend, add, subtract, multiply, divide, average, screen, power, difference,
max, min`. The result is `lerp(a, op(a,b), factor)`, so `factor` is WM's
**Strength** (blend amount) and `mode="blend"` reproduces the old behaviour
exactly. Closes the "bar fragments combine and lacks Average/Screen/Power/Divide"
gap in one node, without disturbing the existing Add/Subtract/... nodes.
- `defaults.rs`: `mode` default + `param_choices`.
- `executor.rs`: `combine_mode_heightmaps` (replaces `blend_heightmaps`).

### 2. Clamp modes: normalize + soft-clip  (CLMP, 323 uses)
`Clamp` gained a **`mode`** enum: `clamp` (hard, default), `normalize` (rescale
the input's actual range to fill [min,max]), `soft_clip` (smooth tanh saturation).
- `defaults.rs`: `mode` default + choices. `executor.rs`: `apply_clamp_mode`.

### 3. HeightSelect: invert + falloff curve  (HSEL, 169 uses)
Added **`invert`** (bool) and **`falloff_type`** (`linear`/`smooth`) -- the two
WM "Select Height" controls bar lacked. `compute_height_select` now takes a
`smooth` flag (smoothstep ramp); invert applies `1-v`.

### 4. New SlopeSelect node  (SSEL, 166 uses -- was 0/5 reproducible)
A native slope-range selector (WM "Select Slope"): inputs heightmap, selects the
band between **`min_slope`/`max_slope`** with **`falloff`**, all in **degrees**
(0..90, matching WM's units; mapped onto bar's normalised slope), plus
`falloff_type` and `invert`. Previously required a SlopeMap->HeightSelect chain
that couldn't express invert/falloff-curve. Full new-node wiring: enum variant,
ports, defaults, ranges, palette (category + search), executor arm, canvas
category colour, param-spec count.

### 5. Expanded erosion params  (ERD2, 14 uses -- was 2/17)
Surfaced 7 HydraulicErosion params the executor already read but the UI never
exposed: `capacity_factor`, `inertia`, `evaporation_rate`, `gravity`,
`erosion_radius`, `max_lifetime`, `seed`. Zero executor change; defaults match
the previous fallbacks so existing recipes are unaffected. Takes the CPU erosion
node from 3 exposed params to 10.
(Note: the GPU `hydraulic_flow_erode` path uses a different shallow-water model
and reads only some of these; CPU/`bar-cli` is the faithful path. GPU parity is
follow-up shader work.)

### 6. Value ranges that were truncating WM
- Noise `frequency` slider 0.1..32 -> **0.1..128** (WM's low "Scale" -> high
  bar frequency, ~1/Scale, exceeded 32).
- Terrace `step_count` 2..32 -> **2..80** (WM "Number of Terraces" reaches 80).
- New ranges for the erosion + slope-select params (degrees for slope).

## Files touched
- `crates/bar-graph/src/defaults.rs` -- params, choices, ranges.
- `crates/bar-graph/src/node.rs` -- `SlopeSelect` variant + ports.
- `crates/bar-graph/src/param_spec.rs` -- variant count 59 -> 60 + test list.
- `crates/bar-engine/src/executor.rs` -- combine modes, clamp modes, select
  invert/smooth, SlopeSelect arm; removed now-dead `blend_heightmaps`/`apply_clamp`.
- `crates/bar-gui/src/panels/palette.rs` -- "Combine" relabel, SlopeSelect entry.
- `crates/bar-gui/src/panels/canvas/style.rs` -- SlopeSelect category colour.

## Verification (all passed)
- `cargo check --workspace`: clean (the executor's exhaustive `match` and the
  param-spec variant-count test forced every new-node touchpoint to be wired;
  the compiler caught a missing canvas-style arm, now fixed).
- `cargo test -p bar-graph`: 15 passed / 0 failed -- includes the variant-count
  guard (now 60) and `defaults_validate_clean` (every node's defaults validate
  against the schema derived from them).
- `bar-cli validate feature-test.barproj`: valid (9 nodes, 10 connections).
- `bar-cli run --target spring-smf feature-test.barproj`: evaluated all 9 nodes
  (Combine/screen, Clamp/normalize, HeightSelect invert+smooth, the new
  SlopeSelect, expanded HydraulicErosion, freq-64 Perlin, 60-step Terrace) and
  exported `wm-gap-feature-test.smf/.smt/mapinfo.lua`.

## Impact: the gap before -> after (measured on the 6-map corpus)

Re-ran `corpus.py` + `compare.py` with the mapping updated to credit the new
capabilities. Tests: `cargo test -p bar-engine` = 210 passed / 0 failed
(includes 10 new behavioural tests); `cargo test -p bar-graph` = 15 / 0.

Device-level coverage (3631 device instances, weighted by use):

| | good | partial | gap |
|--|-----:|--------:|----:|
| before | 34% | 56% | 9% |
| **after** | **69%** | **22%** | **9%** |

CMB2 (763), CLMP (323) and SSEL (166) moved partial->good; ~1250 device
instances (34% of the corpus) reclassified.

Parameter-level fidelity on the core devices (sum of reproduced / total params):

| device (corpus uses) | before | after |
|----------------------|-------:|------:|
| CMB2 Combiner (763)  | 1/2 | **2/2** |
| CLMP Clamp (323)     | 2/5 | **4/5** |
| HSEL Select Height (169) | 3/5 | **5/5** |
| SSEL Select Slope (166)  | 0/5 | **5/5** |
| ERD2 Erosion (14)    | 2/17 | **4/17** |
| (APRL/BLUR/TRCE/BIGA/EWTH/WARP unchanged) | | |
| **core total** | **22/72 (31%)** | **34/72 (47%)** |

So the new params lifted core parameter-level fidelity from ~31% to ~47%, and
device-level "good" coverage from ~34% to ~69%. The biggest single win is SSEL
(Select Slope), which went from entirely unreproducible (0/5) to fully covered
by the new SlopeSelect node. The gap (9%) -- devices with no equivalent at all
(Equation, Switch, Checkpoint, channel split/merge, Render, scalar nodes,
Coast Erosion, Scene View) -- is unchanged; closing it needs new subsystems.

## Second pass: registry refactor + 9 subsystems (DONE)

A follow-up effort (a) refactored the whole node system onto a per-node
descriptor/registry pattern and (b) built the remaining WM-gap subsystems.

**Architecture refactor (all 60 -> now 70 node types):**
- Every node = one descriptor file `crates/bar-graph/src/nodes/<family>/<node>.rs`
  (`pub static DEF: NodeDef`) + one executor file
  `crates/bar-engine/src/exec/<family>/<node>.rs` (`pub fn exec`), grouped in
  family directories. A `LazyLock` REGISTRY + `EXEC` table key them by NodeType;
  a coherence test pairs descriptor<->executor.
- The 830-line `executor.rs` dispatch match, the `node.rs` `default_ports` match,
  the `defaults.rs` param matches, and the duplicated palette `all_nodes` list
  are all deleted. palette / canvas-colour / properties-panel / validation now
  read the registry. Adding a node = its two files + the family index; nothing
  scattered.

**Subsystems built (each: node(s) + executor + tests, all green; several add
new bar-compute ops / a dependency / an eval-contract change):**
- Channel split/merge (ChannelSplit/ChannelMerge + ColorBuffer::channel/from_channels)
- Checkpoint (passthrough), Switch (N-way, runtime-resize + panel)
- Equation (evalexpr per-pixel formula + formula-editor panel)
- CoastErosion (new bar-compute CPU op)
- LightmapBake (new bar-compute horizon-AO + sun-ray-march op, CPU + GPU pipeline)
- Deeper HydraulicErosion (hardness input map, river_depth, method enum)
- Noise shaping (steepness/elevation/offset/gain on the FBM nodes, CPU+GPU+WGSL, no-op at defaults)
- Scalar parameter graph (ScalarValue/ScalarMath/IntValue + scalar-wire param
  binding: a `scalar_bindable` param gets an auto-exposed Scalar input port, and
  `eval.rs` folds a connected scalar over the literal before execute)

**Gap after this pass (corpus, 3631 device instances):**

| | good | partial | gap |
|--|-----:|--------:|----:|
| originally | 34% | 56% | 9% |
| **now** | **75%** | **23%** | **1%** |

Only 3 device types remain unaddressed: SELU (colour selection), COG2 (identity
never confirmed), SCVW (Scene View -- view-only, no terrain output, intentionally
skipped). Verified end-to-end: `cargo test --workspace` green; `bar-cli run` on
`subsystems-test.barproj` (scalar-driven Perlin freq + Equation + CoastErosion +
AutoTexture + LightmapBake + ChannelSplit/Merge) exports a real `.smf/.smt/mapinfo.lua`.

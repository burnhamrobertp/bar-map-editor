---
applyTo: "crates/bar-engine/**"
---

# bar-engine — Node Executors, Bundler Pipeline & SD7 I/O

## Role
`bar-engine` is the integration layer that wires together the graph, compute
pipelines, and file formats into runnable workflows. It provides the
`NodeExecutor` implementations used during evaluation, orchestrates the bundler
export pipeline, and handles SD7 archive extraction/import. It is used by both
`bar-app` (interactive) and `bar-cli` (headless). `bar-engine` also re-exports
`bar-project` types as a convenience façade so callers only need one import.

## Responsibilities
- Implement `NodeExecutor` as `CpuExecutor` (all operations in software) and
  `HybridExecutor` (dispatches noise, erosion, and blur to GPU via
  `bar-compute` when pixel count exceeds a configurable threshold, falls back to
  CPU otherwise).
  Key helpers in `executor.rs`: `generate_rock_soil`, `generate_vegetation`,
  `generate_texture_overlay`, `get_input_color` (reads a `PortValue::Color` input),
  alongside existing `generate_auto_texture`, `compute_slope_map`, `compute_local_ao`,
  `apply_color_modulation`, `parse_hex_color_srgb`.
- Orchestrate the **bundler export pipeline**: after `evaluate_graph`, collect
  the `LayerSet` (heightmap, metalmap, typemap, texture, normalmap, grassmap,
  specular) from the Bundler node's input connections, invoke the matching
  `ExportCodec`, stage output files, copy `FileRef` pass-through files, then
  package into an archive (7z / zip / directory).
- Own the **target system**: `TargetRegistry` maps codec/target IDs to
  `TargetConfig`; `ExportCodec` trait + `SpringSmfCodec` implement Spring
  SMF+SMT output.
- Orchestrate **SD7 extract** (`extract_sd7_to_work_dir`) and **import**
  (`import_sd7`, `import_sd7_to_project`): unpack archives into an
  app-controlled cache directory (never alongside the source archive), read
  SMF/SMT, build a `Project` from terrain data, return a `WorkDirScan`. Work
  dirs live under `<ProjectDirs cache>/OpenMachine/work/<stem>_<hash>/`, keyed
  by the canonical archive path so re-opens are stable. `prune_old_work_dirs`
  trims entries older than a configurable age and is invoked from `bar-app`
  startup.
- Provide standalone export helpers: `export_smf`, `export_smt`,
  `export_heightmap_png`, `export_normalmap_png`, `export_grassmap_png`,
  `export_texture_png`, `export_sd7_directory`, `export_with_target`.

## Data Ownership
Executors are stateless (or borrow `Arc<GpuContext>`); they produce values
consumed by `evaluate_graph`. The bundler pipeline owns transient staging
directories during execution. No long-lived mutable state.

## Key Public Types
| Type | Description |
|---|---|
| `CpuExecutor` | `NodeExecutor` impl fully in software |
| `HybridExecutor` | `NodeExecutor` impl with GPU acceleration and CPU fallback |
| `BundlerResult` | `node_id, label, output_path, files_written` |
| `TargetRegistry` | Registry of named export targets |
| `TargetConfig` | Codec ID, name, version, layers, packaging config |
| `ExportCodec` | Trait: `validate`, `compute_dimensions`, `write` → `WrittenFiles` |
| `SpringSmfCodec` | `ExportCodec` impl for Spring SMF+SMT |
| `ExportPlan` | `map_name, dimensions, settings (MapSettings)` |
| `WrittenFiles` | Paths written by an export pass |
| `ImportResult` | Result of an SD7 import |
| `WorkDirScan` | Re-export of `bar_project::WorkDirScan` |
| `find_bundler_nodes`, `execute_bundlers` | Orchestration helpers |
| `extract_sd7_to_work_dir`, `import_sd7`, `import_sd7_to_project` | SD7 I/O |

Re-exports from `bar-project`: `Recipe`, `Project`, `EditorLayout`,
`MapSettings`, and related types.  
Re-exports from `bar-graph`: `NodeType`, `ParamValue`.

## Interaction Surface
**Calls into:**
- `bar-data` — SMF/SMT I/O, `Heightmap`/`ColorBuffer` pixel types
- `bar-compute` — `GpuNoisePipeline`, `GpuErosionPipeline`, `GpuFilterPipeline`
  in `HybridExecutor`; CPU noise + rayon in `CpuExecutor`
- `bar-graph` — `evaluate_graph`, `NodeExecutor` trait, output-accessor helpers
- `bar-project` — `Recipe`/`Project` load/save, `MapSettings`, `WorkDirScan`
- `zip`, `sevenz-rust`, `walkdir` — archive operations

**Exposes to callers (`bar-app`, `bar-cli`):**
- `CpuExecutor` / `HybridExecutor` (pass to `evaluate_graph`)
- `execute_bundlers`, `find_bundler_nodes`
- Export function family
- `extract_sd7_to_work_dir`, `import_sd7_to_project`
- `TargetRegistry`

## Boundaries — What This Crate Must NOT Do
- Must not depend on `bar-render`, `bar-gui`, `bar-app`, or `bar-cli`.
- Must not contain any egui or windowing code.
- Must not block the UI thread — all heavy work should be designed to be called
  from background threads by `bar-app`.
- `HybridExecutor` may hold an `Arc<GpuContext>` but must not create a wgpu
  device from scratch — that is `bar-compute`'s and `bar-app`'s responsibility.
- Do not add new `ExportCodec` impls without a corresponding entry in
  `TargetRegistry`.

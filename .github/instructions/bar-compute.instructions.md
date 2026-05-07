---
applyTo: "crates/bar-compute/**"
---

# bar-compute — GPU & CPU Compute Pipelines

## Role
`bar-compute` owns GPU-accelerated and CPU-fallback implementations of the
compute-intensive terrain algorithms (noise generation, erosion, blur). It also
creates and holds the shared GPU device handle used across the whole application.
It knows nothing about the graph, the UI, or file formats.

## Responsibilities
- Create and expose `GpuContext` — a clonable `Arc<Device>` + `Arc<Queue>`
  wrapper — that the rest of the stack shares. `GpuContext::from_existing`
  attaches to an already-running wgpu device (eframe's).
- Run noise generation (Perlin, Simplex, Ridged, Worley) via WGSL compute
  shaders (`GpuNoisePipeline`) or via the `noise` crate + Rayon on the CPU.
- Run hydraulic erosion (virtual-pipe flow model, `FlowErosionParams`) and
  thermal erosion on the GPU (`GpuErosionPipeline`).
- Run box-blur and future filter operations on the GPU (`GpuFilterPipeline`).
- Define the typed parameter structs (`NoiseParams`, `HydraulicErosionParams`,
  `ThermalErosionParams`, `FlowErosionParams`) that callers pass in.

## Data Ownership
Pipelines own transient GPU buffers (staging, storage, uniform) that exist only
for the duration of a single dispatch. After dispatch they return an owned
`bar_data::Heightmap` to the caller and hold no further state. `GpuContext` is
shared-ownership via `Arc`; `bar-compute` is the point of creation only.

## Key Public Types
| Type | Description |
|---|---|
| `GpuContext` | Shared GPU device + queue; `from_existing(device, queue)` |
| `ComputeDevice` | Device initialisation helpers |
| `GpuNoisePipeline` | GPU noise generator; `generate(params, w, h) -> Heightmap` |
| `GpuErosionPipeline` | GPU hydraulic + thermal erosion |
| `GpuFilterPipeline` | GPU box blur and filters |
| `NoiseParams` / `NoiseType` | Noise configuration |
| `HydraulicErosionParams` | Droplet erosion parameters |
| `ThermalErosionParams` | Talus-angle erosion parameters |
| `FlowErosionParams` | Virtual-pipe erosion parameters |
| `ComputeError`, `GpuErosionError`, `GpuFilterError` | Error enums |

## Interaction Surface
**Calls into:** `bar-data` (takes `&Heightmap`, returns owned `Heightmap`);
`wgpu` for compute dispatch; `noise` crate + `rayon` for CPU fallback.  
**Exposes to callers (`bar-engine`, `bar-app`):**
- `GpuContext::from_existing` — GPU context creation
- Pipeline `generate` / `erode` / `blur` methods — single-shot async-free compute

## Boundaries — What This Crate Must NOT Do
- Must not depend on `bar-graph`, `bar-engine`, `bar-project`, `bar-gui`,
  `bar-render`, `bar-app`, or `bar-cli`.
- Must not read or write disk files (no SMF, no archives, no project files).
- Must not render anything to screen; it owns compute pipelines, not render
  pipelines.
- Must not contain graph evaluation logic or node dispatch tables.

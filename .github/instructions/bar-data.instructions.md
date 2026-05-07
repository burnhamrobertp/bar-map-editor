---
applyTo: "crates/bar-data/**"
---

# bar-data — Primitive Data Types & File I/O

## Role
The lowest layer of the stack. `bar-data` owns the canonical in-memory pixel
buffer types and all Spring engine binary file formats. Every other crate that
touches pixel data imports its types from here. It has no knowledge of the
graph, the UI, or any compute pipeline.

## Responsibilities
- Define and implement `Heightmap` (2-D `f32` grid, normalised 0–1) and
  `ColorBuffer` (2-D RGBA `f32` grid) — the two primitive data types that
  flow throughout the workspace.
- Provide `bytemuck`-safe byte casting (`as_bytes`) so buffers can be uploaded
  to the GPU without copying.
- Implement the Spring SMF binary reader/writer (`SmfHeader`, `SmfMap`), covering
  heightmap (`u16`), metalmap (`u8`), typemap (`u8`), tile-index map, and DXT1
  minimap.
- Implement the SMT tile-file reader/writer (`write_smt`, `read_smt`) including
  DXT1 tile encoding and minimap generation.
- Provide correct `u16 ↔ f32` round-trip helpers for SMF import/export.

## Data Ownership
`bar-data` is the **authoritative creator** of `Heightmap` and `ColorBuffer`.
It is the only crate that may define or alter these types. `SmfMap` owns a
complete parsed `.smf` file in memory (including a `Heightmap` instance).
Transient GPU-upload buffers live only inside the call site that calls
`as_bytes`; `bar-data` does not touch the GPU itself.

## Key Public Types
| Type | Description |
|---|---|
| `Heightmap` | 2-D `f32` grid; `data()`, `as_bytes()`, `to_u16()`, `from_u16()` |
| `HeightSample` | `#[repr(C)]` single-sample wrapper, `Pod + Zeroable` |
| `ColorBuffer` | 2-D RGBA `f32` grid; `resize()` (bilinear), `to_rgba8()`, `from_rgba8()` |
| `SmfHeader` | Spring SMF binary header |
| `SmfMap` | Full parsed .smf file including all sections |
| `HeightmapError` | `DimensionMismatch`, `InvalidDimensions` |
| `Sd7Error` | IO and format parse errors for .sd7 workflows |

## Interaction Surface
**Calls into:** only `std`, `bytemuck`, `image`.  
**Exposes to callers:** raw `&[f32]` / `&[u8]` slices, pixel get/set, byte
cast helpers, u16 round-trip, and Spring SMF/SMT binary I/O.

## Boundaries — What This Crate Must NOT Do
- Must not depend on `bar-graph`, `bar-compute`, `bar-engine`, `bar-gui`,
  `bar-render`, `bar-project`, `bar-app`, or `bar-cli`.
- Must not open or display any GUI.
- Must not directly create wgpu buffers or submit GPU work.
- Must not contain evaluation logic (noise generation, erosion, etc.).

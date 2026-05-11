---
applyTo: "crates/bar-render/**"
---

# bar-render — 3D Terrain Viewport Renderer

> **bar-editor renderer posture:** `bar-render` is the wgpu integration
> point for Recoil shader ports. Porting Recoil's GLSL shaders to WGSL
> and running them in this pipeline is the correct approach to
> engine-faithful previews. Embedding or launching the Recoil engine as
> a subprocess is explicitly NOT the approach — see
> `docs/recoil-vendor-strategy.md`.
>
> Vendored upstream GLSL sources live in `vendor/recoil/shaders/GLSL/`.
> Ported WGSL files live in `shaders/recoil/` with SPDX GPL-3.0-or-later
> headers. The shader concatenation order in `renderer.rs` is:
> `modern_sky.wgsl` -> `smf_ground.wgsl` -> `water.wgsl` -> `terrain.wgsl`.
>
> The water surface uses two off-screen passes (planar reflection +
> planar refraction) plus the main pass. Each off-screen pass uses a
> `clip_plane` uniform to keep only one side of the water plane;
> `shade_water` blends them with Schlick fresnel above water and
> Snell's-window logic below. See `shaders/water.wgsl::shade_water`.

## Role
`bar-render` owns the real-time 3D preview pipeline. Given a `Heightmap` (and
optionally a `ColorBuffer`), it produces a rendered frame in an off-screen wgpu
texture that egui can display as a texture widget. It knows nothing about the
graph, the project format, or compute pipelines. Its only data dependency is
`bar-data`.

## Responsibilities
- Generate triangulated terrain meshes from `Heightmap` data
  (`generate_terrain_mesh`, `generate_terrain_mesh_lod` with configurable LOD
  cap) and upload them to GPU vertex/index buffers.
- Maintain an orbital `Camera` (azimuth / elevation / distance / target) with
  `orbit()`, `zoom()`, and `pan()` operations; expose a `view_projection()`
  matrix for the shader.
- Upload albedo textures from `ColorBuffer` to GPU (`update_texture`); fall
  back to procedural height-based colour when no texture is bound (shader
  looks up colour by normalised Y position).
- Render to an off-screen `Rgba8UnormSrgb` texture; expose the
  `wgpu::TextureView` via `output_view()` for egui texture registration.
- Respond to viewport resize (`resize(w, h)`) by recreating the depth and
  output textures at the new dimensions. `resize` is called every frame by
  `bar-app` when the available viewport area changes, ensuring pixel-exact
  rendering with no stretching.
- Accept per-render physical scale parameters: `height_scale`, `x_extent`,
  `z_extent`, `water_y`, `water_color`. These are computed by `bar-app` from
  the map's elmo range and passed through `update_mesh` / `update_mesh_lod`.
- Render a water / lava plane at world-Y = `water_y` and discard submerged
  terrain fragments in the fragment shader to eliminate z-fighting.

## Data Ownership
`TerrainRenderer` owns all GPU resources: render pipeline state, camera uniform
buffer, vertex/index buffers, depth texture, output texture, and optionally an
albedo texture. Dropping `TerrainRenderer` releases all GPU memory immediately.
`Camera` is a plain value type owned by the `Session` in `bar-app`.

## Key Public Types
| Type | Description |
|---|---|
| `Camera` | Orbital camera: `target, distance, azimuth, elevation, fov, near, far` |
| `TerrainRenderer` | Full wgpu render pipeline with upload and render methods |
| `TerrainVertex` | `#[repr(C)]` vertex: position + normal + UV, `Pod + Zeroable` |
| `generate_terrain_mesh` | Build full-resolution mesh from `Heightmap` |
| `generate_terrain_mesh_lod` | Build decimated mesh capped at `max_grid_size` |

## Interaction Surface
**Calls into:** `bar-data::Heightmap` and `bar-data::ColorBuffer` to generate mesh
geometry and upload textures; `wgpu` for render pipeline; `glam` for matrix
math; `bytemuck` for uniform buffer byte casting.  
**Exposes to `bar-app`:**
- `TerrainRenderer::new(device, queue, output_format)` — pipeline creation
- `update_mesh(device, heightmap, height_scale, x_extent, z_extent, water_y, water_color)` — full-res geometry
- `update_mesh_lod(device, heightmap, height_scale, max_grid_size, x_extent, z_extent, water_y, water_color)` — decimated geometry
- `update_texture(device, queue, color_buffer)` — replace albedo
- `render(device, queue, camera)` — render one frame
- `resize(device, width, height)` — recreate output/depth textures
- `output_view()` — `Option<&wgpu::TextureView>` for egui registration
- `has_mesh()` — whether a mesh has been uploaded
- `width`, `height` — current output texture dimensions (public fields)
- `Camera` — value type for orbit/zoom/pan

## Shader Contract
The terrain shader lives at `shaders/terrain.wgsl` (loaded at compile time via
`include_str!`). The `CameraUniform` struct layout must be kept in sync between
Rust and WGSL. Total size: **336 bytes**, 16-byte aligned.
Compile-time size assertion: `const _: () = assert!(size_of::<CameraUniform>() == 336);`

Bind group layout across all pipelines:
| Group | Binding | Name | Declared in |
|---|---|---|---|
| 0 | 0 | `camera` (uniform buffer) | `terrain.wgsl` |
| 1 | 0/1 | `albedo_tex` / `albedo_sam` | `terrain.wgsl` |
| 1 | 2 | `metalmap_tex` | `terrain.wgsl` |
| 1 | 3/4 | `typemap_tex` / `material_sam` | `terrain.wgsl` |
| 2 | 0/1 | `reflection_texture` / `reflection_sampler` | `terrain.wgsl` |
| 2 | 2/3 | `refraction_texture` / `refraction_sampler` | `terrain.wgsl` |
| 3 | 0/1 | `water_normal_tex` / `water_normal_sam` | `water.wgsl` |
| 3 | 2 | `heightmap_tex` (R32Float, non-filterable) | `terrain.wgsl` |

The reflection and refraction textures share group 2 to keep the renderer
within the default `max_bind_groups = 4` wgpu limit.

UV encoding used by the mesh generator and tested in the fragment shader:
- `uv.x < -0.5` → water/lava plane fragment (flat colour, no lighting)
- `uv.y > 1.5` → skirt/cap fragment (height-based colour, never textured)
- otherwise, `has_texture != 0` → sample albedo; else procedural height colour

`height_scale` passes the per-frame scale to the shader, which divides
`world_position.y` by it to recover the original `[0, 1]` normalised height for
colour lookup. `water_y ≥ 0` triggers a `discard` on terrain fragments at or
below the water plane to eliminate z-fighting.

## Boundaries — What This Crate Must NOT Do
- Must not depend on `bar-compute`, `bar-graph`, `bar-project`, `bar-engine`,
  `bar-gui`, `bar-app`, or `bar-cli`.
- Must not perform graph evaluation, noise generation, erosion, or any compute
  pipeline work.
- Must not open file dialogs or contain egui widget code.
- Must not create a wgpu device — always receives one from the caller.
- The `Camera` type is a plain math struct; it must not embed GUI interaction
  logic (mouse deltas are applied by `bar-app`).

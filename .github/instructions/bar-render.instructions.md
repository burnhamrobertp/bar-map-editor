# bar-render engineering guide

GPU-side terrain and feature viewport rendering via wgpu.

## Module layout

| File | Purpose |
|---|---|
| `renderer.rs` | `TerrainRenderer` -- all terrain passes, feature draw call |
| `features.rs` | `FeatureRenderer`, `FeatureInstance` -- placeholder box pipeline |
| `terrain.rs` | Mesh generation (grid, skirts, water plane) |
| `camera.rs` | Orbit camera, `Camera` type |
| `picking.rs` | CPU ray-cast terrain picker |

Shaders live in `shaders/` at the workspace root:
- `terrain.wgsl` -- main terrain + water + sky
- `shaders/recoil/modern_sky.wgsl`, `smf_ground.wgsl`, `minimap.wgsl` -- ported Recoil shaders concatenated at runtime
- `features.wgsl` -- feature placeholder box shader

## Render pipeline

Render order within `TerrainRenderer::render_internal` (one encoder, one queue.submit):

1. Reflection pre-pass -- clipped below water plane, terrain + sky
2. Refraction pre-pass -- clipped above water plane, terrain + sky
3. Main terrain pass (LoadOp::Clear color + depth): terrain geometry, sky quad
4. Feature pass (LoadOp::Load color + depth): placeholder boxes via `FeatureRenderer`

All passes share the camera uniform at group 0 / binding 0 (336-byte `CameraUniform`).

## Camera uniform (group 0)

`CameraUniform` is 336 bytes. The terrain shader uses all fields. The feature shader
only reads `view_proj` (first 64 bytes). WGSL allows structs smaller than the bound
buffer, so the feature shader declares a minimal `Camera { view_proj: mat4x4<f32> }`.

## Feature rendering (M1-M3)

`FeatureRenderer` is created inside `TerrainRenderer::new()` and stored as
`TerrainRenderer::feature_renderer: Option<FeatureRenderer>`.

`FeatureInstance` layout (80 bytes, `repr(C)`):
- `col0..col3: [f32; 4]` -- column-major 4x4 model transform
- `tint: [f32; 4]` -- RGBA tint

### Grouped mesh pipeline (M3)

`FeatureRenderer` maintains two pools:
- `meshes: HashMap<String, FeatureMesh>` -- one `FeatureMesh` per named model type, each
  with its own `vertex_buffer`, `index_buffer` (Uint32), and `instance_buffer`
- `placeholder_vb`/`placeholder_ib`/`placeholder_instances` -- shared cube mesh (Uint16
  indices) for unloaded and unknown types

The vertex layout is `bar_data::S3oVertex` (32 bytes) for both real models and the
placeholder cube -- one render pipeline covers both.

`TerrainRenderer::update_feature_instances(device, groups, unknowns)` takes:
- `groups: &HashMap<String, Vec<FeatureInstance>>` -- instances per loaded model name
- `unknowns: &[FeatureInstance]` -- instances for placeholder box

`TerrainRenderer::load_feature_mesh(device, name, mesh: &bar_data::S3oMesh)` uploads a
real S3O model. Called from bar-app after each model arrives from the background loader.

`FeatureRenderer::draw` loops real meshes first (Uint32 indices), then placeholder (Uint16)
in a single render pass.

## Adding new render passes

New passes should be added to `render_internal` using the same encoder, either
before or after existing passes as the depth-ordering requires. The depth texture
view is `self.depth_texture` and the color output is `self.output_view`.

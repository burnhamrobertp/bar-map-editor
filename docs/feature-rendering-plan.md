# Feature rendering in the 3D previewer -- architecture plan

Features are placed objects on BAR maps (trees, rocks, geo-thermal vents, crystals,
etc.). They are authored as placement data (`feature_type` name + world position +
rotation angle). The game engine renders them using its own asset pipeline; the
editor needs to visualize them in the 3D preview for placement verification.

## Current state

`MapState.features: Vec<PlacedFeature>` is fully populated through the
import->recipe->export pipeline but the 3D preview ignores it. This plan covers
adding end-to-end feature visualization.

## Crate placement

No new crates. Natural homes for each piece:

| Piece | Crate | Rationale |
|---|---|---|
| BAR game data catalog, VFS/`.sdz` reading | `bar-engine` | Already does archive extraction (sevenz-rust/zip) |
| `.s3o` parser | `bar-data` | Format parsing is `bar-data`'s job, same as SMF/SMT |
| GPU buffers, placeholder pipeline, instance rendering | `bar-render` | Same WGPU device as terrain |
| Coordinate transform, catalog load trigger | `bar-app` | Owns both `Session` (WGPU) and the feature list |
| BAR install path preference | `bar-gui::Settings` | Settings file already handles all user prefs |

## Milestones

### M1: Placeholder-only rendering (no game data, no model loading) -- COMPLETE

All features render as solid colored unit boxes at correct world positions. No
catalog, no `.s3o`. Does not require a BAR installation to be configured.

**Acceptance:** open any imported `.sd7` with features, see colored boxes at sane
positions in the 3D preview, occluded by terrain.

Work items:

- `FeatureInstance` -- 80-byte `repr(C)` struct: 4x4 column-major transform matrix,
  `tint: [f32; 4]`, `is_placeholder: u32`, `_pad: [u32; 3]`. Lives in `bar-render`.
- `FeatureRenderer` in `bar-render/src/features.rs`:
  - Placeholder unit-cube mesh (12 triangles, hardcoded vertices)
  - Render pipeline shared between real models and placeholders
  - Instance buffer (rebuilt when feature list changes)
  - `new(device, output_format, depth_format, camera_bgl)` -- takes the camera BGL
    by reference so it shares the same bind group layout as the terrain pipeline
  - `update_instances(device, &[FeatureInstance])` -- rebuilds GPU instance buffer
  - `draw<'a>(&'a self, pass: &mut RenderPass<'a>, camera_bg: &'a BindGroup)`
- Expose `pub fn camera_bind_group(&self) -> &wgpu::BindGroup` on `TerrainRenderer`
  (one-liner; unblocks shared camera uniform without duplicating the buffer).
- `feature_renderer: Option<FeatureRenderer>` and `features_dirty: bool` added to
  `Session` in `bar-app`.
- `build_feature_instances()` free function in `bar-app`: converts `&[PlacedFeature]`
  + map dimension params into `Vec<FeatureInstance>` (Spring world-space ->
  render-space coordinate transform).
- Features draw in the main pass **after terrain geometry, before the sky quad**,
  using `LoadOp::Load` on both color and depth attachments so terrain correctly
  occludes features. This is a separate render pass, not injected into the terrain
  pass closure.
- `features_dirty` set to `true` on project load and project reset.

**Placeholder color:** solid magenta/orange. Wireframe is not used -- `PolygonMode::Line`
is not supported on all WGPU backends (Metal in particular).

### M2: Game version selection and feature catalog -- COMPLETE

Catalog loaded from the selected game archive. Known vs unknown feature types
visually differentiated (different tint). Validation panel warns on unknown types.

Work items:

- `selected_game_archive: Option<PathBuf>` (and `bar_install_path: Option<PathBuf>`)
  added to `bar-gui::Settings` (persisted to disk).
- Prefs panel UI: new "BAR installation" section. Auto-populate from
  `BarVersions::detect()` on first run.
- `FeatureCatalog` in `bar-engine/src/feature_catalog.rs`:
  ```
  pub struct FeatureDef {
      pub name: String,
      pub model_path: Option<String>,  // e.g. "objects3d/Arborreal.s3o"
      pub footprint_x: u16,
      pub footprint_z: u16,
  }
  pub struct FeatureCatalog {
      defs: HashMap<String, FeatureDef>,  // keys are lowercased
  }
  impl FeatureCatalog {
      pub fn from_game_archive(archive_path: &Path) -> Self { ... }
      pub fn lookup(&self, feature_type: &str) -> Option<&FeatureDef> { ... }
  }
  ```
- Lua parsing strategy: **pattern-matching extractor, not a full Lua runtime.**
  `featuredata.lua` and `features/*.lua` use predictable key-value patterns for
  the fields we need (`object`, `footprintx`, `footprintz`). A line-by-line
  pattern matcher covers 95%+ of cases. Anything it can't parse falls back to
  unknown-type (placeholder). `mlua` (full Lua runtime, ~1MB, C toolchain dep)
  is the escalation path if coverage proves insufficient.
- Helper: `pub fn read_file_from_archive(archive: &Path, internal_path: &str) -> Option<Vec<u8>>`
  -- reads a named file from an `.sdz` (zip) archive. Used by both catalog loading
  and model loading.
- Catalog loading runs on a background `std::thread` via mpsc, same pattern as
  SD7 extraction in `AppWrapper`. `AppWrapper` gains:
  `feature_catalog: Option<FeatureCatalog>` and catalog-load channel state.
- Reload triggered when `selected_game_archive` changes in settings.
- Validation: features with no catalog match emit warnings in the validation panel.

### M3: S3O model loading and rendering -- COMPLETE

Recognized feature types render their actual `.s3o` models from the game archive.
Unknown types retain the placeholder box.

Work items:

- `S3oVertex` (`position: [f32;3]`, `normal: [f32;3]`, `uv: [f32;2]`, `bytemuck::Pod`)
  and `S3oMesh` (`vertices`, `indices`, `aabb_min`, `aabb_max`) in `bar-data/src/s3o.rs`.
- `pub fn parse_s3o(data: &[u8]) -> Result<S3oMesh, S3oError>` -- reads the binary
  S3O format (fixed header + recursive piece tree), flattens all pieces into a single
  VB+IB in CPU-space before upload. No articulation needed for a placement previewer.
  Format spec: https://springrts.com/wiki/Model_Format_S3O
- `FeatureRenderer::load_mesh(device, name, mesh: &S3oMesh)` -- uploads a real model
  for a named feature type. Stored in `meshes: HashMap<String, FeatureMesh>`.
- On catalog-loaded event in `AppWrapper`: for each unique feature type in the current
  map, extract the corresponding model from the game archive using
  `read_file_from_archive`, parse with `parse_s3o`, upload via `load_mesh`. Only
  models for types actually present in the map are loaded (typically 5-30 types).
  Models that fail to parse fall back to placeholder silently.
- Render: one `draw_indexed_instanced` call per unique mesh type (instances grouped
  by feature type). Unknown types share the placeholder mesh draw call.

### M4: Polish (lower priority)

- Hover tooltip showing feature type name (requires pixel picking integration --
  non-trivial).
- LOD: suppress features below a screen-size threshold when zoomed out.
- Features in reflection/refraction passes.
- Async progressive model loading (instead of blocking until all models ready).

## Rendering pipeline integration

Features share the camera bind group (group 0) with the terrain pipeline:

```
TerrainRenderer::camera_bind_group() -> &wgpu::BindGroup
  |
  v
FeatureRenderer::draw(pass, camera_bg)
```

Render order within a frame:
1. Reflection pass (terrain only -- features excluded in M1/M2/M3)
2. Refraction pass (terrain only)
3. Main pass:
   a. Terrain geometry draw
   b. Feature instances draw  <-- new, LoadOp::Load on color+depth
   c. Sky fullscreen quad

## Coordinate transform

Spring world-space to render-space (lives in `bar-app::build_feature_instances`):

```
rx = (x / map_width_elmos - 0.5) * 2.0 * x_extent
ry = (y - min_height) / height_range * height_scale
rz = (z / map_height_elmos - 0.5) * 2.0 * z_extent
rotation: Y-axis by -angle (Spring stores degrees, CCW from +Z)
```

**Open question:** Spring may snap placed features to terrain height at runtime,
meaning the stored Y in the SMF feature section is always 0. If so, the renderer
needs to sample the heightmap at (x, z) to compute the display Y. Verify against
a known map before declaring M1 complete.

## Risks

**`.3do` models.** Some older BAR features use `.3do` (more complex format, no
UVs). Render as placeholder in M3, surface as validation warning. Address later if
demand exists.

**VFS archive layering.** BAR layers multiple `.sdz` archives. For M3, only look
inside the selected game archive. Falls back to placeholder if a model is not found
there. Covers 99%+ of BAR's own feature definitions.

**Coordinate system alignment.** Verify the angle convention (Spring degrees around
Y, 0 = facing +Z) against a known asymmetric feature before calling M1 done.

**S3O textures.** S3O embeds texture name references but not the textures. M3
renders flat-shaded with normals only. Texture loading is a future enhancement.

**Feature count ceiling.** Up to 65535 features per map (16-bit index in SMF).
Instance buffer at 80 bytes/instance = ~5MB GPU max. Fine. The `is_placeholder`
flag in `FeatureInstance` keeps the GPU layout uniform regardless of catalog state.

## Implementation order

1. `TerrainRenderer::camera_bind_group()` -- one-liner, unblocks everything.
2. `FeatureInstance` type + `FeatureRenderer` with placeholder pipeline (M1).
3. `S3oVertex` stub in `bar-data` (even without parser -- lets bar-render import it).
4. Wire `feature_renderer` into `Session`; wire `build_feature_instances` and draw
   call into `AppWrapper::update`. Test M1.
5. `selected_game_archive` in `Settings` + prefs UI (M2).
6. `FeatureCatalog::from_game_archive` with Lua pattern extractor (M2).
7. Background catalog load + validation warnings (M2). Test M2.
8. `parse_s3o` in `bar-data` with tests (M3).
9. `FeatureRenderer::load_mesh` + model extraction on catalog-ready (M3). Test M3.
10. Update `bar-render.instructions.md` and `bar-engine.instructions.md`.

# bar-render engineering guide

GPU-side terrain, water, sky, and feature viewport rendering via wgpu.
Engine-faithful port of Recoil's `SMFFragProg.glsl`, `BumpWaterFS.glsl`,
and `ModernSkyFS.glsl` -- see `docs/recoil-shader-ports.md` for the
per-shader status and what's missing.

## Module layout

| File | Purpose |
|---|---|
| `renderer.rs` | `TerrainRenderer` -- all terrain / water / sky / shadow passes, feature draw orchestration |
| `features.rs` | `FeatureRenderer`, `FeatureInstance` -- S3O model + placeholder box pipeline |
| `terrain.rs` | Mesh generation (grid, skirts, water plane) |
| `camera.rs` | Orbit camera, `Camera` type |
| `picking.rs` | CPU ray-cast terrain picker |
| `shadow.rs` | Directional shadow caster + receiver, PCF sampling |

Shaders live in `shaders/` at the workspace root, organised by
origin:

| Directory | Provenance |
|---|---|
| `shaders/recoil/` | Pure engine ports (BAR's `SMFFragProg`, `ModernSkyFS`, etc.). Stable upstream contract; only changes when Recoil changes. |
| `shaders/widgets/` | **BAR LuaUI widget ports** -- effects driven by `mapinfo.custom.*` per-map blocks (e.g. `custom_fog.wgsl` for the height-fog widget). Per-map authored content; segmented so engine-native paths stay clean. See `memory/feedback_no_game_widget_porting.md` for the in-scope rule. |
| `shaders/*.wgsl` (top level) | Composer shaders -- `terrain.wgsl`, `water.wgsl`, `features.wgsl`, `gamma_encode.wgsl`, shadow shaders. These declare bindings + entry points and call helpers from the directories above. |

Concatenation order at pipeline-build time
(`TerrainRenderer::new`):
`recoil/modern_sky.wgsl` → `recoil/smf_ground.wgsl` →
`widgets/custom_fog.wgsl` → `water.wgsl` → `terrain.wgsl`.

Widget shaders go BEFORE the composer shaders that call them
because WGSL does not forward-reference functions (it does
forward-reference module-scope `var`s, which is why widget shaders
can use `camera.*` even though that binding is declared in
`terrain.wgsl`).

When adding a new widget port, drop the WGSL into
`shaders/widgets/<name>.wgsl` and `include_str!` it in
`TerrainRenderer::new` (and in the `terrain_shader_wgsl_parses`
unit test). The Rust-side state (bind groups, uniforms,
`update_*` methods) ideally belongs under
`crates/bar-render/src/widgets/<name>.rs` -- the existing
`custom.fog` plumbing currently still sits in `SmfLighting` +
`CameraUniform` for historical reasons; new widgets should land
in their own module.

## Render passes

`TerrainRenderer::render_internal` submits one or more command encoders
in this order (per frame):

1. **Shadow pass** -- terrain + features drawn from sun's POV into a
   single depth texture. Camera bind group binds a shadow-caster
   uniform. Receiver bind group built once and reused.
2. **Reflection pre-pass** (when `water_y >= 0`) -- world mirrored
   about y=water_y, clipped to the camera's side of the water plane.
   Terrain + sky drawn into `reflection_view`.
3. **Refraction pre-pass** (when `water_y >= 0`) -- world rendered from
   the original camera with a clip plane on the opposite side of the
   water plane. Terrain (with full SMF water-absorption shading) +
   features drawn into `refraction_view`. **The depth attachment is
   sampled later by the water shader** so it must be created with
   `RENDER_ATTACHMENT | TEXTURE_BINDING` usage.
4. **Main pass** (split into three subdraws on one encoder):
   - Terrain ground: `draw_indexed(0..water_index_offset)`
   - Sky: `vs_sky` full-screen triangle
   - Features: `FeatureRenderer::draw` -- LoadOp::Load color + depth
   - Water plane: `draw_indexed(water_index_offset..num_indices)` --
     LoadOp::Load color + depth, alpha-blends the BumpWater composite
     over the existing scene

Per-frame uniform writes use `queue.write_buffer` between submits;
write order matches pass order so each pass reads the right uniform.

## Bind groups

### Group 0 -- camera + skybox

| Binding | Resource | Type |
|---|---|---|
| 0 | `camera_buffer` | `CameraUniform` (496 bytes) |
| 1 | `skybox_view` | `texture_cube<f32>` |
| 2 | `skybox_sampler` | filtering sampler |

Defaults to a 1×1 black cubemap; `update_skybox` rebuilds the bind
group with a real cubemap loaded from mapinfo's `atmosphere.skyBox`.
Every pipeline binds this at group 0, so the cubemap is always
available (currently sampled only by `fs_sky` and the SMF sky-cube
reflection path in `fs_main`).

### Group 1 -- terrain textures

| Binding | Resource |
|---|---|
| 0 | albedo (BC1-compressed SMT atlas or evaluated RGBA) |
| 1 | albedo sampler (filtering, ClampToEdge) |
| 2 | metalmap (R8Unorm) |
| 3 | typemap (R8Unorm) |
| 4 | material sampler (shared by metal + typemap) |
| 5 | `detailTex` (legacy single detail texture) |
| 6 | detail sampler (filtering, Repeat) |
| 7..10 | `splatDetailNormalTex1..4` -- the four splat-detail-normal textures |
| 11 | `splatDistrTex` -- 4-channel splat distribution map |
| 12 | `skyReflectModTex` -- per-pixel sky-reflection mask |

All splat / detail textures default to 1×1 grey or black so the shader
gates evaluate to a no-op contribution when the map doesn't authoring
them. The renderer-side flags (`advanced_splat_enabled`,
`sky_reflect_mod_enabled`) flip in `update_splat_textures` /
`update_sky_reflect_mod` and propagate to the shader through
`SmfLighting::skybox_params` / `splat_params` (overridden in
`sync_to_frame`, not the recipe -- runtime state).

### Group 2 -- water planes + depth

| Binding | Resource |
|---|---|
| 0 | `reflection_view` |
| 1 | reflection sampler |
| 2 | `refraction_view` |
| 3 | refraction sampler |
| 4 | `water_params_buffer` (`WaterParamsUniform`, 80 bytes) |
| 5 | `refraction_depth` (sampled depth texture) |
| 6 | depth sampler (non-filtering) |

Refraction depth is sampled by the water shader for the engine's
depth-aware refraction mixback (`BumpWaterFS:304-314`).

### Group 3 -- water normal + heightmap

| Binding | Resource |
|---|---|
| 0 | water normal map (procedurally generated, 128×128) |
| 1 | water normal sampler (Repeat, filtering) |
| 2 | heightmap (R32Float, non-filtering -- read via `textureLoad`) |

### Group 4 -- shadow receiver

| Binding | Resource |
|---|---|
| 0 | shadow uniform (light VP matrix + sun dir) |
| 1 | shadow depth texture |
| 2 | comparison sampler (for hardware 2×2 PCF) |

A dummy receiver group is bound in pre-passes that don't actually
sample shadows -- pipeline layouts still require the slot.

## Camera uniform (496 bytes)

`CameraUniform` is std140-aligned and packed for `bytemuck::Pod`.
Field groups, in order:

- Camera matrices: `view_proj`, `inv_view_proj`, `camera_pos`,
  `has_texture`
- Terrain transform: `height_scale`, `water_r/g/b`, `water_y`, `time`,
  `skip_water`, `height_range_elmos`, `screen_w/h`, `x_extent`,
  `z_extent`
- Lighting: `sun_dir_exp`, `ground_ambient`, `ground_diffuse`,
  `ground_specular`
- Water absorption: `water_absorb`, `water_base_color`,
  `water_min_color`
- Per-frame state: `brush_cursor`, `clip_plane`
- Custom fog: `custom_fog_color_atten` (rgb+atten), `custom_fog_params`
  (enabled + ceiling height)
- Atmosphere / sky: `sun_color`, `sky_color_density` (rgb+density),
  `sky_dir`, `cloud_color`, `skybox_params` (enabled + detail_strength
  + sky_reflect_mod_enabled)
- Splat detail: `splat_tex_scales`, `splat_tex_mults`, `splat_params`
  (elmo_per_render_xz + advanced_enabled + diffuse_alpha)

When adding a new uniform field, update both this struct AND the WGSL
declaration at the top of `terrain.wgsl`; the size assertion at the
bottom of the struct catches layout drift at compile time.

`SmfLighting` is the public-facing inputs struct that the host builds
from `MapSettings`; `SmfLighting::to_uniform_slots()` packs into the
binary `CameraUniform` layout. The runtime upload flags
(`skybox_enabled`, `advanced_splat_enabled`, `sky_reflect_mod_enabled`,
`elmo_per_render_xz`) are NOT sourced from MapSettings -- they're
overridden in `sync_to_frame` based on which assets the renderer has
actually uploaded.

## Sampler convention

Every world-space filtered sampler routes through
`crate::samplers::make_filtered_sampler(device, label, address_mode)`.
This guarantees a single workspace-wide filtering story:
linear min/mag/mip + `anisotropy_clamp: 16`, mirroring BAR's
engine-wide `Springsettings.cfg::MaxTexAniso = 16` applied via
`Bitmap.cpp:1746`.

**Use the helper for**: terrain albedo, splat detail textures,
feature model textures, grass blade + grass shading textures,
water caustics, skybox cubemap -- anything that binds a textured
world-space asset.

**Don't use the helper for**: samplers where any filter must be
`Nearest` (depth-comparison shadow PCF, mipmap-Nearest lookups
like water reflection / refraction, full-screen post-passes like
gamma encode, or 1×1 placeholder textures without mip chains).

If you find yourself writing `device.create_sampler` directly for
a world-space asset, the helper is what you want -- skipping it
leads to ad-hoc divergence where one texture looks crisp at
oblique angles and the texture next to it looks smeared.

## Asset upload paths

Map-authored textures from mapinfo `resources = { ... }` and
`atmosphere = { ... }` are uploaded by the host (`bar-app::viewport`)
via `update_*` methods on `TerrainRenderer`:

| Source | Renderer method | Default when unset |
|---|---|---|
| `atmosphere.skyBox` | `update_skybox(Cubemap)` / `clear_skybox()` | 1×1 black cubemap (procedural sky path runs) |
| `resources.detailTex` | `update_detail_texture(rgba, w, h)` | 1×1 mid-grey (zero contribution) |
| `resources.splatDetailNormalTex1..4` + `splatDistrTex` | `update_splat_textures([5 textures])` / `clear_splat_textures()` | 1×1 mid-grey (zero contribution) |
| `resources.skyReflectModTex` | `update_sky_reflect_mod(rgba, w, h)` / `clear_sky_reflect_mod()` | inert 1×1 (gate disables sample) |
| `resources.specularTex` | `update_specular_tex(rgba, w, h)` / `clear_specular_tex()` | inert 1×1 (gate falls back to global ground_specular / spec_exponent) |
| `resources.grassShadingTex` | `update_grass_shading_tex(rgba, w, h)` / `clear_grass_shading_tex()` | 1×1 mid-grey (extension falls back to playable albedo) |
| `resources.lightEmissionTex` | `update_light_emission_tex(rgba, w, h)` / `clear_light_emission_tex()` | inert 1×1 (0,0,0,0) (gate skips apply-emission stage) |

The host side (`bar-app::viewport::sync_*` helpers) handles file
discovery via recursive `find_file_in_dir` on the `.barproj/passthrough/`
tree, decoding via `bar_data::load_dds_cubemap` /
`viewport::load_2d_image`, and idempotent re-upload on project change.

**Inert-default convention**: where the table says "gate skips
sample", the 1×1 placeholder is **never sampled** because a
`*_enabled` flag in the `skybox_params` / `custom_fog_params` uniform
gates the entire shader branch behind it. This mirrors the engine's
compile-time `#ifdef SMF_SPECULAR_LIGHTING` / `SMF_SKY_REFLECTIONS` /
`SMF_LIGHT_EMISSION` toggles (`SMFFragProg.glsl:403-416, 392-401`).
When a map ships a real texture, `update_*` flips the gate on; when
it doesn't, the global-uniform fallback (or no contribution) runs
instead -- matching engine behaviour.

## Feature rendering

`FeatureRenderer` is owned by `TerrainRenderer::feature_renderer:
Option<FeatureRenderer>`. Two pools:

- `meshes: HashMap<String, FeatureMesh>` -- one entry per named S3O
  model, each with its own vertex buffer, index buffer (Uint32),
  instance buffer, and a group-1 `texture_bind_group` holding tex1 +
  tex2 + shared sampler.
- `placeholder_*` -- shared cube mesh (Uint16 indices) for unloaded and
  unknown types, drawn with the renderer-wide 1×1 white texture in
  both slots.

`FeatureInstance` layout (80 bytes, `repr(C)`):
- `col0..col3: [f32; 4]` -- column-major model matrix
- `tint: [f32; 4]` -- RGBA tint

Vertex layout is `bar_data::S3oVertex` (32 bytes) for both real and
placeholder meshes. Attributes feed locations 0 (position), 1 (normal),
7 (uv); locations 2-6 are per-instance transform + tint.

### S3O texture channel convention

Per `cont/base/springcontent/shaders/GLSL/ModelFragProg.glsl:87-109`:
- **texture1.rgb** is the diffuse colour. Sampled directly.
- **texture1.a** is the team-colour mask, not opacity. For map
  features (gaia team) this is typically 0 -- ignored by our shader.
- **texture2.r** is self-illumination / emissive. The engine adds
  `extraColor.rrr` to its `reflection` lighting multiplier, which then
  multiplies the texture1 RGB -- so the glow is the feature's own
  colour, not a white wash. Our shader replicates this as
  `diffuse * (lit + emissive)`.
- **texture2.g** is the specular intensity multiplier. Engine
  multiplies sun-spec by `extraColor.g * 4.0` and mixes env reflection
  by the same channel. We don't have env reflection yet so we only
  apply the spec multiplier.
- **texture2.a** is the opacity. Leaf-card cutouts and translucent
  materials (crystals, glass) live here. Our fragment shader uses
  `shading.a` as the output alpha and discards texels below 0.05.
- Pipeline blend is `BlendState::ALPHA_BLENDING`. Draw order across
  meshes is not back-to-front yet; depth-pre-pass-then-blend is a
  follow-up if semi-transparent features layer wrong.

**Default-texture caveat**: placeholder cubes and S3O models with no
loaded texture2 bind a 1x1 `(0,0,0,255)` fallback (`default_shading_view`),
not the 1x1 white that texture1 uses. White on texture2 would mean
emissive=1 (whole feature self-illuminating) and spec-mult=4 -- not the
intended no-op. If you add another use of the texture2 binding,
re-check the fallback for that pixel pattern.

## Gamma-encode post-pass

BME mirrors BAR's gamma-incorrect pipeline: every shader runs in
sRGB-perceptual space and writes perceptual bytes to a non-sRGB
framebuffer. On a native sRGB display the engine's final intensity is
therefore `byte/255` raised to the display gamma (~2.2). eframe
composites our output onto an sRGB swapchain, which would otherwise
re-encode our perceptual bytes such that displayed intensity lands at
`V` instead of `V^2.2` -- visibly brighter, with washed-out highlights
and oversaturated channels.

A fullscreen `gamma_pipeline` runs at the end of `render_internal`,
sampling `output_texture` (the live perceptual render target) and
writing `pow(c, gamma_params.exponent)` into a separate
`display_texture`. The exponent is a uniform driven by
`TerrainRenderer::set_gamma_exponent`; the viewport debug overlay's
gear menu surfaces it as a slider so it can be tuned visually against
in-engine reference screenshots. eframe / egui_wgpu does partial
gamma handling in its compose pipeline so the net swapchain chain is
neither pure sRGB nor pure passthrough -- the residual correction
lands somewhere in `[1.0, 2.2]` (current empirical default 1.5). The
public `output_view()` accessor returns the **display** view;
`read_pixels` (used by the CLI preview) also reads the display target
so saved PNGs match the editor viewport.

Cross-pass intermediates (refraction, reflection) keep their raw
perceptual contents -- only the final swapchain-bound copy is gamma-
encoded. Shader code (`shaders/gamma_encode.wgsl`) is a fullscreen
triangle, no vertex buffer, depth disabled.

## Adding new render passes

New passes should be added to `render_internal` using the same
encoder, either before or after existing passes as the depth-ordering
requires. The depth texture view is `self.depth_texture` and the
color output is `self.output_view` (the **internal** perceptual
target; the public `output_view()` accessor returns the gamma-encoded
copy and is for egui only). Use the established pattern:

1. Write the relevant per-pass camera uniform variant via
   `queue.write_buffer(&self.camera_buffer, ...)`.
2. Create a fresh `CommandEncoder`, begin the render pass with the
   right load/store ops, set the pipeline + bind groups + buffers,
   draw, submit.

If the pass needs a new bind-group resource (additional texture or
uniform), extend an existing group rather than adding a new one --
each new group costs a `set_bind_group` call per pass and requires
plumbing through every pipeline layout that uses it.

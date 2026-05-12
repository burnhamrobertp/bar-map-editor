# Recoil shader port status

What's vendored, what's been translated to WGSL, and what remains. Source
for the map editor's pivot from "invented procedural visuals" to
"engine-faithful visuals via source-translated shaders" — see
`ROADMAP.md` for current status.

Vendored upstream files live at `vendor/recoil/shaders/GLSL/`. Our ports
live at `shaders/recoil/<name>.wgsl` and concatenate into the renderer
pipeline in this order: `modern_sky.wgsl` → `smf_ground.wgsl` →
`water.wgsl` → `terrain.wgsl`. Both directories are pinned to the
upstream commit recorded in `vendor/recoil/UPSTREAM.md`.

---

## Status table

| Recoil source                                    | Status                        | WGSL port                          | Where it runs                                        |
| ------------------------------------------------ | ----------------------------- | ---------------------------------- | ---------------------------------------------------- |
| `ModernSkyVS.glsl`, `ModernSkyFS.glsl`           | **Ported**                    | `shaders/recoil/modern_sky.wgsl`   | Sky pass (skybox + fog source)                       |
| `SMFVertProg.glsl`, `SMFFragProg.glsl`           | **Partially ported**          | `shaders/recoil/smf_ground.wgsl`   | Ground pass (called from `terrain.wgsl`)             |
| `MiniMapVertProg.glsl`, `MiniMapFragProg.glsl`   | **Ported** (not yet GUI-wired)| `shaders/recoil/minimap.wgsl`      | 2D inspector (off-line; not yet replacing topo view) |
| `BumpWaterVS.glsl`, `BumpWaterFS.glsl`           | **Replaced** (see below)      | `shaders/water.wgsl` (original)    | Water pass (Group 3, called from `terrain.wgsl`)     |
| `SMFShadingTexture{Vert,Frag}Prog.glsl`          | Folded into SMF               | (math inlined in `smf_ground.wgsl`)| n/a                                                  |
| `SMFBorderProg`                                  | Not vendored                  | —                                  | Map border (cosmetic edge fill)                      |
| `BumpWaterCoastBlur*`                            | Not vendored                  | —                                  | Shore softening (subordinate to BumpWater)           |

---

## What's actually involved in a port

GLSL→WGSL syntax translation is ~20% of the effort. The remaining 80% is
surrounding infrastructure: uniforms, bind groups, vertex layouts, auxiliary
textures, and additional render passes.

- **Pure shader math** (lighting formulas, fog mix, water absorption):
  translatable in hours. Direct line-for-line port works.
- **Uniform plumbing** (sun direction, ground colours, water tints):
  one-time cost per uniform. `CameraUniform` currently carries: view-proj,
  inv-view-proj, camera_pos, has_texture, height_scale, water rgb + water_y,
  time, quality, skip_water, screen size, sun_dir_exp, ground_ambient,
  ground_diffuse, ground_specular, water_absorb, water_base_color,
  water_min_color, brush_cursor. Additional fields need explicit add + Rust
  write per frame.
- **Auxiliary textures** (detail, normal, foam, caustics): need the asset
  itself — the engine ships these in its content tree; we vendor them or
  stand up procedural substitutes.
- **Additional render passes** (refraction, depth, FBO reflection): requires
  structural renderer changes — extra textures per resize, extra draw pass,
  extra bind group. The existing planar reflection pre-pass is the pattern.

---

## ModernSky — done

- Source: `ModernSky{VS,FS}.glsl`
- Port: `shaders/recoil/modern_sky.wgsl`
- Uniform inputs: time (in `CameraUniform`).
- Auxiliary textures: none.
- Extra passes: none.
- Drives both the skybox and the atmospheric fog source in `terrain.wgsl`.

---

## SMF ground — partially done

- Source: `SMFVertProg.glsl`, `SMFFragProg.glsl`
- Port: `shaders/recoil/smf_ground.wgsl` (called from `terrain.wgsl`)
- Uniform inputs added: `sun_dir`, `ground_ambient`, `ground_diffuse`,
  `ground_specular`, `ground_specular_exponent`, `water_absorb`,
  `water_base_color`, `water_min_color`. All sourced from
  `MapSettings.lighting` / `MapSettings.water`.
- **What's faithful**: lighting model (`groundAmbientColor +
  groundDiffuseColor * cos(N·L)`, multiplied by `SMF_INTENSITY_MULT =
  210/255`), Blinn-Phong specular with mapinfo's `specularExponent`,
  `SMF_WATER_ABSORPTION` underwater path (`SMF_SHALLOW_WATER_DEPTH = 10`
  elmos transition, `waterMinColor` clamp, `waterAbsorbColor * depth`
  gradient, ground-shadow-coeff decay term).
- **What's stubbed / skipped**:
  - **Detail texture (`detailTex`)** — engine multiplies diffuse by a
    high-frequency noise texture for breakup. Not present in our asset
    pipeline; terrain looks slightly smoother at high zoom than in-engine.
  - **Splat detail textures (DNTS)** — multi-channel splat overlay. Mapinfo
    can supply DNTS paths but the editor doesn't render them yet.
  - **Sky cube reflection** — optional cubemap-driven sky reflection on
    terrain (wet rocks). Needs a `skyReflectModTex` input.
  - **Shadow map** — directional-light shadow sampling. Not implemented;
    `groundShadowCoeff` hardcoded to `1.0`.
  - **Parallax / blend normals** — keyed off `parallaxHeightTex` /
    `blendNormalsTex`. Same asset gap as detail texture.
  - **Info overlay** — gameplay overlays (FoW, metal, LoS). Out of scope.

---

## MiniMap — done (not yet GUI-wired)

- Source: `MiniMap{VS,FS}Prog.glsl`
- Port: `shaders/recoil/minimap.wgsl`
- Uniform inputs: a transform matrix (identity for direct display; the engine
  uses it for camera-frustum overlay).
- Auxiliary textures: minimap diffuse — we'd feed our AutoTexture output.
- Extra passes: none.
- The 2D inspector currently does its own topo-style rendering. Switching to
  the ported shader is deferred until there's a concrete user complaint.

---

## BumpWater — replaced by original PBR shader

The Recoil BumpWater port (`shaders/recoil/smf_water.wgsl`) has been deleted
and replaced with an intentionally original PBR water shader at
`shaders/water.wgsl`. Motivation: the Recoil port carries GPL-v2 terms that
complicate the project's licensing; the original shader targets the same
BAR-website visual aesthetic without the GPL entanglement.

- Shader: `shaders/water.wgsl`
- Entry point called via `shade_water()` from `terrain.wgsl`
- Extra bind group: Group 3 (`water_normal_tex` / `water_normal_sam`).
  Normal map is a procedurally generated 128x128 texture; no vendored asset
  required.
- Uses existing planar reflection pre-pass (Group 2) for the reflection
  sample at quality > 0.5.
- **What's implemented**: GGX specular (`ggx_d`, roughness 0.06), Schlick
  Fresnel (f0 = 0.02), 4-octave scrolling normal-map surface, planar-
  reflection sample with normal distortion, sky-color fallback when quality
  is low.
- **Not implemented** (same gaps as the old port, except these are now
  explicitly out of scope for the original shader):
  - Refraction pre-pass, shorewaves, caustics -- deferred until user need.

---

## Other shaders not yet ported

- **`SMFBorderProg`** — not vendored. Renders the off-map border zone
  (repeated edge pixel + lighting). Cosmetic; defer until user-visible need.
- **Unit / feature shaders** — out of scope for the editor.

---

## Refresh procedure

When upstream Recoil moves and we want to take new versions:

1. `bash vendor/recoil/sync.sh` against a fresh local Recoil clone.
2. Update `vendor/recoil/UPSTREAM.md` with the new commit hash.
3. Diff the changed GLSL files against our WGSL ports; reflect any semantic
   changes. Mechanical syntax updates (renames, refactors) can usually skip.
4. Commit the sync and port updates separately.

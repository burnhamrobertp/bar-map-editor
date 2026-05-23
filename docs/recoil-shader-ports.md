# Recoil shader port status

What's vendored, what's been translated to WGSL, and what remains. Source for the map editor's pivot from "invented procedural visuals" to "engine-faithful visuals via source-translated shaders".

Vendored upstream files live at `vendor/recoil/shaders/GLSL/`. Our ports live at `shaders/recoil/<name>.wgsl` and concatenate into the renderer pipeline in this order: `modern_sky.wgsl` → `smf_ground.wgsl` → `water.wgsl` → `terrain.wgsl`. Both directories are pinned to the upstream commit recorded in `vendor/recoil/UPSTREAM.md`.

---

## Status table

| Recoil source                                  | Status                       | WGSL port                          | Notes                                                       |
| ---------------------------------------------- | ---------------------------- | ---------------------------------- | ----------------------------------------------------------- |
| `ModernSkyVS.glsl`, `ModernSkyFS.glsl`         | **Ported**                   | `shaders/recoil/modern_sky.wgsl`   | Atmosphere block fully wired (sun/sky/cloud colour, density, sky dir, cubemap skybox). |
| `SMFVertProg.glsl`, `SMFFragProg.glsl`         | **Mostly ported**            | `shaders/recoil/smf_ground.wgsl` + `shaders/terrain.wgsl` | Detail tex, splat detail-normal (color + normal perturb), basic splat-detail color, SMF_BLEND_NORMALS, splat-detail diffuse-alpha, lightEmissionTex glow, groundShadowDensity, sky cube reflection, water absorb, shadows, spec-add-not-multiply all in. Remaining: parallax (verified non-issue per below), infotex (gameplay overlay, out of editor scope). |
| `BumpWaterVS.glsl`, `BumpWaterFS.glsl`         | **Fully ported**             | `shaders/water.wgsl`               | Octave normals, surface composite, refraction with depth mixback + engine-formula UV distortion, 7-tap reflection blur (`opt_blurreflection`) with mapinfo-driven `blurBase`/`blurExponent`, fresnel reflection, sun spec, height fog overlay, tonemap, 32-frame caustic animation (loaded from engine `bitmaps.sdz`), shore foam (`GetShorewaves` -- CPU chamfer coast-distance bake via `bar_data::coastmap` + engine-shipped foam/waverand textures + cliff-foam term) all in. |
| `MiniMapVertProg.glsl`, `MiniMapFragProg.glsl` | **Ported** (not GUI-wired)   | `shaders/recoil/minimap.wgsl`      | Inspector still draws its own topo view; switch deferred until concrete user complaint. |
| `ModelVertProg.glsl`, `ModelFragProg.glsl`     | **Mostly ported**            | `shaders/features.wgsl`            | Diffuse + SMF-style lighting + shadow + texture2.r emissive + texture2.g spec multiplier + env-cubemap reflection mixed by texture2.g + team-colour mix via texture1.a (`MFP:87-109`). **Remaining**: dynamic point/spot light accumulation (`MFP:42-77`, `MAX_DYNAMIC_MODEL_LIGHTS`). **Deferred** -- engine exposes `Spring.AddMapLight` / `AddModelLight` Lua APIs but BAR's gameplay layer doesn't drive them, no BAR map ships a widget that calls them, and the editor preview has no light-source authoring today. Implement when (a) BAR starts using dynamic lights or (b) BME adds light-emitter authoring. |
| `bar-game/modelmaterials_gl4/templates/cus_gl4.{vert,frag}.glsl` (1377+765 lines) | Not ported -- runtime gated   | (Preview falls back to `features.wgsl` engine-faithful ModelFragProg path) | The CUS GL4 LuaRules gadget (`luarules/gadgets/cus_gl4.lua`, 2417 lines) replaces the engine's stock model shader with a PBR-style replacement carrying unit-specific options (treads, health-displace, flashlights, raptors, scavengers, XMAS, normal-mapping, BRDF/env LUT). Most of those options apply only to live unit rendering (`Spring.AddUnit*` callbacks, real-time team state, dynamic lights) and have no editor-side source today. The Preferences "Advanced Model Shading" toggle gates the engine-faithful ModelFragProg subset already in `features.wgsl` (emissive, spec/env-reflection mix, team colour). A full cus_gl4 port would require asset plumbing for `brdf_0.png` / `envLut_0.png` and per-feature normal maps that BME's project format doesn't yet carry. |
| `SMFShadingTexture{Vert,Frag}Prog.glsl`        | Folded into SMF              | (math inlined in `smf_ground.wgsl`)| n/a                                                         |
| `SMFBorderProg`                                | Not vendored                 | —                                  | Off-map border. Cosmetic; depends on detail tex pipeline that's now in. |
| `BumpWaterCoastBlur*`                          | Not vendored                 | —                                  | Subordinate to shore-foam — see below.                      |
| `ShadowGen*`                                   | Replaced                     | `shaders/shadow_*.wgsl`            | Custom shadow caster + receiver; not a direct port of upstream's path. |

---

## What's actually involved in a port

GLSL→WGSL syntax translation is ~20% of the effort. The remaining 80% is surrounding infrastructure: uniforms, bind groups, vertex layouts, auxiliary textures, and additional render passes.

- **Pure shader math** (lighting formulas, fog mix, water absorption): translatable in hours.
- **Uniform plumbing**: one-time cost per uniform. `CameraUniform` currently carries lighting + water + atmosphere + custom fog + skybox + splat-detail params (see `bar-render::renderer::CameraUniform`).
- **Auxiliary textures**: need the asset itself. For map-authored textures (detail, splat-detail-normal, skyReflectModTex, custom skybox cubemap) we load from the map's archive via `bar-app::viewport::sync_*` helpers. For **engine-shipped** textures (foam, caustic animation, waverand) we have no path yet — see SDP reader item below.
- **Additional render passes**: extra texture + extra draw + extra bind group entry. The planar reflection / refraction pre-passes are the established pattern.

---

## ModernSky — done

- Port: `shaders/recoil/modern_sky.wgsl`
- Uniform inputs: `sun_color`, `sky_color`, `sky_dir`, `cloud_color`, `cloud_density` — all from mapinfo `atmosphere = { ... }`.
- Cloud time scaled by 0.15 / real-second to match engine's `time = frameNum * 0.005f` (at 30 game-FPS).
- Cubemap skybox path: when `atmosphere.skyBox` is set and the DDS loads via `bar-data::load_dds_cubemap`, `fs_sky` samples the cubemap via `skybox_tex` (group 0 binding 1) instead of running the procedural path.

---

## SMF ground — mostly done

Engine-faithful pieces (in `shaders/recoil/smf_ground.wgsl` + `shaders/terrain.wgsl::fs_main`):

- Lighting: `groundAmbientColor + groundDiffuseColor * cos(N·L) * shadow` scaled by `SMF_INTENSITY_MULT = 210/255`. Specular added **on top of** the texture multiply (not into the shade), matching engine `fragColor += specularInt`.
- `SMF_WATER_ABSORPTION`: full path. `SMF_SHALLOW_WATER_DEPTH = 10` elmo transition, `waterMinColor` clamp, `waterAbsorbColor * step_height` gradient, ground-shadow-coeff decay, water-light intensity ramp. Render-Y → elmo conversion via `height_range_elmos` (was wrong by ~75× before).
- Custom-fog height-based tint (mapinfo `custom.fog`) applied as multiplicative tint mix, gated by height in elmos.
- Simple `detailTex` path (`GetDetailTextureColor` non-splat branch): tiled once across the playable area, gated off when the map uses splat detail (engine routes detailTex to its border shader in that case).
- `SMF_DETAIL_NORMAL_TEXTURE_SPLATTING`: full path. Four splat-detail- normal textures sampled at per-channel `splatTexScales`, weighted by `splatDistrTex * splatTexMults`. Alpha of weighted sum drives the detail brightness; RGB rotated through tangent-space basis (`stnMatrix`) and mixed with surface normal by `splatDetailStrength.x = clamp(sum_of_cofacs, 0, 1)`. Y-floor at 0.01 per engine.
- `SMF_SKY_REFLECTIONS`: full path. `reflect(cameraDir, normal)` samples the skybox cubemap, mixed into `diffuseCol` by `skyReflectModTex` per-pixel. Applied AFTER normal perturbation, BEFORE shade multiply (engine order).
- Shadow map: directional-light shadow sampling with hardware PCF (separate from upstream's path but produces equivalent output).

**Still missing (per-feature):**

- `SMF_PARALLAX_MAPPING` — verified not used by any BAR map (`parallaxHeightTex` is referenced only by engine source files in the BAR + RecoilEngine repos). Implementation-cost vs zero-user-benefit → not scheduled.
- `SMF_BLEND_NORMALS` (`detailNormalTex` / `blendNormalsTex`) — perturbs surface normal from a single normal map. Aurelia ships one (`normal.dds` via `detailNormalTex`) but its visible contribution overlaps with `SMF_DETAIL_NORMAL_TEXTURE_SPLATTING`. Worth a follow- up but low priority.
- `HAVE_INFOTEX` — gameplay overlay (FoW, metal, LoS spots). Editor scope.
- Basic `SMF_DETAIL_TEXTURE_SPLATTING` (the simpler 1-texture splat path) — not implemented but rarely used by modern BAR maps; most use the normal-splat path which IS done.

---

## BumpWater — mostly done

Port lives at `shaders/water.wgsl`. Originally an "intentionally original PBR shader" but has been progressively replaced with line-by- line ports from `BumpWaterFS.glsl` as engine fidelity became the target.

**Engine-faithful pieces:**

- 4-octave perlin-style normal sampling via `PerlinAmp` chain (`a, a², a³, a⁴`), Y-flipped from tangent → world space (`.xzy` swizzle per upstream).
- Refraction-pass terrain + features rendered separately with the clip plane keeping the below-water half-space.
- Surface composite: `surfaceColor + diffuseColor * diffuse + vec3(ambient)`, mixed with refraction by `0.1 + surfaceMix * 0.1`. Per `BumpWater.cpp:429,436`, `surfaceColor` is pre-multiplied by 0.4 and `diffuseFactor` by 15.0 in the engine's `#define` substitution — we apply the same pre-scaling on the CPU side.
- Sun specular: anti-Phong gated by view angle (`angle * pow(...)`), multiplied by shadow occlusion.
- Reflection: planar pre-pass mirrored about y = water_y. Mixed via `fresnelMin + fresnelMax * pow(angle, fresnelPower)`, gated by `shallowScale` so shoreline water doesn't pick up the mirror.
- `SMF_WATER_ABSORPTION` underwater terrain rendered into the refraction texture so the seabed is tinted before the surface composite samples it. The texture × shade order matches `(diffuse + detail) * shadeInt`.
- Depth-aware refraction mixback (`BumpWaterFS:304-314`): main pass samples the refraction-pass depth at the distorted UV and replaces with the undistorted sample when the distortion picked up something closer than the water plane (shoreline bleed prevention).
- Camera-depth-attenuated distortion (`BumpWaterFS:291`). Engine formula is `60 * (1 - pow(fragZ, 80)) * shallowScale` pixels, scaled by `1/screen_w` to a UV offset. We translate the pixel constant into a fixed UV magnitude so the distortion strength doesn't scale up at our lower preview render resolutions -- the engine uses 1920px (60/1920 = 0.031 UV) but our previews are 512-1024px where the engine's pixel formula would give 0.06-0.12 UV, visibly stronger than in-engine. Magnitude is tuned to `0.006` UV (~20% of the engine's effective magnitude). The aggressive undershoot compensates for our missing refraction blur (see next item) -- with a sharp refraction texture the same UV offset reads as a louder ripple than the engine's blurred sample. The `pow(z, 80)` depth gate is preserved so far-camera fragments still attenuate to zero (this is what prevents the "Azurite ocean = spilled milk" deep-water blowout).
- Reinhard tonemap with white-point 4 to keep HDR contributions from saturating to white. Engine doesn't tonemap; without it the engine- faithful HDR-ish math would saturate in our 8-bit framebuffer.
- Height-based custom fog applied via the same path as the terrain shader so the surface composite picks up the same cool cast.

**Still missing (per-feature):**

- **Shore foam** (`GetShorewaves`) — needs the engine-shipped `foam.png`
  + `waverand` textures plus a coast-distance map. See SDP reader prerequisite below.
- **Caustics** — needs the engine-shipped `caust00..caust31.tga` animation sequence. Same prerequisite.
- **Refraction blur** (`BumpWaterCoastBlur` — `coastDistance` / depth-aware Gaussian over the refraction texture). Engine runs this before BumpWater samples the refraction texture so the per-pixel refraction sample is already softened. Without it we sample a sharp refraction texture and the same UV offset reads as a louder ripple than in-engine -- which is why our refraction UV magnitude has to be cranked down to ~20% of the engine's effective UV to look comparable. Adding the blur would let us bring the UV magnitude back up to the engine's value and recover crisp ripple visibility without the "everything underwater is a wobbly mess" look.
- **Multi-tap reflection blur** (`opt_blurreflection`) — 7 extra reflection samples + `BlurBase` / `BlurExponent` uniforms. Pure shader-side, no new textures. Engine treats it as a user quality setting (`springsettings.cfg::BumpWaterBlurReflection`), not a per-map property. Implement when the editor has a "preview quality" toggle.

---

## Prerequisite: SDP reader

`Shore foam`, `caustics`, and any future port of engine-shipped assets (unit shaders, GUI icons, etc.) all need read access to BAR's content archives. BAR distributes content as `.sdp` files (Spring Pool format) under `<install>/data/packages/`, indexed by SHA-1 hash and resolved through a `pool/` directory of compressed-by-hash blobs.

### What an SDP reader has to do

1. Read the .sdp header + filename table (zlib-compressed file list with SHA-1 hashes).
2. For each requested filename, locate the corresponding hash in the `pool/aa/bbccdd...gz` layout (first byte of the hash is the directory prefix).
3. zlib-decompress the pool blob to recover the raw asset bytes.
4. Hand the bytes to the existing `bar-data::load_dds_cubemap` / `bar-app::viewport::load_2d_image` decoders.

### Where to put it

- New module: `crates/bar-data/src/sdp.rs`
  - `pub struct SdpArchive { ... }` with `open(path)` / `read_file(name) -> Result<Vec<u8>, ...>`.
  - `pub fn find_in_sdps(install_dir: &Path, filename: &str) -> Option<Vec<u8>>` that walks all .sdp files under `<install>/data/packages/` looking for the asset.
- Integration: `bar_install::BarVersions::detect()` already locates the BAR install — `runner.rs` stores the result. Plumb that path into the renderer's asset loaders.

### Reference

- Engine source: `rts/System/FileSystem/Archives/PoolArchive.{h,cpp}` in `beyond-all-reason/RecoilEngine`.
- The SDP filename table format is documented in the same file: each entry is `[name_len: u8][name: utf8][md5: 16 bytes][crc32: 4 bytes][size: u32]` (note: MD5 in the table, but the *pool* keys files by SHA-1 of the decompressed bytes — read the engine source carefully).

### Deferred work that unblocks once this lands

| Feature                            | What it needs from SDP                                       |
| ---------------------------------- | ------------------------------------------------------------ |
| **Shore foam** (`GetShorewaves`)   | `bitmaps/foam.png`, `bitmaps/waverand.png`. Plus a coast map generated from the heightmap (`coast_distance(x,z) = distance to nearest above-water cell`) — that part is CPU-side and doesn't need the SDP, but the foam pass is dead without the foam texture. |
| **Caustics**                       | `bitmaps/caustics/caust00.tga` … `caust31.tga` (animation sequence). The waterdepth gate uses the same coast map as foam. |
| **SMF_BLEND_NORMALS**              | Nothing engine-shipped — the normal texture is map-authored. No SDP dependency; just hasn't been scheduled.       |
| **Multi-tap reflection blur**      | Nothing engine-shipped (pure shader). No SDP dependency.     |
| **SMFBorderProg**                  | Nothing engine-shipped — uses map textures. No SDP dependency. |

---

## Display preferences

The Preferences > Display section exposes three runtime toggles
that BME forwards into `TerrainRenderer` per render-call. All three
are user preferences -- persisted in `settings.json` under the
per-user config directory, NOT in any project file -- so toggling
them in one project carries to the next.

### Design rule

**These toggles only ever RAISE quality when on.** Turning a toggle
off must never strip a path that BME's baseline already renders;
that would let the toggle reduce fidelity below the baseline, which
is the opposite of what an "Advanced..." opt-in should do. The
Sculpt layout therefore does NOT force these toggles off as a
performance shortcut -- the renderer runs the engine-faithful path
unconditionally in both Sculpt and Preview.

The single exception is **Grass**, which is a legitimate
performance toggle: the user explicitly asked for grass to be off
in Sculpt regardless of the preference. Grass is a discrete widget
draw (~thousands of instanced blade quads), not part of the
baseline SMF terrain rendering, so suppressing it in the
high-performance authoring view doesn't violate the rule.

| Setting                      | Behaviour                                                                                                                                          | Sculpt forced off?                            | Engine analogue                                |
| ---------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------- | ---------------------------------------------- |
| **Grass**                    | Issue the `map_grass_gl4` widget draw.                                                                                                             | yes (Sculpt = always off, Preview = pref)     | `mapinfo.custom.grassConfig` widget            |
| **Advanced Map Shading**     | Reserved gate for FUTURE engine SMF paths BME doesn't yet implement (parallax, infotex, etc.). Currently a no-op on the rendering side.            | no -- toggle does not strip the baseline      | `springsettings.cfg::AdvMapShading` config     |
| **Advanced Model Shading**   | Reserved gate for the eventual `cus_gl4` port (PBR + normal maps + BRDF / env LUT + dynamic lights). Currently a no-op on the rendering side.      | no -- toggle does not strip the baseline      | `cus_gl4.lua` LuaRules toggle                  |

### Why the two "advanced" toggles are stubs today

BME already renders the engine's stock SMF advanced paths
(`SMFFragProg.glsl` -- splat detail-normal, sky reflections,
per-pixel specular, light emission, detail-normal blend, basic
splat, detail tex, sun specular) and the engine's stock model path
(`ModelFragProg.glsl` -- texture2 emissive, spec multiplier,
env-cubemap reflection, team-color). That IS the baseline. The
toggles are reserved for the next quality tier above the baseline:

- **Advanced Map Shading** would attach engine paths BME hasn't yet
  built. Parallax and infotex are listed as deferred above; both
  have specific blockers (verified non-issue, gameplay overlay
  scope) so this slot stays a stub until a concrete enhancement
  scope appears.
- **Advanced Model Shading** would drive a real cus_gl4 PBR port
  (see the cus_gl4 row of the status table for the gap). That
  requires asset plumbing for `brdf_0.png` / `envLut_0.png` and
  per-feature normal maps that BME's project format doesn't yet
  carry, plus a substantial new shader.

The flags thread through the renderer (`TerrainRenderer::
advanced_map_shading` / `advanced_model_shading`) and the uniform
layout (`terrain_detail_params.zw`) so the eventual implementations
can attach without re-plumbing. Shaders currently ignore them.

## Verified non-issues

Items the engine "supports" but that no BAR map actually uses, per a search of the `beyond-all-reason` GitHub org:

- **Parallax mapping** (`parallaxHeightTex`) — referenced only by engine source. No mapinfo in any map sets it. Not scheduled.

---

## Refresh procedure

When upstream Recoil moves and we want to take new versions:

1. `bash vendor/recoil/sync.sh` against a fresh local Recoil clone.
2. Update `vendor/recoil/UPSTREAM.md` with the new commit hash.
3. Diff the changed GLSL files against our WGSL ports; reflect any semantic changes. Mechanical syntax updates (renames, refactors) can usually skip.
4. Commit the sync and port updates separately.

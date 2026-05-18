# TODO

Flagged items not yet implemented. Public, committed, and intended as the durable backlog -- distinct from per-session AI memory (which expires) and from `.github/instructions/*.instructions.md` (which describes the code as it currently exists, not what it should become).

## How to use this file

**Add an item** when it's a real, scoped piece of work that isn't going to happen in the current session. Drop it in the most relevant section, or make a new section if none fits. Include enough context (file paths, function names, the symptom that motivated it) that anyone can pick it up cold months later.

**Mark an item DONE** by removing it from this file when the work lands. Don't leave a graveyard of completed items -- the value of the file is in being scannable. If a completed item is worth remembering (non-obvious decision, surprising trade-off), capture it in a `docs/` file describing the actual implementation; this file is for "not yet."

**Remove an item** without doing it if it turns out to be wrong, no longer applicable, or superseded by a different approach. A one-line note in the commit message is enough; don't leave tombstones.

**Cross-reference plans**: when a body of related work has its own plan in `docs/` (e.g. `feature-rendering-plan.md`), an entry here can just be a pointer to that file instead of duplicating its content.

**Don't track here**: per-session debug notes, in-progress design discussion, hypothetical features the user hasn't actually asked for. The bar is "the user wants this and we're not getting to it right now."

## On hold

Items below are paused while the 3D-painting / brush subsystem is being reconsidered. The Sculpt3D layout itself is back in the UI (for feature placement), but the Composition Layers panel that exposes the brush flow is commented out in `crates/bar-gui/src/layouts/sculpt3d.rs`. Re-enabling those items means reaching agreement on the brush-flow direction first.

- **Sculpting the heightmap via the brush tools crashes the editor.** When the Composition Layers panel is re-enabled and the user selects a brush (Raise/Lower/Smooth/Flatten) + drags in the Sculpt3D viewport, BME reliably crashes. Need to capture the panic backtrace to know whether it's a renderer issue (heightmap upload while the previous frame is still in flight?), a brush-math issue, or a graph-eval invariant being broken by mid-stroke mutation.
- **Metalmap density overlay in the renderer.** The metalmap sculpt layer writes to `SculptState.metal_overlay` and persists as `sculpt-metal.png` sidecar, but the 3D viewport doesn't render a visible overlay yet. Was deferred behind embedded-viewport (Phase F). The viewport has since been embedded; this is now unblocked but unscheduled.
- **Typemap colour overlay in the renderer.** Same shape as the metalmap overlay -- data is captured, persisted, and editable; the visual overlay isn't rendered.
- **Multi-layer FinalComposition stacks (heightmap + color).** Today FC has exactly one paint slot per kind. Heightmap and color should support an ordered list of layers (each with its own asset id / path, plus a display name) so the user can add / remove / select layers in Sculpt3D the way a Photoshop layer stack works. Metalmap and typemap stay single-slot (their values are quantised; multi-layer with opacity doesn't make sense). Touches: FC default params (need indexed schema -- `heightmap_layer_count: UInt`, plus per-slot `heightmap_layer_<i>_{id,path,name}`), `executor.rs::composite_heightmap_layer` / `composite_color_layer` (walk the full stack in order rather than reading a single asset_path), `persistence.rs::resolve_relative_paths` + `pack_assets_for_save` (loop over the slots), brush flow (selected target becomes `(kind, slot_index)` not just `kind`), Sculpt3D layer panel (list view per kind with `+` to add, click to select, right-click to delete). No opacity / blend modes in v1 -- just additive heightmap deltas and over-the-top alpha-mask colour.
- **Replace tool-palette text buttons with icons.** The brush-tool selector in the sculpt layout (`crates/bar-gui/src/layouts/sculpt3d.rs` -- Pointer / Raise / Lower / Smooth / Flatten etc.) uses text labels. Standard editor convention is icons in a compact toolbar. Pick / draw / source SVG-or-raster icons per tool (raise = up-arrow, lower = down-arrow, smooth = blur-circle, flatten = horizontal-line, pointer = arrow cursor, paint = bucket). Add tooltip on hover for tool name + shortcut. Lives in the same place that currently renders the text buttons.
- Plan doc: `docs/3d-painting-plan.md` (the broader subsystem plan these items belong to).

## UX / UI

- **Action-bar grouping: centered group label variant deferred.** The separator + reorder version of the build-group layout shipped, but the original UX question was "separator OR centered group label above each group (action bar gets taller)". The label variant wasn't prototyped; revisit if the separator alone doesn't read clearly.
- **Expose shader debug visualisation toggles in the viewport gear menu.** The terrain shader has a small family of `DBG_VISUALIZE_*` and `DBG_*` constants (`shaders/terrain.wgsl`) that short-circuit the fragment output for diagnostics -- splat-detail channels, spec lobe, sky reflection, normal perturbation, detail color, etc. They're currently hand-edited as `const ... = true | false` and require a rebuild. Convert them to a runtime uniform (one `u32` bitfield on `CameraUniform` -- both the Rust struct in `crates/bar-render/src/renderer.rs` and the WGSL struct in `shaders/terrain.wgsl` need the new field, plus the `size_of` assert at the bottom of the Rust struct definition needs the new total). The gear menu already exists at `viewport.rs::draw_viewport_debug_overlay`; add a single-select dropdown there (off / each enable-disable variant / each viz mode) once the uniforms are in place. CameraUniform is built in five places in `renderer.rs`; all need the new field.

## Preview view

- **Implement non-map "surrounding terrain".** In-engine, the map sits inside a larger landscape that fills the horizon -- the visible terrain extends well past the map boundary so the world doesn't end abruptly at a cliff. BME's preview viewport currently renders only the map itself, with no surrounding context, which makes the boundary look like a void. Need to generate or fake a surrounding skirt -- the engine approach is a flat extension with the edge heightmap row/column extruded outward, textured with the map's edge splat/detail tile.

## Rendering: terrain / shaders

- **Feature shadow shape.** Small features (trees, crystals) have poorly-shaped shadows. Bilinear feature placement (the "attachment" half) is fixed; the remaining issue is the shadow silhouette itself. Likely needs a cascade or camera-focused shadow frustum rather than the current full-map orthographic one (current setup: `crates/bar-render/src/shadow.rs`, 4096^2 shadow map covering the whole map at uniform per-texel coverage).
- **Chunked terrain rendering (for 32+ block maps).** Current renderer uses a single grid mesh capped at 8192^2 vertices. At the largest shipped BAR maps (64-block / 8193 native heightmap) that's ~3.7GB of vertex+index data. Engine handles this via spatial chunks (`CSMFGroundDrawer` / `SMFGroundTextures`) -- only visible patches are tessellated at full density. Deferred until a real memory ceiling is hit (current is fine on dev hardware).
- **Metal-spot specular blowout on Ascendency (reverse direction + saturation).** Two related symptoms on Ascendency's metal-pad patches: (1) at certain camera angles the spec lobe saturates to pure white across the whole metal-pad region, more aggressively than in-game which shows a brighter-but-bounded response; (2) the camera angles that produce bright vs dim spec appear inverted compared to in-game -- bright when looking away from the sun rather than toward it. Investigation done: confirmed `specularTex` (ASC_speculartex.dds) loads correctly; per-pixel spec_color + spec_exp from the texture are in use (not the global fallback); confirmed `sync_splat_textures` now uploads correctly after the missing-channel fix (`Ice_1k_dnts.tga` absent from .sd7 was killing the whole splat path). Tried negating `sun_dir.z` (no change), then `sun_dir.x` (made spec only visible from underground -- wrong direction). Shader math matches engine exactly (same Blinn-Phong, same `splatDetailStrength.x = min(1.0, dot(splatCofac, vec4(1)))`, same `mix(normal, perturbed, strength)`, same `(texture * shade) + spec` order). Unresolved candidates: (a) perturbed-normal tilt direction differs from engine (possibly a `splat_tex_mults` sign or a tangent-basis derivative-sign difference at slopes); (b) detail-normal sampling UV mirrored vs engine for non-flat terrain. Both need an A/B render against an in-engine debug capture of the same viewpoint to disambiguate; not solvable from shader-side toggle tests alone. Debug viz `DBG_VISUALIZE_SPEC` in `shaders/terrain.wgsl` already wired to output R = spec_color.r, G = spec_sample.a, B = cos_specular when investigating further.
- **Splat detail-normal CPU mip quality.** When the source splat detail-normal is NOT a DDS with a baked mip chain (rare; nearly all BAR maps ship DDS), `renderer.rs::ensure_full_mip_chain` falls back to a 2x2 box filter on sRGB-encoded RGBA8 bytes without normal renormalisation. The DDS path is fine (artist-baked). Improving the fallback would mean: (a) operate in linear space, (b) renormalise per output texel for the normal channels, (c) optionally Kaiser / Lanczos kernel. Only worth doing if a non-DDS splat texture appears in the wild.

## Rendering: shader-port catalogue

See `docs/recoil-shader-ports.md` for the full status table and per-feature context. Still-open ports cross-referenced here so they show in one backlog place:

- **`SMF_BLEND_NORMALS`** (`detailNormalTex` / `blendNormalsTex`) -- single-normal-map perturbation. Aurelia ships one but its visible contribution overlaps with the splat-detail-normal path that IS done. Low priority, but worth a quick check on maps that ship the texture without the splat variant.
- **Basic `SMF_DETAIL_TEXTURE_SPLATTING`** -- the simpler 1-texture splat path. Most modern BAR maps use the 4-texture normal-splat variant which is done; the simple path appears on older maps.
- **`HAVE_INFOTEX`** -- gameplay overlay (FoW, metal, LoS spots). Documented as out-of-scope for the editor preview in `recoil-shader-ports.md`; tracked here in case scope changes.
- **Shore foam** (`GetShorewaves`) -- needs SDP reader (see below) for `foam.png` / `waverand` plus a CPU-computed coast-distance map.
- **Caustics** -- needs SDP reader for `caust00..caust31.tga` animation sequence.
- **Refraction blur** (`BumpWaterCoastBlur`) -- engine blurs the refraction texture before BumpWater samples it; we currently compensate by undershooting the distortion UV magnitude to ~20% of engine's. Adding the blur would let the UV magnitude come back up to engine's value and improve ripple crispness. Pure shader-side pass; no asset dependencies.
- **Multi-tap reflection blur** (`opt_blurreflection`) -- 7 extra reflection samples + `BlurBase` / `BlurExponent` uniforms. Pure shader. Engine treats it as a user quality setting.
- **MiniMap port wiring** -- shader is ported (`recoil/minimap.wgsl`) but not GUI-wired. Inspector still draws its own topo view. Switch when there's a concrete user complaint about the topo view.

## Engine asset access

- **SDP reader.** Prerequisite for shore foam, caustics, and any future port of engine-shipped assets (unit shaders, GUI icons, etc.). Format details and engine references are in `docs/recoil-shader-ports.md` under "Prerequisite: SDP reader." Planned location: `crates/bar-data/src/sdp.rs`. Not started.

## Features

See `docs/feature-rendering-plan.md`. M1/M2/M3 are complete.

### Editor UI

- **"Features" action bar entry for managing map-bundled features.** Add a new top-level action bar button that opens a management view for the custom features included in this map's data (the `features/`
  + `objects3d/` + `unittextures/` subtrees copied into the `.barproj` on save). Should let the user list what's bundled, inspect each entry, and add / remove entries. Goes alongside the existing Compile / Bundle / Test-in-BAR / Edit-Map-Info buttons in `crates/bar-gui/src/layouts/shell.rs::TopBottomPanel::top("action_bar")`.

### M4 rendering polish

- LOD: suppress features below a screen-size threshold when zoomed out.
- Features in the reflection / refraction passes. Refraction-pass feature rendering is partially in (so underwater features show through water); reflection-pass is not. Verify and finish.
- **Geovent feature rendering.** Geovents (geothermal vents -- maps designate them as metal-spot-like features that grant continuous energy) currently render as the placeholder yellow cube rather than the engine's animated geovent geometry/effect. Engine renders them as procedural circle of vertices with steam/heat-haze effect, not as an S3O model. Needs a dedicated geovent visualisation path (likely a billboard or screen-space ring + heat shimmer) since there's no S3O to load.

## Logging

- **Dedicated Status log channel (or separate stream) for foreground operations.** The info-level audit landed (about a 75% demotion), so the BME log panel's INF view is now focused on milestone events. Worth revisiting later: should there be a dedicated *Status* log type / channel for things the user is actively tracking (current bundle step, current export progress) so they read top-of-mind, rather than mixing with general info? Today both share the INF level.

## Project / workflow

- **Recipe-owned config files: expand parser coverage + add export regenerators.** Architectural rule (see `docs/bar-map-format.md`): every known map-archive config file should parse into the recipe at SD7 import time, be edited via structured UI, and be regenerated on export. Such files do NOT live in `.barproj/passthrough/` -- the recipe is the source of truth. `mapinfo.lua` is the first one done (filter in `bar_engine::extract::is_recipe_owned_config`; existing parser in `bar_project::apply_mapinfo_overrides`; Map Settings modal covers each parsed field; bundler regenerates via `generate_mapinfo` on export). Remaining work to bring more files under the same rule:
    1. **Extend `apply_mapinfo_overrides` coverage.** Doc lists fields the parser still ignores: `grass = { ... }` table (blade dimensions / wave / colour / strawcount), `terrainTypes = { ... }` (per-id move-speed modifiers referenced by the typemap), water long tail (`waveLength`, `repeatX/Y`, `causticsResolution/Strength`, `shoreWaves`, `waveFoamIntensity`, `waveOffsetFactor`, `blurBase/Exponent`, `perlinStart`, `perlinLacunarity`), atmosphere `fluidDensity` + `skyAxisAngle` (currently stored as separate `sky_dir`), top-level scalars (`autoShowMetal`, `modtype`, `notDeformable`), `smf` block (`grassmapTex` -- engine resolves the others). Each new field needs a MapSettings home + parser case + Map Settings UI control.
    2. **`mapoptions.lua` parser + Map Settings UI + export regenerator.** Schema: array of `{ key, name, desc, type, def, min, max, step, maxlen?, items? }`. Will need a new sub-struct on MapSettings (or sibling on Recipe) plus a list-editor in the Map Settings modal. Once landed, add to `is_recipe_owned_config`.
    3. **`mapconfig/map_startboxes.lua`.** Schema in `docs/bar-map-format.md` "Startboxes" section. Map per-ally-team to polygons + start points. Recipe already carries `start_positions` (single per-team point); startboxes are the richer polygon form. Probably belongs in a new sub-struct on the recipe.
    4. **`mapconfig/featureplacer/*` (`set.lua`, `featureplacement_set.lua`, `config.lua`).** Feature placement data. Today BME's `PlacedFeature` already structurally models placements but the importer only reads the SMF feature section; need to fall back to `mapconfig/featureplacer/set.lua` for maps that ship features via the gadget instead. Export regenerator emits the appropriate variant the user chose (or both, for compatibility).
    5. **`mapconfig/map_metal_layout.lua`.** Custom metal-spot layout. Recipe currently has no place for this. Decide: new sub-struct, or a graph node that emits placements?
    6. **`mapconfig/mapinfo/0_apply_options.lua`.** Modifies mapinfo at parse time based on `mapoptions` choices. Cross-cutting; probably needs the mapoptions parser landed first.

  Until each item lands, those files stay in passthrough (verbatim cargo) and the bundler reads them from there on export. After each item lands: add the file pattern to `is_recipe_owned_config` so future imports skip the passthrough copy.

- **PassThrough vs. Edit Map Info: resolve the overlap.** The PassThrough node was the original way to ship miscellaneous files (mapinfo.lua, splat textures, skybox, specular, sky-reflect mask, detail tex, LuaGaia scripts, etc.) into the bundle. Since then the Edit Map Info panel has taken over the textures listed in mapinfo (`MapSettings.resources.*`), which means there are now two sources of truth for "is this texture part of the map":
    1. The PassThrough node's `files` param (graph topology).
    2. `MapSettings.resources` and the file actually living in `<barproj>/passthrough/` (Edit Map Info panel + disk state). Symptom: disconnecting PassThrough from Bundler affects only the *export* (the disconnected files won't be packaged into the bundle), but the *preview* keeps loading and rendering textures whose filenames live in `app.map_settings().resources.*` -- the `sync_skybox` / `sync_detail_texture` / `sync_splat_textures` / `sync_sky_reflect_mod` / `sync_specular_tex` functions in `crates/bar-app/src/viewport.rs` read those filenames directly, independent of graph topology.

  Two cleanups worth considering:
    - **Narrow PassThrough's role.** For the map-info-managed textures, the Edit Map Info panel should be the single source of truth -- removing them from PassThrough's `files` list (or auto-driving that list from `MapSettings.resources`). PassThrough then only ships truly miscellaneous files: Lua gadgets, mapconfig scripts, sound, etc.
    - **Or: tie sync to graph reachability.** Add a helper on `GraphEngine` like `bundle_files_reachable_from(bundler_id) -> HashSet<String>` that the sync layer consults, so disconnecting PassThrough makes the preview blank the affected textures. The first cleanup is the deeper fix; the second is a band-aid.
- **Classify remaining PassThrough-only files: Edit Map Settings vs node-driven.** A bunch of files currently only reach the bundle via the PassThrough node (Lua gadgets, mapconfig/featureplacer data, mapoptions, sound configs, custom feature S3O references that aren't in MapSettings.resources, etc.). Decide per-category which belong in the structured Edit Map Settings panel (mapinfo-derived fields, lobby-visible options, settings-shaped data) and which should be exposed as proper graph nodes (placement / generation data that benefits from upstream parameterisation). Anything left over stays as raw PassThrough cargo. Output should be a list per category with a routing decision; informs the PassThrough-overlap cleanup above.

## Open investigations

- **Misplaced dots on Ascendancy ground.** Visible in the 3D preview; user described them as "splat details that are supposed to be on top of metal spots, misaligned." Bisected: NOT from splat rendering (disabling `DBG_SPLAT_NORMAL_PERTURB` + `DBG_SPLAT_DETAIL_COLOR` doesn't make them go away). Possible candidates: metalmap rendering / orientation, feature placeholder positioning, or a separate 2D overlay. Not investigated further; needs descriptive info from the user (size, color, ground-attached vs screen-space) plus a fresh screenshot.
- **Minimap override resolution rules.** `docs/bar-map-format.md` previously claimed `maps/minimap.bmp|.dds` is THE override path; reality is there are several (at least three) ways to override the minimap. Research the precise resolution rules, including the SMF-embedded copy, the loose `maps/minimap.*` file, and at least one other path. Source to chase: Moose's answer in the BAR Discord `#mapping` channel on 2026-05-17. Update the doc with the verified rules + remove the placeholder note in the file-layout table.

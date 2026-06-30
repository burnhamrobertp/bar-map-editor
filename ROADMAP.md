# Roadmap

Status of the editor. "Done" means it works end-to-end; "In progress" means
partially built with known gaps (including subsystems paused mid-build, marked
"on hold"); "Up next" is the planned short-term work; "Deferred" is known but
not scheduled.

---

## Done

### Core editing
- **Node graph** -- 59 node types (noise generators, erosion, blend/combine,
  mask, colorize, splat/maps, layout, import/export); subgraph nesting with
  collapsible IO-boundary nodes; macro presets; node palette with text search;
  stable recipe key IDs; connection type validation
- **Layout node** -- composites primitives (ellipse / rectangle / ridge) and
  Catmull-Rom splines (open paths or closed filled regions) into a heightmap;
  per-item height + falloff; ridge / valley / mask modes; mirror / rotate
  symmetry. Edited through a 2D canvas in the node edit view with a live lit
  3D preview (shared `TerrainPane`)
- **Project format** -- `.barproj` JSON + sibling `.assets/` dir; portable,
  version-controllable; full undo/redo
- **SD7 import/export** -- Spring SMF/SMT binary codecs; metalmap, typemap,
  minimap, normalmap, texture, grassmap; mapinfo.lua generation from
  `MapSettings`; 7z archive packaging
- **Final Composition node** -- singleton terminal node that composites paint
  layers (heightmap delta, color RGBA, metalmap, typemap) over the procedural
  graph and feeds the exporter; auto-created at bootstrap, can't be deleted.
  Replaces the former Sculpt node and Bundler
- **Feature placement** -- loads the BAR feature catalog (S3O models) from game
  archives; Add Features palette with on-disk-cached thumbnails; click-to-place,
  drag-to-move, Ctrl+wheel rotate, all undoable; placements round-trip through
  SMF import/export
- **2D inspector** -- heightmap topo view; draggable start-position markers;
  metalmap / typemap toggle layers
- **Structured mapinfo editor** -- form-based editor for all `mapinfo.lua`
  fields (dimensions, physics, atmosphere, water/lava, lighting, DNTS, spawns);
  split into per-section action-bar modals; round-trips through project
  save/load
- **Validation panel** -- pre-export checks: spawn positions on playable land,
  metalmap/typemap dimension parity, mapinfo references, height-range sanity,
  bundler-with-no-inputs, orphan nodes, archive-path collisions
- **Test-in-BAR launcher** -- detects BAR install via Preferences; exports
  project to temp dir; copies SD7 into a fixed BME-owned slot in the BAR maps
  dir; spawns lobby
- **Welcome panel** -- preset cards (rolling hills, mountains, archipelago,
  etc.) on empty canvas; recent files menu
- **Progressive preview** -- low-res (96 grid, 128px eval) fires immediately;
  high-res (up to 2048 grid, 512px eval) fires after 300ms cooldown
- **GPU vertex displacement** -- static flat grid + heightmap texture in vertex
  shader; sub-region texture uploads for per-brush-dab feedback without mesh
  rebuild; central-difference normals computed in shader
- **Engine-faithful rendering** -- Recoil `ModernSkyVS/FS` port for sky and
  atmospheric fog; `SMFFragProg/VertProg` ground port (Blinn-Phong, underwater
  absorption, per-pixel specular, splat detail + DNTS detail-normals, light
  emission glow); shadow mapping (terrain + feature depth-caster passes,
  mapinfo `groundShadowDensity`); `SMFBorder` map-edge extension (mirror
  geometry, curvature bend, horizon fog); PBR water (GGX specular, Schlick
  Fresnel, planar reflection pre-pass, refraction, Snell's window, caustics,
  shore foam from a CPU coastmap bake); lava as an opaque emissive surface;
  grass-widget rendering; brush cursor ring overlay
- **GPU compute** -- wgpu compute shaders for noise and erosion; CPU fallback
  when no discrete GPU

---

## In progress

### Sculpt / paint brush flow (on hold)
The FinalComposition paint-layer brush UI -- height raise/lower/smooth/flatten,
color stamping, metalmap / typemap value stamping, and the layer selector in
the Sculpt3D layout -- is paused. The Composition Layers section in
`sculpt3d.rs` is commented out, so Sculpt3D is currently scoped to feature
placement only. Paint-layer storage, the brush-dispatch wiring, and sidecar
persistence (`sculpt-metal.png`, `sculpt-type.png`) remain in place so the flow
can be reattached without re-threading the UI.

### Water shader assets
The water normal map is a 128x128 procedurally generated approximation
(`make_water_normal_map`). A real tiling normal texture from BAR's content tree
(`cont/base/maphelper/`) would improve ripple quality. (Refraction, caustics,
and shore foam are now implemented; only the procedural-normal substitution
remains.)

---

## Up next

### Pathing / movement-class overlay
Compute and visualise BAR-style terrain passability: slope thresholds per
movement class, water-depth thresholds, impassable cliff detection. Overlay
in both the 2D inspector and 3D viewport. Reference: Recoil pathing module
slope tables.

### Minimap shader wired to inspector
`shaders/recoil/minimap.wgsl` is ported (and parse-tested) but not yet driving
the 2D inspector view. The inspector currently uses a CPU topo-gradient render.
Switching to the ported shader gives engine-accurate coloring and avoids the
separate CPU render path.

---

## Deferred

- **Parallax / relief mapping** -- per-fragment parallax on terrain; the
  renderer reserves uniforms and bind-group slots but does not sample them
- **World Machine `.tmd` import** -- deferred behind World-Machine-side format
  fragility
- **Non-BAR Spring/Recoil targets** -- architecture stays pluggable but no
  active work; BAR is the sole export target
- **Multiple export formats** -- only `.sd7 / spring-smf` today; other archive
  or target formats deferred
- **Batch headless export** -- `bar-cli` does single-project eval + export;
  multi-project batch scripting is not planned

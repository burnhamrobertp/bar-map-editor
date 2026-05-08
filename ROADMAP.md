# Roadmap

Status of the editor. "Done" means it works end-to-end; "In progress" means
partially built with known gaps; "Up next" is the planned short-term work.

---

## Done

### Core editing
- **Node graph** -- 34 node types (noise generators, erosion, blend, mask,
  texture, splat, import/export); subgraph nesting; stable recipe key IDs;
  connection type validation
- **Project format** -- `.barproj` JSON + sibling `.assets/` dir; portable,
  version-controllable; full undo/redo
- **SD7 import/export** -- Spring SMF/SMT binary codecs; metalmap, typemap,
  minimap, normalmap, texture, grassmap; mapinfo.lua generation from
  `MapSettings`; 7z archive packaging
- **Sculpt overlays** -- height delta, metalmap, typemap, texture sidecar PNGs
  merged onto the base heightmap at export; overlay persists across save/load
- **2D Sculpt pipeline node** -- delta-buffer node in the graph (u8 per pixel,
  128 = no change); infers layer (heightmap/metalmap/typemap) from downstream
  Bundler port; painting UI driven off the same infrastructure as PaintedHeightmap
- **2D inspector** -- heightmap topo view; draggable start-position markers;
  metalmap / typemap toggle layers
- **Structured mapinfo editor** -- form-based editor for all `mapinfo.lua`
  fields (dimensions, physics, atmosphere, water, lighting, DNTS, spawns);
  round-trips through project save/load
- **Validation panel** -- pre-export checks: spawn positions on playable land,
  metalmap/typemap dimension parity, mapinfo references, height-range sanity,
  bundler-with-no-inputs, orphan nodes, archive-path collisions
- **Test-in-BAR launcher** -- detects BAR install via Steam path; exports
  project to temp dir; copies SD7 into BAR maps dir; spawns lobby
- **Welcome panel** -- preset cards (rolling hills, mountains, archipelago,
  etc.) on empty canvas; recent files menu
- **Progressive preview** -- low-res (96 grid, 128px eval) fires immediately;
  high-res (up to 2048 grid, 512px eval) fires after 300ms cooldown
- **GPU vertex displacement** -- static flat grid + heightmap texture in vertex
  shader; sub-region texture uploads for per-brush-dab feedback without mesh
  rebuild; central-difference normals computed in shader
- **3D sculpting** -- heightmap raise/lower/smooth/flatten brushes with radius,
  strength, falloff; live preview via dirty-rect heightmap region upload; color,
  metalmap, typemap paint layers previewed via synthesised albedo (via floating
  preview window; embedded Sculpt3D layout viewport is not yet wired)
- **Engine-faithful rendering** -- Recoil `ModernSkyVS/FS` port for sky and
  atmospheric fog; Recoil `SMFFragProg/VertProg` port for ground lighting
  (Blinn-Phong, underwater absorption); original PBR water shader (GGX
  specular, Schlick Fresnel, 4-octave scrolling normals, planar reflection
  pre-pass); brush cursor ring overlay in shader
- **GPU compute** -- wgpu compute shaders for noise and erosion; CPU fallback
  when no discrete GPU

---

## In progress

### Sculpt3D embedded viewport
The Sculpt3D layout (`sculpt3d.rs`) has its full sidebar UI -- layer selector
(Height/Colour/Metal/Type), tool selector, radius/strength/falloff sliders,
and a sculpt-layer status panel. The 3D viewport (`draw_viewport_on` in
`bar-app`) currently lives as a floating `egui::Window` launched from the
standard layout. The Sculpt3D layout's central panel is a placeholder. Wiring
the renderer into that panel is the next step to deliver the integrated one-
screen sculpting experience described in `docs/3d-painting-plan.md`.

### Metal / typemap brush persistence
Metal and typemap brush strokes write to `SculptState` (`paint.sculpt.metal_overlay`,
`paint.sculpt.type_overlay`) and are persisted as sidecar PNGs (`sculpt-metal.png`,
`sculpt-type.png`) alongside the `.barproj` on every save. This approach differs
from the `MetalSculpt` / `TypeSculpt` graph-node design in the painting plan doc;
the sidecar model was what was actually built. The strokes survive save/load correctly.
The remaining gap is that these overlays are only visible as synthesised viewport
tinting -- there is no per-type-id colour legend or metal density gradient shown
in the embedded Sculpt3D viewport (which itself is not yet wired; see above).

### Water shader assets
The water normal map is a 128x128 procedurally generated approximation. A real
tiling normal texture from BAR's content tree (`cont/base/maphelper/`) would
improve ripple quality. Refraction, shorewaves, and caustics are not
implemented (opaque water, no foam or coast blending).

### SMF ground shader gaps
Shadow map is hardcoded to 1.0 (no directional shadow). Detail texture
(`detailTex`), DNTS splat channels, and parallax normals are not sampled
(asset pipeline gap, not a shader gap -- uniforms and bind group slots exist).

---

## Up next

### Sculpt3D embedded viewport (closes the integrated sculpting experience)
Wire `draw_viewport_on` (currently a floating window in `bar-app`) into the
central panel of the Sculpt3D layout so sculpting happens in a single full-
screen view. The sidebar controls, layer selector, and brush dispatch are
already in place; only the central panel and the input routing need to connect.

### Pathing / movement-class overlay
Compute and visualise BAR-style terrain passability: slope thresholds per
movement class, water-depth thresholds, impassable cliff detection. Overlay
in both the 2D inspector and 3D viewport. Reference: Recoil pathing module
slope tables.

### Node palette search
Text filter on the node creation palette. Simple substring match on node name
and category.

### Minimap shader wired to inspector
`shaders/recoil/minimap.wgsl` is ported but not yet driving the 2D inspector
view. The inspector currently uses a CPU topo-gradient render. Switching to
the ported shader gives engine-accurate coloring and avoids the separate CPU
render path.

---

## Deferred

- **Shore water effects** -- refraction pre-pass, shorewaves, caustics, coast
  blur; needs `coastmap`, `foam`, and `waverand` texture assets from BAR
- **Detail / DNTS textures** -- terrain surface breakup at high zoom; needs
  the asset from BAR's content tree and a `detailTex` bind group slot
- **Shadow map** -- directional shadow casting on terrain; requires a shadow
  depth pass and `groundShadowCoeff` plumbing in the SMF shader
- **SMF border shader** -- the engine's off-map edge fill (`SMFBorderProg`);
  cosmetic, deferred until user request
- **Non-BAR Spring/Recoil targets** -- architecture stays pluggable but no
  active work; BAR is the sole export target
- **Multiple export formats** -- only `.sd7 / spring-smf` today; other archive
  or target formats deferred
- **Batch headless export** -- `bar-cli` does single-project eval + export;
  multi-project batch scripting is not planned

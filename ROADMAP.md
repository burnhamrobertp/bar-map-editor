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
  metalmap, typemap paint layers previewed via synthesised albedo
- **Engine-faithful rendering** -- Recoil `ModernSkyVS/FS` port for sky and
  atmospheric fog; Recoil `SMFFragProg/VertProg` port for ground lighting
  (Blinn-Phong, underwater absorption); original PBR water shader (GGX
  specular, Schlick Fresnel, 4-octave scrolling normals, planar reflection
  pre-pass); brush cursor ring overlay in shader
- **GPU compute** -- wgpu compute shaders for noise and erosion; CPU fallback
  when no discrete GPU

---

## In progress

### Sculpt-to-export path
The 3D viewport and 2D inspector both update a live heightmap in-session.
That edited heightmap merges into the export via the sculpt overlay sidecar
PNG (a 16-bit grayscale delta file). The current bridge is manual: the user
clicks "Save heightmap as PNG" then wires a `FileInput` node. A proper Sculpt
node that writes the delta automatically into the graph -- so the export round-
trip is zero-click -- is not yet built.

### Typemap brush
The typemap paint target is wired in the UI (3D sculpt layout, brush-target
radio). The brush dab updates the inspector cache and synthesises a visualisation
colour for the viewport. The overlay merge at export uses the same path as
metalmap. Known gap: the typemap brush stroke is not yet recorded onto a
`PaintedMask` node in the graph, so edits are lost on re-eval unless manually
saved via the PNG bridge (same as heightmap).

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

### Sculpt node (closes the sculpt-to-export gap)
A `Sculpt` node type that owns the height delta directly inside the graph,
writes it to the overlay PNG on every save, and does not require a manual
`FileInput` hookup. This would make the "sculpt and export" workflow zero-
friction.

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

# Recoil source map (R1 finding, recorded as a doc)

This is the in-repo copy of the R1 finding from the v0.2 plan. It
identifies the upstream Recoil files that M2's renderer pivot will
port to WGSL.

## Upstream

- Repo: <https://github.com/beyond-all-reason/RecoilEngine>
- Pinned commit: see `vendor/recoil/UPSTREAM.md`
- Source path inside Recoil:
  `cont/base/springcontent/shaders/GLSL/`

## Files we port

| Concern | Files |
|---|---|
| Terrain (SMF renderer) | `SMFFragProg.glsl`, `SMFVertProg.glsl` |
| Terrain shading lookup | `SMFShadingTextureFragProg.glsl`, `SMFShadingTextureVertProg.glsl` |
| Water | `BumpWaterFS.glsl`, `BumpWaterVS.glsl` |
| Sky | `ModernSkyFS.glsl`, `ModernSkyVS.glsl` |
| Minimap (reference for the 2D inspector) | `MiniMapFragProg.glsl`, `MiniMapVertProg.glsl` |

The full vendored set is at `vendor/recoil/shaders/GLSL/`.

## What we deliberately don't port

| Concern | Why not |
|---|---|
| `BumpWaterCoastBlur*` | Coastline blur is a quality-of-life detail — defer until water is matching at all and we want polish. |
| `SMFBorderFragProg.glsl` / `SMFBorderVertProg.glsl` | Map-edge "world border" rendering — not visible in the the preview camera framing; defer. |
| `Grass*Prog.glsl` | bar-editor doesn't render grass yet. If we do, this is the obvious starting point. |
| `ProjFX*Prog.glsl`, `Icons*VS.glsl`, `Shapes*Prog.glsl` | Engine UI / projectile / icon rendering — not part of map preview. |
| `Shadow*Prog.glsl` | Real-time shadow casting in the preview is out of v0.2 scope. The terrain shader's lighting model approximates this when shadows are off. |
| `ModelFragProg*.glsl`, `ModelVertProg*.glsl` | Unit / feature model rendering — bar-editor previews terrain only. |

If any of these become relevant, add the file(s) to `vendor/recoil/sync.sh`'s
`SHADERS` array, run sync, commit, and add a row above.

## Porting notes (worked out; revisit during M2)

- Recoil GLSL uses `#version 130` plus engine-injected `#define`s for
  optional features (shadows, voxel feedback, etc.). The WGSL ports
  pick a single capability tier rather than carrying every `#ifdef`.
- Uniform buffer layout differs between OpenGL and wgpu — every
  uniform has to be repacked. The diff between the GLSL and our WGSL
  port is largest in the uniform-binding section, smallest in the
  fragment math.
- Recoil samples textures with old-style `texture2D` calls; WGSL
  uses `textureSample`. Mechanical translation.
- Lighting uses Lambert + Half-Lambert mix and a single sun direction
  uniform. Maps to a small uniform block in the WGSL port.

## What this finding doesn't yet cover

- The actual WGSL ports. Those land with M2 commits, file-by-file.
- The capability matrix (with / without shadows, with / without
  high-quality terrain detail). Decide at M2 design time.

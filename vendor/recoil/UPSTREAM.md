# Recoil engine — vendored shader sources

Open Machine ports a small set of Recoil's GLSL shaders to WGSL so the
3D preview matches the engine's actual map rendering. This directory
holds the **pristine upstream copies** of those shaders; the WGSL
ports live alongside our own shaders in `shaders/` at the repo root.

The shaders here are **never edited in place** — keeping them
byte-identical to upstream lets us diff-check against new releases
when we sync. All porting/adaptation happens in the WGSL ports.

## Source

- Upstream repo: <https://github.com/beyond-all-reason/RecoilEngine>
- Pinned commit: `681aea7cb8316c505ee3699db716b57abcd89066`
- Commit date: 2026-04-24
- Commit subject: "fix: memory barrier bitfield now pulls from
  correct parameter (#2949)"
- Source path inside Recoil:
  `cont/base/springcontent/shaders/GLSL/`

## What's here

| File | Purpose | Used by |
|---|---|---|
| `SMFFragProg.glsl` / `SMFVertProg.glsl` | Main SMF terrain shader (lighting, splatting) | M2 terrain ground shader port |
| `SMFShadingTextureFragProg.glsl` / `SMFShadingTextureVertProg.glsl` | Lookup texture pre-pass | M2 terrain shader port |
| `BumpWaterFS.glsl` / `BumpWaterVS.glsl` | Bump-water surface shader | M2 water shader port (replaces the abandoned procedural FBM water) |
| `ModernSkyFS.glsl` / `ModernSkyVS.glsl` | Atmospheric sky shader | M2 sky shader port (replaces the abandoned procedural gradient sky) |
| `MiniMapFragProg.glsl` / `MiniMapVertProg.glsl` | In-engine minimap renderer | Reference for the 2D inspector's heightmap visualization |

Files in this directory are GPL v2-or-later. See `LICENSE` and
`GPL-2.0.txt` / `GPL-3.0.txt`. License analysis (R2) covers how this
propagates into the OM workspace.

## Refresh procedure

When we want to pick up upstream fixes:

1. `cd ~/Projects/bar-recoil && git pull upstream master`
2. From the OM repo root: `bash vendor/recoil/sync.sh`
3. Update the pinned commit hash above.
4. `git diff vendor/recoil/shaders/GLSL/` — every changed line is a
   potential update for the WGSL port. Walk the diff, mirror the
   meaningful changes into our port.
5. Commit the vendor refresh and the port updates as **separate**
   commits so the upstream sync stays auditable.

We never refresh as a side-effect of other work.

## Why a sparse copy instead of a submodule

A full Recoil clone is ~1.5 GB (engine source, test fixtures, build
tooling). We need ~1 300 lines of GLSL. A submodule would cost every
contributor a multi-minute initial fetch for files we don't compile,
link, or run. The sparse copy is auditable (git diff between two
checkouts of this repo shows exactly which upstream changes we
adopted), version-pinned via the commit hash above, and trivially
refreshed.

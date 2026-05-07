# Recoil vendor strategy (R5)

## Goal

Bring Recoil source-of-truth shaders into the workspace so M2's
shader ports have a single, version-pinned reference that every
contributor sees identically — without paying the cost of a full
Recoil clone in the repo.

## Decision: sparse copy, not submodule

We copy ~10 GLSL files (~1 300 lines) and the upstream license files
into `vendor/recoil/`. We **don't** add a git submodule that points
at the full Recoil tree.

### Why not a submodule

Recoil's repo is ~1.5 GB. The files M2 needs are ~50 KB. A submodule
would:

- Force every fresh clone to fetch ~1.5 GB before the workspace builds
  (or break the build if `--init` is forgotten).
- Slow CI by minutes per run.
- Create a class of "submodule got out of sync with the parent
  pointer" issues that submodules are infamous for.
- Provide nothing in return — we don't compile, link, or run any
  Recoil source. We translate it.

### Why not a `Cargo.toml` git dep

Same problem. Recoil isn't a Rust crate; we'd have nothing to
register and nothing to build.

### Why not just read from the local clone at `~/Projects/bar-recoil`

The plan deliberately separates *source-of-truth* (in-repo, pinned)
from the contributor's *local working copy* of Recoil. Reading from
the user's clone would mean every contributor has a different
"reference," with no audit trail of which version their port targeted.
We learned this lesson with the abandoned procedural water shader —
referencing "the BAR website screenshots" was not a substitute for a
pinned reference.

## Layout

```
vendor/recoil/
    UPSTREAM.md          ← pinned commit hash, source paths, refresh procedure
    LICENSE              ← upstream license file (GPL v2-or-later)
    GPL-2.0.txt
    GPL-3.0.txt
    AUTHORS
    sync.sh              ← script that re-copies from a local clone
    shaders/
        GLSL/
            SMFFragProg.glsl
            SMFVertProg.glsl
            SMFShadingTextureFragProg.glsl
            SMFShadingTextureVertProg.glsl
            BumpWaterFS.glsl
            BumpWaterVS.glsl
            ModernSkyFS.glsl
            ModernSkyVS.glsl
            MiniMapFragProg.glsl
            MiniMapVertProg.glsl
```

## How M2 consumes the vendor copies

WGSL ports live in a separate `shaders/recoil/` directory at the repo
root (created when M2 lands). Each port references its source file by
path so readers can diff:

```wgsl
// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from vendor/recoil/shaders/GLSL/BumpWaterFS.glsl
// Upstream commit pinned in vendor/recoil/UPSTREAM.md
```

We never `include_str!()` the GLSL files into the Rust binary — they
exist for human reference only. The compiled binary contains only the
WGSL ports.

## Refresh discipline

- Upstream syncs are their own commits. Never bundled with port work.
- The upstream commit hash in `vendor/recoil/UPSTREAM.md` is the
  source of truth; if it disagrees with what's actually in
  `shaders/GLSL/`, the hash is wrong — fix it.
- Refreshes go in chronological order (no skipping forward to
  pre-release tags). We pick a stable commit on Recoil `master` each
  time.

## License consequences

See `docs/licensing.md`. Short version: vendored files and their
ports are GPL v3. The combined bar-editor binary is GPL v3. Other other crates
remain MIT/Apache, and downstream consumers who don't include the
GPL crates aren't bound by GPL.

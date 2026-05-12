# License analysis — porting from Recoil into BAR map editor

This document resolves R2 from the v0.2 plan. It documents (a) the
upstream license terms, (b) how those propagate when we port code into
BAR map editor, and (c) the operational rules we follow to stay
compliant.

## Upstream: Spring / Recoil

Spring (forked as Recoil for BAR) is licensed
**GPL v2 or any later version**. From the upstream `LICENSE`:

> Spring is free software: you can redistribute it and/or modify
> it under the terms of the GNU General Public License as published
> by the Free Software Foundation, either version 2 of the License,
> or (at your option) any later version.

The "or any later version" wording lets us choose to redistribute under
**GPL v3**, which is what we do for any vendored / ported code.

## What "porting" means here

When we translate a Recoil GLSL shader into WGSL we are creating a
**derivative work** of the original GLSL. The translation does not
reset the copyright; the WGSL still inherits the original's GPL
terms. The same rule applies to porting any algorithmic code (a C++
function rewritten in Rust): the Rust version is a derivative of the
C++ source.

In practice this means:

- A WGSL file that is a 1:1 translation of `BumpWaterFS.glsl` is
  GPL-licensed.
- A Rust function that ports the equation from a Recoil C++ math
  routine is GPL-licensed.
- A Rust function that we wrote from scratch using the upstream code
  *only as a behavioural reference* (read it, understood the math,
  re-derived our own implementation) is **not** a derivative — but
  the line is fuzzy and we don't push on it. When in doubt, treat
  ports as GPL.

## Project license

BAR map editor is licensed **GPL-2.0-or-later**, matching the upstream
Recoil license. This means:

- All workspace crates are GPL-2.0-or-later. There is no dual-license
  boundary to manage.
- The compiled `bar-app` binary is GPL-2.0-or-later.
- Anyone redistributing the binary must offer source for the whole
  combined work (which is fine -- we're already open source).

## Isolation discipline

Since the whole project is GPL, there is no license boundary to enforce
between ported and original code. The discipline is purely about
attribution and traceability:

1. **Vendored upstream sources live in `vendor/recoil/`** and are
   never edited in place. They carry the upstream license headers
   they came with (or none, when upstream supplies none -- the
   project-level `LICENSE` covers them by inclusion).
2. **WGSL ports of Recoil shaders live in `shaders/recoil/`** (to
   be created as M2 lands). Every port file starts with an SPDX
   header:

   ```glsl
   // SPDX-License-Identifier: GPL-2.0-or-later
   // Ported from Recoil's <FileName>.glsl. Upstream commit
   // <hash> at vendor/recoil/UPSTREAM.md.
   ```
3. **Rust ports of Recoil algorithms** (if any) live in a dedicated
   crate (`bar-recoil-port` -- to be created when needed) for
   discoverability. It carries the same GPL-2.0-or-later workspace
   license as every other crate.
4. **All crates** use the workspace `LICENSE` (GPL-2.0-or-later). No
   crate carries a separate MIT/Apache header.

## What we **don't** do

- We don't copy-paste GPL code into MIT/Apache files and try to
  relicense it. Source license follows the original work.
- We don't ship binary blobs of compiled Recoil shaders without
  source. (Compiled SPIR-V from a GPL GLSL source is itself GPL;
  shipping it without source would violate the license.)
- We don't strip authorship comments from upstream files when we
  vendor them.

## Attribution

Every release ships the contents of `vendor/recoil/LICENSE`,
`vendor/recoil/AUTHORS`, and a short note in the application About
dialog: "BAR map editor includes shaders ported from the Recoil engine
(github.com/beyond-all-reason/RecoilEngine), used under GPL v2 or later."
That note is added when M2 lands; it doesn't apply yet because no
ports exist in the current tree.


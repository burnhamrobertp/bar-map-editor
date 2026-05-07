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

BAR map editor is licensed **MIT/Apache-2.0** today. Both are
GPL-compatible: code under either license can be combined into a
GPL-licensed binary. This means **the application as a whole, when
linked with GPL-licensed Recoil ports, is distributed under GPL v3
terms** — even if the individual non-port crates retain their MIT/
Apache headers.

Practical consequences:

- The compiled `bar-app` binary is GPL v3.
- Anyone redistributing the binary must offer source for the whole
  combined work (which is fine — we're already open source).
- Individual non-port crates (`bar-graph`, `bar-data`, etc.) remain
  dual-licensed; downstream projects that consume *only* those
  crates as a library — without the Recoil ports — get them under
  the original dual-license terms.

## Isolation discipline

To keep the GPL boundary explicit we follow these rules:

1. **Vendored upstream sources live in `vendor/recoil/`** and are
   never edited in place. They carry the upstream license headers
   they came with (or none, when upstream supplies none — the
   project-level `LICENSE` covers them by inclusion).
2. **WGSL ports of Recoil shaders live in `shaders/recoil/`** (to
   be created as M2 lands). Every port file starts with an SPDX
   header:

   ```glsl
   // SPDX-License-Identifier: GPL-3.0-or-later
   // Ported from Recoil's <FileName>.glsl. Upstream commit
   // <hash> at vendor/recoil/UPSTREAM.md.
   ```
3. **Rust ports of Recoil algorithms** (if any) live in a dedicated
   crate (`bar-recoil-port` — to be created when needed) that itself
   carries a `LICENSE` of GPL-3.0-or-later. The crate does *not*
   carry the workspace's MIT/Apache dual-license headers.
4. **Other crates** keep their MIT/Apache headers. They depend on
   `bar-recoil-port` like any other crate; the GPL terms propagate at
   link time, not file-by-file.
5. **Public API**: nothing in the GPL crates is re-exported from the
   non-GPL crates. A consumer that wants the procedural-only path
   (no engine fidelity, no Recoil ports) can opt out at build time
   via a feature flag (deferred design — the v0.2 binary always
   includes the ports).

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
(github.com/beyond-all-reason/RecoilEngine), used under GPL v3."
That note is added when M2 lands; it doesn't apply yet because no
ports exist in the current tree.

## Open question (deferred)

If a downstream user wants to consume `bar-graph` / `bar-data` as a
library *without* GPL contamination, the cleanest path is a
build-time `--features no-recoil-ports` switch on `bar-app` that
removes the GPL crates from the dependency tree. We don't ship that
flag yet because there's no concrete demand. Listed here so it's not
forgotten.

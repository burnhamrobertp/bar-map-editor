# BAR map archive analysis

Companion to `bar-map-format.md`. Where the format doc describes the schema a current map archive should target, this doc captures what shipped maps actually contain -- the gap between intended format and lived reality, plus the cleanup opportunities that gap reveals (both in maps and in engine / game code).

Derived from a content audit of 121 shipped BAR map archives, cross-referenced against the Recoil engine source (notably `rts/Map/MapInfo.cpp`, `rts/Map/MapParser.cpp`, `rts/Map/SMF/SMFMapFile.cpp`, `rts/Map/SMF/SMFReadMap.cpp`, `rts/Game/GameSetup.cpp`) and BAR's game-side Lua (notably `luarules/gadgets/include/startbox_utilities.lua` and `luarules/gadgets/game_initial_spawn.lua`). Engine behaviour can drift between releases; re-verify against the active engine + game versions before depending on edge-case behaviour described here.

---

## Two-phase consumption model

A map archive is read by two distinct consumers:

1. **Engine phase (synchronous, C++).** The Recoil binary parses `mapinfo.lua`, loads `.smf` + `.smt` binaries, and resolves all texture references mapinfo declares. Everything in `mapinfo.lua`'s recognised tables is consumed here.
2. **Game-Lua phase (asynchronous, scripted).** Once the map is loaded, the engine boots `LuaRules/main.lua` and `LuaGaia/main.lua`. These gadgets load `mapconfig/*.lua`, read `mapoptions.lua` via `Spring.GetMapOptions()`, and place features / startboxes / weather effects.

Tooling that wants to validate or edit map content only needs to understand the engine phase; the game-Lua phase is opaque scripted behaviour that should be preserved verbatim on round-trip.

| File | Read by | Effect of absence |
|---|---|---|
| `mapinfo.lua` | engine | Fatal load error, unless `maphelper/mapinfo.lua` exists |
| `maphelper/mapinfo.lua` | engine | Engine probes this BEFORE root `mapinfo.lua`; first-hit wins. Almost universally a one-line `return VFS.Include("mapinfo.lua")` shim that delegates to root. |
| `maps/<name>.smf` | engine | Fatal -- heightmap binary missing |
| `maps/<name>.smt` | engine | Fatal if referenced by `smf.smtFileName0` |
| `maps/minimap.bmp` / `.dds` | engine | Falls back to the SMF-embedded minimap |
| `mapoptions.lua` | game | No author-defined map options |
| `mapconfig/*.lua` | game | All optional; gadget-specific |
| `LuaGaia/*.lua` | game | Neutral-player gadgets disabled |
| `LuaRules/*.lua` | game | Map-specific game rules disabled |
| `libs/<name>/*.lua` | game | Only needed by gadgets that `VFS.Include` from them |
| `bitmaps/*` | engine | Engine fallback path -- `bitmaps/foam.jpg`, `bitmaps/ocean.jpg`, `bitmaps/caustics/caust00.tga` etc. override engine defaults globally |
| `features/*.lua`, `objects3d/*.s3o`, `unittextures/*.dds` | engine + game | Map-provided feature defs / models |

---

## Template-driven copy-paste

Shipped maps are overwhelmingly template-copied from one another. The clearest evidence is the prevalence of fields the modern engine doesn't read:

| Key | Prevalence | Status |
|---|---|---|
| `mapfile` (top-level) | ~88% | Not read by modern engine; SMF path resolves from the archive name automatically |
| `grass.grassDistTGA` | ~42% | Never parsed |
| `grass.grassBladeColorTex` | ~30% | Never parsed |
| `atmosphere.skyDir` | varies | Deprecated; engine logs `L_DEPRECATED` and ignores. Replacement: `atmosphere.skyAxisAngle` |
| `grass.mapGrassColorModTex`, `grass.grassWindPerturbTex` | <5% each | Not read by active engine paths |
| `rassBladeColorTex` (typo'd, missing leading `g`) | occasional | Engine silently ignores unknown keys; the typo propagates because the file is template-copied |

The typo case is the diagnostic. Maps that ship `rassBladeColorTex` are clearly forking from another map that introduced the typo -- no author would type that key fresh. The same template-fork pattern explains why dead-but-harmless keys persist across decades of maps.

---

## File-archive composition

File-category breakdown across the 121-archive audit:

- **engine-magic paths** (e.g. `maps/*.smf`, `LuaGaia/*`, `mapconfig/*.lua`, `bitmaps/*`): ~74.7%
- **`mapinfo`-referenced filenames** (engine finds via VFS, wherever placed in the archive): ~9.6%
- **unclassified** (neither magic nor mapinfo-referenced): ~15.6%

The unclassified bucket includes legitimate content (gadgets that `VFS.Include` files from non-standard paths -- not classifiable without running each map's Lua) and outright cruft (covered below).

---

## Vestigial paths and author-side cruft

Files at conventional paths that the active engine doesn't read, or files that should never have been bundled:

| Path | Note |
|---|---|
| `metalmap.bmp` at archive root | Engine reads the metalmap layer from the SMF binary. Loose file unused. |
| `typemap.bmp` at archive root | Same -- engine reads from SMF. |
| `maps/source/*.tmd` | Spring Terrain Modeller author-side build artefacts. |
| `maps/source/*.py` | Author-side helper scripts. |
| Duplicate texture variants (e.g. `Foo_2k_dnts.tga` alongside the `_1k_dnts.tga` that mapinfo actually references) | Unused; left over from iteration. |
| `mapinfo` keys with typos like `rassBladeColorTex` | Silently ignored. |

A cleanup pass over the BAR map catalog could safely drop all of the above without affecting load behaviour.

---

## LuaGaia / LuaRules verbatim templating

The `LuaGaia/` directory is near-universally copy-pasted from a single source template. The standard scaffold:

- `LuaGaia/main.lua` -- synced gadget host
- `LuaGaia/draw.lua` -- unsynced draw host
- `LuaGaia/Gadgets/FP_featureplacer.lua` -- reads `mapconfig/featureplacer/*` and spawns features
- `LuaGaia/effects/drop.png`, `LuaGaia/effects/snowflake.png` -- weather particle assets

`LuaRules/` is far less common -- most maps don't customise gameplay rules and ship no `LuaRules/` content.

Tooling that round-trips archives should treat both directories as opaque template content unless the user is explicitly editing gameplay logic. The verbatim-template pattern means a future "single canonical template" maintained outside individual map archives would eliminate hundreds of duplicate file copies across the catalog with no behavioural change.

---

## Map-provided vs mod-provided features

Most maps reference only features the game's mod archive provides (trees, rocks, generic crystals). The map archive carries no feature-definition or model files.

A minority ship custom features in `features/*.lua` + `objects3d/*.s3o` + `unittextures/*.dds` (the audit found this on a small fraction of archives). Unlike mapinfo dead-keys, the cargo-cult problem is much less common here -- when a map ships features, it's because the author actually used them.

---

## Format overlap suggesting consolidation

Some `mapinfo` fields are alternatives at different generations of the engine's renderer. The format spec hasn't been pruned even when one path has effectively supplanted the other:

- `resources.detailTex` (older non-splat detail) vs `resources.splatDistrTex` + `splatDetailNormalTex1..4` (modern splat-detail-normal path). Both still work; modern maps almost universally use the splat path.
- `resources.splatDetailTex` (older greyscale splat) vs `resources.splatDetailNormalTex1..4` (current detail-normal splat). Largely supplanted.
- `lighting.groundSpecularColor` (global) vs `resources.specularTex` (per-pixel). When the texture is present it takes precedence per fragment; the global value is the fallback.
- `atmosphere.skyDir` (deprecated, ignored with a log) vs `atmosphere.skyAxisAngle`.
- `voidWater` / `voidGround` -- read by gameplay gadgets, not by the engine renderer. The naming suggests an engine-renderer effect; it isn't one.

A format-cleanup pass could deprecate the legacy alternatives in a future spec version and have tooling refuse to emit them, while keeping the parser tolerant for backwards compatibility.

---

## Engine / game code that could potentially be retired

Fields widely set in shipped maps but never parsed by the active engine binary suggest C++ paths whose vestiges are pure noise. Candidates for a code-side audit (verify against current consumers in `rts/Map/`):

- `grass.grassBladeColorTex`, `grass.grassDistTGA`, `grass.mapGrassColorModTex`, `grass.grassWindPerturbTex` -- if no current engine path reads these, the parser block and whatever GLSL texture binding they used to feed should be removed together.
- `mapfile` -- 88% of shipped maps set it, but the engine resolves the SMF path from the archive name. Either the parser dropped the key but the parser block still validates it as "not unknown", or it's silently consumed and discarded. Either way the field has no behaviour. Dropping the parser entry is backwards-compatible with all maps that still set it.
- `atmosphere.skyDir` -- the `L_DEPRECATED` log path could be retired once authoring tooling has stopped emitting it and enough template churn has cycled through to make the deprecation message redundant.

Confirming each requires grepping `rts/Map/` for current consumers and verifying nothing depends on the legacy key.

---

## Methodology notes

Audit pool: 121 `.sd7` archives shipped via BAR's current map catalog. Each archive was opened (without disk extraction) and its file list classified into:

- **magic** -- file at a known engine-magic path (e.g. `maps/*.smf`, `LuaGaia/*`, `mapconfig/*.lua`, `bitmaps/*`). Engine or game-Lua reads these by convention.
- **mapinfo-ref** -- filename referenced by a string value in `mapinfo.lua`. Position-independent: the engine finds it via VFS lookup wherever it happens to live in the archive.
- **unclassified** -- neither category. Includes author-side cruft and intentional-but-non-standard content.

`mapinfo.lua` parsing was a regex-based key extraction (no Lua evaluator), so dynamically-keyed tables are not represented in prevalence stats. The audit pool covers what's shipped today; older maps no longer in the active catalog may exhibit additional vestigial keys.

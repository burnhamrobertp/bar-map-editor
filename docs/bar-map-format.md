# BAR map format reference

What the engine and the game actually require from a map archive, vs what's
optional with a default. Source of truth for our `mapinfo.lua` emitter,
validators, and the eventual mapconfig writers.

Derived from reading:

- `~/Projects/bar-recoil/rts/Map/MapInfo.cpp` (the engine's mapinfo reader)
- `~/Projects/bar-recoil/rts/Map/MapParser.cpp` (top-level archive parser)
- `~/Projects/bar-recoil/rts/Map/SMF/SMFMapFile.cpp` and `SMFReadMap.cpp`
- `~/Projects/bar-recoil/rts/Game/GameSetup.cpp` (start-position handling)
- `~/Projects/bar-game/luarules/gadgets/include/startbox_utilities.lua`
- `~/Projects/bar-game/luarules/gadgets/game_initial_spawn.lua`

Pinned commits live at the top of each clone's `git log`. Re-verify before
making decisions if the BAR/Recoil source moves.

---

## Bare-minimum viable map

Three files, two mapinfo fields:

- `mapinfo.lua` (must parse without syntax errors)
- `maps/<name>.smf` (heightmap binary, valid SMF header)
- `maps/<name>.smt` (tile binary referenced by `smf.smtFileName0`)

Minimum mapinfo content:

```lua
return {
    name = "TestMap",

    smf = {
        smtFileName0 = "testmap.smt",
    },

    teams = {
        [0] = { startPos = { x = 512, z = 512 } },
        [1] = { startPos = { x = 1536, z = 1536 } },
    },
}
```

Everything below is optional — the engine has a hardcoded default for every
field. `teams[i].startPos` is *conditionally* required: only when
`Game.startPosType == 0` (SPAWN_FIXED). For RANDOM / CHOOSE_IN_GAME the
startPos values are ignored.

---

## Archive files

| File                         | Required by | Effect of absence                                                                  |
| ---------------------------- | ----------- | ---------------------------------------------------------------------------------- |
| `mapinfo.lua`                | engine      | Fatal load error — `MapInfo.cpp:52`                                                |
| `maps/<name>.smf`            | engine      | Fatal — heightmap binary missing                                                   |
| `maps/<name>.smt`            | engine      | Fatal — referenced via `smf.smtFileName0`, must be present                         |
| `mapconfig/map_startboxes.lua` | game (optional) | Falls back to autohost / random box. See [Startboxes](#startboxes).               |
| `LuaGaia/*`, `LuaUI/*`         | game (optional) | Map-supplied widgets/gadgets disabled.                                             |
| `metalmap.bmp`, `typemap.bmp`  | n/a         | Engine reads metalmap/typemap from the SMF binary directly. Loose files unused.   |

---

## `mapinfo.lua` — top level

| Field             | Required | Default    | Effect of absence                                                            |
| ----------------- | -------- | ---------- | ---------------------------------------------------------------------------- |
| `name`            | game     | engine name | Lobby shows generic name                                                    |
| `shortname`       | cosmetic | unset      | Falls back to `name`                                                         |
| `description`     | cosmetic | unset      | Falls back to `name`                                                         |
| `author`          | cosmetic | `""`       | No author shown                                                              |
| `version`         | cosmetic | unset      | Not read by engine                                                           |
| `mapfile`         | optional | auto-detect | Slower archive load (warning logged); auto-resolves                         |
| `modtype`         | optional | unset      | Spring uses default; BAR maps conventionally set `3`                         |
| `maphardness`     | optional | `100.0`    | Uniform deform resistance at default                                         |
| `notDeformable`   | optional | `false`    | Map is deformable                                                            |
| `gravity`         | optional | `130.0`    | Spring default gravity                                                       |
| `tidalStrength`   | optional | `0.0`      | No tide                                                                      |
| `maxMetal`        | optional | `0.02`     | Default 2% metal density                                                     |
| `extractorRadius` | optional | `500.0`    | Default extractor radius                                                     |
| `voidWater`       | optional | `false`    | Water renders normally                                                       |
| `voidGround`      | optional | `false`    | Ground renders at edges                                                      |

---

## `mapinfo.lua → smf`

| Field           | Required | Default                                       | Effect of absence                                            |
| --------------- | -------- | --------------------------------------------- | ------------------------------------------------------------ |
| `smtFileName0`  | engine   | n/a                                           | Tile rendering fails; required if SMF references tile file 0 |
| `smtFileNameN`  | optional | n/a                                           | Only required if SMF was built with > 1 tile file            |
| `minheight`     | optional | reads from SMF header                         | Falls back to header values; mapinfo override takes priority |
| `maxheight`     | optional | reads from SMF header                         | Same                                                         |
| `minimapTex`    | optional | embedded minimap from SMF                     | Engine uses 1024×1024 DXT1 minimap baked into SMF            |
| `metalmapTex`   | optional | embedded metalmap from SMF                    | Path override only                                           |
| `typemapTex`    | optional | embedded typemap from SMF                     | Path override only                                           |
| `grassmapTex`   | optional | unset                                         | No grass rendering on this map                               |

---

## `mapinfo.lua → atmosphere`

Every field optional. Defaults from `MapInfo.cpp:134-165`.

| Field              | Default                  |
| ------------------ | ------------------------ |
| `minWind`          | `5.0`                    |
| `maxWind`          | `25.0`                   |
| `fogStart`         | `0.1`                    |
| `fogEnd`           | `1.0`                    |
| `fogColor`         | `{0.7, 0.7, 0.8}`        |
| `skyBox`           | `""` (procedural sky)    |
| `skyColor`         | `{0.1, 0.15, 0.7}`       |
| `skyAxisAngle`     | `{0, 1, 2, 0}`           |
| `sunColor`         | `{1, 1, 1}`              |
| `cloudColor`       | `{1, 1, 1}`              |
| `fluidDensity`     | `0.3`                    |
| `cloudDensity`     | `0.5`                    |

---

## `mapinfo.lua → lighting`

Every field optional. Defaults from `MapInfo.cpp:207-221`.

| Field                  | Default                    |
| ---------------------- | -------------------------- |
| `sunDir`               | `{0, 1, 2, 1}` (normalised) |
| `groundAmbientColor`   | `{0.5, 0.5, 0.5}`          |
| `groundDiffuseColor`   | `{0.5, 0.5, 0.5}`          |
| `groundSpecularColor`  | `{0.1, 0.1, 0.1}`          |
| `groundShadowDensity`  | `0.8`                      |
| `unitAmbientColor`     | `{0.4, 0.4, 0.4}`          |
| `unitDiffuseColor`     | `{0.7, 0.7, 0.7}`          |
| `unitSpecularColor`    | falls back to `unitDiffuseColor` |
| `unitShadowDensity`    | `0.8`                      |
| `specularExponent`     | `100.0`                    |

---

## `mapinfo.lua → water`

All optional; large defaults block in `MapInfo.cpp:236-334`. Highlights:

| Field           | Default                | Notes                                         |
| --------------- | ---------------------- | --------------------------------------------- |
| `damage`        | `0.0`                  | No water damage                               |
| `absorb`        | `{0, 0, 0}`            | Black absorption                              |
| `baseColor`     | `{0, 0, 0}`            | Black base                                    |
| `minColor`      | `{0, 0, 0}`            | Black minimum                                 |
| `surfaceColor`  | `{0.75, 0.8, 0.85}`    | Light blue surface                            |
| `surfaceAlpha`  | `0.55`                 |                                               |
| `planeColor`    | absent                 | Presence enables a flat water plane           |
| `texture`       | `"ocean.jpg"` from `gamedata/resources.lua` | Engine fallback chain |
| `foamTexture`   | `"foam.jpg"` from same                  | Engine fallback chain |
| `normalTexture` | `"waterbump_4tiles.dds"` from same     | Engine fallback chain |
| `caustics`      | 32 default textures from `bitmaps/caustics/` |                  |

A long tail (`fresnel*`, `perlin*`, `wave*`, `causticsResolution`, etc.) all
have defaults — see source. None are required.

---

## `mapinfo.lua → splats` (BAR-specific)

| Field        | Default                  |
| ------------ | ------------------------ |
| `texScales`  | `{0.02, 0.02, 0.02, 0.02}` |
| `texMults`   | `{1, 1, 1, 1}`           |

---

## `mapinfo.lua → grass` (BAR-specific)

All optional. Defaults from `MapInfo.cpp:190-197`.

| Field               | Default     |
| ------------------- | ----------- |
| `bladeWaveScale`    | `1.0`       |
| `bladeWidth`        | `0.7`       |
| `bladeHeight`       | `4.5`       |
| `bladeAngle`        | `1.0`       |
| `maxStrawsPerTurf`  | `150`       |
| `bladeColor`        | `{0.1, 0.4, 0.1}` |

---

## `mapinfo.lua → resources`

All optional. Engine has fallback chain to `gamedata/resources.lua` for the
common textures.

| Field                       | Default fallback                          |
| --------------------------- | ----------------------------------------- |
| `detailTex`                 | `"detailtex2.bmp"` from `gamedata/resources.lua` |
| `specularTex`               | unset                                     |
| `splatDetailTex`            | unset (splat detail layer disabled)       |
| `splatDistrTex`             | unset (requires `splatDetailTex`)         |
| `grassShadingTex`           | unset                                     |
| `skyReflectModTex`          | unset                                     |
| `detailNormalTex`           | unset                                     |
| `lightEmissionTex`          | unset                                     |
| `parallaxHeightTex`         | unset                                     |
| `splatDetailNormalTex[N]`   | unset (per-channel sub-table)             |

---

## `mapinfo.lua → teams`

| Field                                  | Required                                  | Default                                  |
| -------------------------------------- | ----------------------------------------- | ---------------------------------------- |
| `teams[i]`                             | conditional on `Game.startPosType`        | warning logged, falls back if SPAWN_FIXED |
| `teams[i].startPos.x`, `.z`            | conditional (SPAWN_FIXED only)            | n/a                                      |
| `teams[i].startPos.y`                  | never read                                | engine recomputes from heightmap         |

`startPosType` modes (`Game.startPosType`):

- `0` SPAWN_FIXED — engine uses `teams[i].startPos`
- `1` SPAWN_CHOOSE_BEFORE_GAME — players pick before game; mapinfo positions ignored
- `2` SPAWN_CHOOSE_IN_GAME — players pick in game; mapinfo positions ignored

For our editor's purposes: always emit `teams` with all spawns for safety.

---

## `mapinfo.lua → terrainTypes`

Every field has a default. Optional.

| Field                          | Default      |
| ------------------------------ | ------------ |
| `name`                         | `"Default"`  |
| `hardness`                     | `1.0`        |
| `receiveTracks`                | `true`       |
| `moveSpeeds.tank/kbot/hover/ship` | `1.0` each |

---

## Startboxes

Format defined entirely by `bar-game`'s
`luarules/gadgets/include/startbox_utilities.lua` (line 122 onward). The
**Recoil engine never reads startboxes** — they are pure game-layer state.
The engine's only related API is `Spring.SetAllyTeamStartBox(allyTeamID,
xmin, zmin, xmax, zmax)` for AABB clamping, and game gadgets compute that
themselves from the polygon.

**Archive path:** `mapconfig/map_startboxes.lua` (root of SD7).

**Schema** the parser expects:

```lua
return {
    [allyTeamID] = {
        nameLong = "North-West",   -- string, optional, parser auto-fills if missing
        nameShort = "NW",          -- string, optional
        startpoints = { {x, z}, ... },  -- elmo coords, optional, never gameplay-read
        boxes = {                       -- array of polygons; required for polygon configs
            {
                {x, z},                 -- {x, z} OR {x, z, spline_strength}
                {x, z},                 -- spline_strength ∈ [0,1], default 0
                ...
            },
            -- additional disjoint polygons allowed per ally team
        },
    },
    -- additional ally teams …
}
```

- Coordinates are **elmos** (world units), not normalised.
- Polygons are arrays of vertices — closed implicitly, conventionally clockwise.
- Spline strength is **rendering-only** (Catmull-Rom tessellation in
  `startbox_utilities.lua:193-208`). Containment checks happen on the
  tessellated polygon. Plain rectangles emerge vertex-identical, no cost.
- Ally-team keys are 0-based; the parser normalises 1-based keys via
  `NormalizeConfigKeys()`.
- Per-gametype branching: the parser includes the file via
  `WrappedInclude()` which injects a `gametype` shim. Maps may
  conditionally `return` different tables based on
  `Spring.Utilities.Gametype.IsFFA()` etc. — this is map-author code, the
  parser just receives whatever the file ultimately returns.
- Multiple disjoint polygons per ally team are supported (`boxes` is an
  array, not a single polygon).

**Strategic direction (maps-metadata#605):** the new bar-lobby loads
startboxes from Rowy via maps-metadata, not from map archives. The
maps-metadata schema is moving from `poly.maxItems: 2` (axis-aligned
rectangles) to N-point polygons, with bounding-box fallbacks for
rect-only consumers (SPADS, Tachyon, engine `StartRectTop/Left/Bottom/Right`).
Our editor's polygon authoring should follow Rowy's UX (drag vertices,
edge-midpoint insertion, drag-to-move) so the same author muscle-memory
works in both places.

**Consumer APIs (game-side, `GG.*`):**

- `GG.IsInsideStartbox(x, z, allyTeamID)` — `nil` if non-polygon config, else `bool`
- `GG.GetStartboxBounds(allyTeamID)` — AABB
- `GG.GetStartboxPolygons(allyTeamID)` — `entry.boxes` array
- `GG.startBoxConfig` — raw table
- `GG.startBoxConfigSource` — `"mapside" | "autohost_polygon" | "autohost_rect" | "fallback"`

---

## Editor implications

1. Emit only fields the user has explicitly customised. Engine defaults are
   stable and well-tuned; emitting them every time creates churn and
   obscures real authorial choices.
2. The bare-minimum-map serves as the floor for our own validation —
   everything below should always be present:
   - `name`, `smf.smtFileName0`, valid SMF + SMT, `teams[]` with `startPos`.
3. Our project format is the single source of truth; we round-trip
   polygons natively and compute the AABB at export time for any rect-only
   downstream we end up needing.
4. `description`/`author`/`version` are cosmetic — never block export on
   them.
5. `teams[i].startPos.y` is engine-computed — never write it.

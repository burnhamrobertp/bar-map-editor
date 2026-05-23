# BAR map format reference

The current archive format for Beyond All Reason maps -- the structure to target when authoring or generating a map.

For analysis of what shipped maps actually contain (template cargo-cult, dead fields, vestigial files, cleanup opportunities), see `bar-map-analysis.md`.

---

## What a map archive is

A BAR map is distributed as an `.sd7` archive (7-zip). Inside:

- An SMF / SMT binary pair holding the heightmap, embedded minimap, embedded metalmap, embedded typemap, and tile pool.
- `mapinfo.lua` declaring engine-consumed properties: render parameters, lighting, water, splats, grass, texture references, team start positions, terrain types.
- Optional game-Lua content under `LuaGaia/`, `LuaRules/`, `mapconfig/`, `features/` that runs once the map is loaded.
- Optional asset overrides (water bitmaps, feature S3O models + textures, minimap).

---

## Bare-minimum viable map

Three files, two mapinfo fields:

- `mapinfo.lua` (must parse without syntax errors)
- `maps/<name>.smf`
- `maps/<name>.smt`

Minimum mapinfo:

```lua
return {
    name = "TestMap",
    smf = { smtFileName0 = "testmap.smt" },
    teams = {
        [0] = { startPos = { x = 512, z = 512 } },
        [1] = { startPos = { x = 1536, z = 1536 } },
    },
}
```

`teams[i].startPos` is required when `Game.startPosType == 0` (SPAWN_FIXED). Everything else has a default.

---

## File layout

| Path | Role |
|---|---|
| `mapinfo.lua` | Engine-parsed map config (required) |
| `maps/<name>.smf` | Heightmap + embedded minimap / metalmap / typemap (required) |
| `maps/<name>.smt` | Texture tile pool referenced by SMF (required) |
| `maps/minimap.bmp` or `.dds` | One of several possible minimap-override paths -- exact resolution rules unverified, see TODO |
| `mapconfig/*.lua` | Auxiliary scripts loaded by gadgets (feature placement, startboxes, etc.) |
| `LuaGaia/*` | Map's neutral-player Lua state |
| `LuaRules/*` | Map-specific gameplay rules |
| `libs/<name>/*.lua` | Lua libraries gadgets include from |
| `features/*.lua` | Map-provided feature definitions |
| `objects3d/*.s3o` | Feature model binaries |
| `unittextures/*.dds` | Feature textures |
| `bitmaps/*` | Engine-default override path (water foam, ocean, caustics, etc.) |

---

## `mapinfo.lua` -- top level

| Field | Default | Notes |
|---|---|---|
| `name` | engine name | Shown by lobby |
| `shortname` | unset | Falls back to `name` |
| `description` | unset | Falls back to `name` |
| `author` | `""` | Display only |
| `version` | unset | Display only |
| `modtype` | unset | BAR convention: `3` |
| `maphardness` | `100.0` | Uniform deform resistance |
| `notDeformable` | `false` | Disable terrain deformation |
| `gravity` | `130.0` | |
| `tidalStrength` | `0.0` | |
| `maxMetal` | `0.02` | Peak metalmap density |
| `extractorRadius` | `500.0` | Elmo |
| `voidWater` | `false` | Gadget-checked |
| `voidGround` | `false` | Gadget-checked |
| `autoShowMetal` | `false` | Toggle metalmap overlay at start |

---

## `mapinfo.lua → smf`

| Field | Default | Notes |
|---|---|---|
| `smtFileName0` | engine resolves from SMF | Reference to `.smt` tile binary |
| `smtFileNameN` | n/a | Only if multiple tile pools |
| `minheight` | reads from SMF header | Mapinfo override wins |
| `maxheight` | reads from SMF header | |
| `minimapTex` | embedded in SMF | External override |
| `metalmapTex` | embedded in SMF | External override |
| `typemapTex` | embedded in SMF | External override |
| `grassShadingTex` | minimap texture | Controls the texture drawn beyond playable map borders when the player picks the "textured" map-border style. Default (the minimap) continues the landscape seamlessly; override to customise the off-map image. (Engine field was originally for grass-blade tinting but BAR doesn't render engine grass, so this is its only practical effect.) |

---

## `mapinfo.lua → atmosphere`

| Field | Default |
|---|---|
| `minWind` / `maxWind` | `5.0` / `25.0` |
| `fogStart` / `fogEnd` | `0.1` / `1.0` |
| `fogColor` | `{0.7, 0.7, 0.8}` |
| `skyBox` | `""` (procedural sky) |
| `skyColor` | `{0.1, 0.15, 0.7}` |
| `skyAxisAngle` | `{0, 1, 2, 0}` (xyz + angle) |
| `sunColor` | `{1, 1, 1}` |
| `cloudColor` | `{1, 1, 1}` |
| `fluidDensity` | `0.3` |
| `cloudDensity` | `0.5` |

---

## `mapinfo.lua → lighting`

| Field | Default |
|---|---|
| `sunDir` | `{0, 1, 2, 1}` (normalised) |
| `groundAmbientColor` | `{0.5, 0.5, 0.5}` |
| `groundDiffuseColor` | `{0.5, 0.5, 0.5}` |
| `groundSpecularColor` | `{0.1, 0.1, 0.1}` |
| `groundShadowDensity` | `0.8` |
| `unitAmbientColor` | `{0.4, 0.4, 0.4}` |
| `unitDiffuseColor` | `{0.7, 0.7, 0.7}` |
| `unitSpecularColor` | falls back to `unitDiffuseColor` |
| `unitShadowDensity` | `0.8` |
| `specularExponent` | `100.0` |

---

## `mapinfo.lua → water`

| Field | Default | Notes |
|---|---|---|
| `damage` | `0.0` | Per-second HP damage to units in the water plane. `> 0` makes the water surface behave as lava. |
| `absorb` | `{0, 0, 0}` | Per-channel absorption coefficient |
| `baseColor` | `{0, 0, 0}` | Deep-water tint |
| `minColor` | `{0, 0, 0}` | Floor of underwater colour |
| `surfaceColor` | `{0.75, 0.8, 0.85}` | |
| `surfaceAlpha` | `0.55` | |
| `planeColor` | absent | Presence enables a flat water plane |
| `texture` | engine `"ocean.jpg"` | Water diffuse |
| `foamTexture` | engine `"foam.jpg"` | Shore foam |
| `normalTexture` | engine `"waterbump_4tiles.dds"` | Bump-water normal |

A long tail (`fresnelMin/Max/Power`, `perlinStart`, `perlinAmplitude`, `perlinLacunarity`, `shoreWaves`, `waveLength`, `waveFoamIntensity`, `waveOffsetFactor`, `causticsResolution`, `causticsStrength`, `reflectionDistortion`, `repeatX/Y`, `blurBase/Exponent`, `ambientFactor`, `diffuseFactor`, `specularFactor`, `specularPower`) all have defaults.

---

## `mapinfo.lua → splats`

The detail-normal splatting path: a 4-channel distribution texture selects which of four detail-normal textures applies per pixel.

| Field | Default |
|---|---|
| `texScales` | `{0.02, 0.02, 0.02, 0.02}` |
| `texMults` | `{1, 1, 1, 1}` |

The 4 detail-normal textures themselves are referenced from `resources`.

---

## `mapinfo.lua → resources`

Texture filenames; engine resolves against the archive's VFS.

| Field | Effect |
|---|---|
| `detailTex` | Tiled detail (older non-splat path) |
| `specularTex` | Per-pixel specular colour (RGB) + exponent (alpha * 16) |
| `splatDistrTex` | 4-channel splat distribution; enables splat-detail-normal path |
| `splatDetailNormalTex1`..`4` | Four detail-normal textures sampled per channel |
| `splatDetailNormalDiffuseAlpha` | Bool: alpha channel contributes additional detail colour |
| `splatDetailTex` | Older greyscale splat path |
| `skyReflectModTex` | Per-pixel sky-reflection mask |
| `detailNormalTex` | Single map-wide normal texture |
| `lightEmissionTex` | Per-pixel emissive contribution |
| `parallaxHeightTex` | Per-pixel parallax depth |

Choosing a path:
- For per-pixel ground detail prefer `splatDistrTex` + `splatDetailNormalTex1..4`; the engine falls back to `detailTex` when no splats are authored.
- For per-fragment specular use `specularTex`; without it the global `lighting.groundSpecularColor` applies uniformly across the terrain.

---

## `mapinfo.lua → teams`

| Field | Notes |
|---|---|
| `teams[i]` | Required when `Game.startPosType == 0` (SPAWN_FIXED) |
| `teams[i].startPos.x`, `.z` | Elmo coords |
| `teams[i].startPos.y` | Never written; engine recomputes from heightmap |

`Game.startPosType`:
- `0` SPAWN_FIXED -- `teams[i].startPos` used
- `1` SPAWN_CHOOSE_BEFORE_GAME -- positions ignored
- `2` SPAWN_CHOOSE_IN_GAME -- positions ignored

Always emit `teams` with all spawns so the map remains valid when a lobby falls back to SPAWN_FIXED.

---

## `mapinfo.lua → terrainTypes`

Movement modifiers per terrain-type ID (the typemap encodes a u8 per pixel referencing one of these entries).

| Field | Default |
|---|---|
| `name` | `"Default"` |
| `hardness` | `1.0` (multiplier applied to top-level `maphardness`, not a replacement) |
| `receiveTracks` | `true` |
| `moveSpeeds.tank` / `.kbot` / `.hover` / `.ship` | `1.0` each |

---

## `mapinfo.lua → custom.fog`

Height-based fog tint applied as a post-pass by a BAR widget (`gui_custom_fog.lua`). Not consumed by the engine binary.

```lua
custom = {
    fog = {
        color    = { 0.6, 0.7, 1.0 },
        height   = 200,    -- elmos
        fogatten = 0.005,  -- per-elmo attenuation below height
    },
}
```

---

## `mapinfo.lua → custom.grassDistTGA`

Path (relative to the map archive) of the grass distribution texture BAR's custom grass renderer samples to decide where blades grow. This is the replacement for the engine's `smf.grassmapTex` field, which BAR's grass system ignores. Set this if the map has grass; leave unset for grassless maps.

```lua
custom = {
    grassDistTGA = "maps/mymap_grass.tga",
}
```

---

## `mapinfo.lua → sound`

Per-channel reverb / filter params. Rarely customised.

---

## `mapconfig/`

Auxiliary game-Lua scripts. All optional; specific gadgets `VFS.Include` the files they need.

| Path | What it does |
|---|---|
| `mapconfig/featureplacer/config.lua` | Feature-placer params |
| `mapconfig/featureplacer/set.lua` | Feature placement data (positions, types) |
| `mapconfig/featureplacer/featureplacement_set.lua` | Alternate placement-set file |
| `mapconfig/map_startboxes.lua` | Startbox polygons (see Startboxes) |
| `mapconfig/map_metal_layout.lua` | Custom metal-spot layout |

---

## `LuaGaia/` and `LuaRules/`

Map's neutral-player and game-rules Lua states. The engine boots `LuaGaia/main.lua` and `LuaRules/main.lua` automatically if present.

Conventional layout:

| Path | Purpose |
|---|---|
| `LuaGaia/main.lua` | Synced gadget host |
| `LuaGaia/draw.lua` | Unsynced draw host |
| `LuaGaia/Gadgets/FP_featureplacer.lua` | Reads `mapconfig/featureplacer/*` and spawns features |
| `LuaGaia/effects/*` | Particle assets (rain, snow, etc.) |
| `LuaRules/main.lua` + `LuaRules/Gadgets/*` | Map-specific gameplay rules |
| `libs/<name>/*.lua` | Lua libraries (e.g. `libs/lcs/`, `libs/s11n/`) gadgets include from |

---

## Features

Two systems coexist:

**Mod-provided.** The game archive ships standard feature definitions in `gamedata/defs.lua`; maps reference these features by name in placement data. No files in the map archive. This is the common case (trees, rocks, generic crystals, etc.).

**Map-provided.** When the map needs features the game doesn't ship, the archive contains:

- `features/*.lua` -- one Lua file per feature definition (returns a Lua table with the feature's properties and S3O model reference)
- `objects3d/<name>.s3o` -- model binary
- `unittextures/<name>.dds` -- textures referenced by the S3O

Placement runs at map start via `LuaGaia/Gadgets/FP_featureplacer.lua` calling `Spring.CreateFeature()`. Coordinates are elmos; rotation in Spring heading units (`-32768..32767`, half-circles per 32768 units).

---

## Startboxes

Format defined by `bar-game`'s `luarules/gadgets/include/startbox_utilities.lua`. The engine never reads startboxes -- they are pure game-layer state.

**Path:** `mapconfig/map_startboxes.lua`

**Schema:**

```lua
return {
    [allyTeamID] = {
        nameLong = "North-West",
        nameShort = "NW",
        startpoints = { {x, z}, ... },
        boxes = {
            {
                {x, z},                  -- {x, z} OR {x, z, spline_strength}
                ...
            },
            -- additional disjoint polygons allowed per ally team
        },
    },
}
```

- Coordinates are elmos.
- Polygons are arrays of vertices, closed implicitly, conventionally clockwise.
- `spline_strength` in `[0, 1]` is rendering-only (Catmull-Rom tessellation); default `0`.
- Ally-team keys are 0-based; 1-based keys are normalised by the parser.
- `boxes` is an array, so a single ally team can own multiple disjoint polygons.
- Maps may conditionally `return` different tables based on gametype (`Spring.Utilities.Gametype.IsFFA()` etc.).

---

## Authoring recommendations

1. Emit only fields explicitly customised by the author. Defaults are stable; re-emitting every default obscures real authorial choices.
2. Treat the bare-minimum map as the validation floor.
3. Never write `teams[i].startPos.y` -- the engine recomputes Y from the heightmap on load.
4. Cosmetic fields (`description`, `author`, `version`) are metadata, not load-critical.
5. `maphelper/mapinfo.lua` -- emit a one-line `return VFS.Include("mapinfo.lua")` stub or skip the file entirely. The engine probes the helper path first but falls through to root.
6. Game-Lua content (`mapconfig/`, `LuaGaia/`, `LuaRules/`, `features/`, `libs/`) should be preserved verbatim by tools that don't intend to author gameplay logic.
7. Strip build artefacts and unused asset variants at export.

---
applyTo: "crates/bar-project/**"
---

# bar-project — Project & Recipe Serialisation Schema

## Role
`bar-project` is the serialisation boundary between the live in-memory graph
and the on-disk project representation. It owns the stable JSON formats for
recipes and projects, the complete map settings schema (used at export time
to generate `mapinfo.lua`), and the `WorkDirScan` type produced when an `.sd7`
archive is extracted. It deliberately does **not** depend on `bar-engine` so
that `bar-gui` can import project types without pulling in the full engine.

## Responsibilities
- Define the on-disk **recipe format**: `Recipe` stores nodes as
  `Vec<RecipeNode>` keyed by stable human-readable strings (rather than
  runtime `NodeId`s) and connections as `"key.port" → "key.port"` pairs.
- Validate recipes on load: duplicate keys, bad port references, zero
  dimensions, version range checks.
- Build a live `GraphEngine` from a `Recipe` via `Recipe::build_graph()` — the
  bridge between the serialisation world and the runtime graph world.
- Define the full **map settings schema**: `MapSettings`, `AtmosphereSettings`,
  `LightingSettings`, `WaterSettings`, `DetailTexture` (consumed by the engine
  export pipeline to emit `mapinfo.lua`).
- Wrap `Recipe` + `EditorLayout` into a `Project` (`.barproj` JSON) with
  save/load helpers.
- Own `WorkDirScan` (paths to `.smf`/`.smt`, tile-grid dims, passthrough files,
  optional map dimensions from the SMF header) without depending on engine code.

## Data Ownership
`bar-project` owns serialisation-layer copies of the graph structure. A call to
`Recipe::build_graph()` constructs and transfers ownership of a fresh
`GraphEngine` to the caller. `Project` owns its `Recipe` and `EditorLayout` in
memory; saving writes a snapshot to disk.

## Key Public Types
| Type | Description |
|---|---|
| `Recipe` | Versioned on-disk graph config; `load`, `from_json`, `to_json`, `build_graph`, `validate`, `sample` |
| `RecipeNode` | `key, node_type, label, params` |
| `RecipeConnection` | `from: "key.port"`, `to: "key.port"` |
| `OutputConfig` | `width, height, map_settings` |
| `MapSettings` | Heights, gravity, DNTS, atmosphere, lighting, water, start positions |
| `AtmosphereSettings` / `LightingSettings` / `WaterSettings` / `DetailTexture` | Sub-settings |
| `Project` | `version, recipe, layout`; `save`, `load` |
| `EditorLayout` | `node_positions (HashMap<String, Position>), canvas_offset, map_width, map_height` |
| `WorkDirScan` | Result of .sd7 extraction: SMF/SMT paths, tile grid, passthrough files, `map_dims` |
| `RECIPE_SCHEMA_VERSION` | On-disk recipe format version; bumped only with a matching migration arm |

## Schema Versioning

`Recipe` carries a `schema_version: u32` field (default `1` for files
that pre-date the field). The loader enforces forward-compat: a recipe
declaring a version newer than `RECIPE_SCHEMA_VERSION` is rejected
with a clear error rather than silently dropping unknown fields.

When changing the on-disk format incompatibly:

1. Bump `RECIPE_SCHEMA_VERSION` by one.
2. Add a migration arm in `Recipe::migrate_to_current()` that brings
   the previous version up to the new one in-place.
3. Bump and migrate in the **same commit**. Never bump silently.

Backwards-compat shims (per-field `serde(alias = ...)`, deprecated
helper functions) are not the right tool. Either bump the schema and
migrate, or break cleanly. The pre-release stance applies: there is
no installed base to protect.

Recipes also validate on load: duplicate keys, bad port references,
zero dimensions, AND per-node param schemas (via
`bar-graph::validate_node_params`). Type-mismatched params in
hand-edited recipes are rejected up front with the offending node and
key cited in the error.

## Interaction Surface
**Calls into:** `bar-graph` to construct `Node`, `GraphEngine`, `PortId` during
`build_graph()`; `serde_json` for serialisation.  
**Exposes to callers (`bar-engine`, `bar-gui`):**
- Stable JSON file formats for `.recipe.json` and `.barproj`
- `Recipe::build_graph()` — the serialisation → runtime bridge
- `MapSettings` — consumed by the engine's `SpringSmfCodec` / `mapinfo.lua`
  writer
- `WorkDirScan` — produced by `bar-engine`, consumed by `bar-gui`

## Boundaries — What This Crate Must NOT Do
- Must not depend on `bar-engine`, `bar-compute`, `bar-render`, `bar-gui`,
  `bar-app`, or `bar-cli` — this is the contract that prevents circular
  dependencies.
- Must not perform archive operations (no zip/7z/SD7 extraction) — that is
  `bar-engine`'s job.
- Must not contain GPU or CPU compute pipeline code.
- Must not contain any GUI code or egui imports.
- Format version constants (`RECIPE_VERSION`, `PROJECT_VERSION`) must be bumped
  whenever the on-disk schema changes in a backward-incompatible way.

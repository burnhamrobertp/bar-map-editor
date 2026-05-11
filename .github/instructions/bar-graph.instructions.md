---
applyTo: "crates/bar-graph/**"
---

# bar-graph — Graph Model & Evaluation Engine

## Role
`bar-graph` is the heart of the node-pipeline abstraction. It owns the live DAG
(`GraphEngine`), defines every `NodeType` variant, defines the runtime value
type (`PortValue`) that flows between nodes, and provides the topological
evaluator. It is deliberately kept free of compute and UI dependencies so that
both `bar-gui` (interactive editor) and `bar-cli` (headless runner) can import it
without pulling in GPU or windowing code.

## Responsibilities
- Maintain the typed, serialisable DAG: `GraphEngine` maps `NodeId → Node` and
  owns the `Vec<Connection>` list. Every structural mutation increments a
  monotonic `revision` counter so consumers can detect staleness cheaply.
- Enforce port-type compatibility on `connect`: `Heightmap ↔ Mask` are
  interchangeable; all other `PortKind` pairs must match exactly.
- Compute topological evaluation order via Kahn's algorithm and detect cycles.
- Define the `NodeType` enum covering all 40+ node variants (generators,
  filters, combiners, texture ops, mask ops, bundler/packaging, import nodes).
  Texture ops include: `SlopeMap`, `HeightSelect`, `SplatMap`, `AutoTexture`,
  `RockSoil`, `Vegetation`, `TextureOverlay`, `NormalMap`, `GrassMap`, `SpecularMap`.
- Define `PortValue` — the runtime value union (`Heightmap`, `Color`, `Mask`,
  `Scalar`, `File`, `FileList`, `Empty`) that flows between ports at eval time.
- Declare the `NodeExecutor` trait (`Send + Sync`) which the engine layer
  implements, and provide the `evaluate_graph` free function.
- Provide bundler-specific helpers: `find_bundler_nodes`,
  `get_bundler_node_*_output`, etc.

## Data Ownership
`GraphEngine` is the **single authoritative owner** of the live graph topology.
`PortValue` wraps clones of `Heightmap` / `ColorBuffer` during an evaluation
pass (functional/value-passing semantics — no aliasing). `bar-gui`'s
`OpenMachineApp` holds the `GraphEngine` during an interactive session.

## Key Public Types
| Type | Description |
|---|---|
| `GraphEngine` | DAG: nodes + connections + revision counter |
| `Node` | `id, node_type, label, position, inputs, outputs, params, dirty` |
| `NodeId` | Newtype `u64` |
| `NodeType` | Enum of all node variants |
| `ParamValue` | `Float, Int, UInt, Bool, String, Vec2` |
| `Port` / `PortId` | Port definition and identity `(node_id, port_name)` |
| `PortKind` | `Heightmap, Mask, Color, Scalar, File, FileList` |
| `PortValue` | Runtime value union |
| `FileRef` | `path + bundle_path` for file pass-through |
| `NodeExecutor` | Trait: `execute(node_type, params, inputs, w, h) -> Result<…>` |
| `EvalError` | Evaluation error enum |
| `evaluate_graph` | Free fn: runs topological evaluation using a `NodeExecutor` |
| `ParamSpec` / `ParamKind` | Per-`NodeType` param schema; `param_specs(nt)` derives it from `default_params` |
| `validate_node_params` | Free fn: returns `Vec<ParamError>` for bad keys/types |

## ParamSpec — Schema Derived from Defaults

`default_params(nt)` returns the canonical `(name, default_value)` list
for every node type. `param_specs(nt)` derives the schema from that
single source of truth: each entry's name + the `ParamKind` of its
default value. There is no separate hand-maintained spec table to
keep in sync.

**Validation strictness:**

- **Type mismatches are hard errors.** A param declared `Float` cannot
  silently accept an `Int`; `Recipe::validate` rejects the recipe with
  a clear node-and-key citation. Past behaviour was to fall back to
  the default at executor time, which produced wrong-but-not-broken
  evaluations.
- **Unknown keys are tolerated.** Old projects carrying params from
  removed node-type variants still load; the unknown key is dropped on
  evaluation. Strict mode is reserved for a future `RECIPE_SCHEMA_VERSION`
  bump that wants to enforce it.

When you add a new param to `default_params`, the spec updates
automatically. When you rename or remove a param, hand-edited recipes
referring to the old name will warn (currently silent) — make sure you
also bump the schema version if the change is incompatible.

## Interaction Surface
**Calls into:** `bar-data` only (for `Heightmap` and `ColorBuffer` in
`PortValue`); no wgpu, no egui, no file I/O.  
**Exposes to callers (`bar-project`, `bar-engine`, `bar-gui`, `bar-app`, `bar-cli`):**
- Full DAG CRUD API (`add_node`, `remove_node`, `connect`, `disconnect`, …)
- `evaluate_graph` + `topological_sort`
- `NodeExecutor` trait contract
- Bundler output helpers

## Boundaries — What This Crate Must NOT Do
- Must not depend on `bar-compute`, `bar-engine`, `bar-render`, `bar-gui`,
  `bar-app`, or `bar-cli`.
- Must not depend on `bar-project` (the project layer sits above the graph).
- Must not contain GPU or CPU compute pipeline implementations — those live in
  `bar-compute` and are wired in by `bar-engine`.
- Must not perform any file I/O (disk reads, archive operations).
- Must not open or interact with any GUI.
- `NodeExecutor` is a trait; its implementations (`CpuExecutor`,
  `HybridExecutor`) live in `bar-engine`.

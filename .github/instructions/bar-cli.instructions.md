---
applyTo: "crates/bar-cli/**"
---

# bar-cli — Headless Command-Line Runner

## Role
`bar-cli` provides scriptable, headless terrain
generation and project management without any GUI or GPU context. Every
command is a one-shot read/process/write pipeline with no persistent state.

## Responsibilities
Implement six `clap`-derived subcommands:

| Subcommand | Description |
|---|---|
| `run` | Load a `Recipe`, build the `GraphEngine`, evaluate via `CpuExecutor`, export via `--target <ID>` or all Bundler nodes. Supports `--width`/`--height` overrides and `--bundler` label filter. |
| `validate` | Load and validate a recipe (JSON parse, node keys, graph build, topological sort). Report counts and errors. |
| `info` | Print nodes, connections, params, and evaluation order for a recipe. |
| `new` | Generate and print/write the built-in sample recipe via `Recipe::sample()`. |
| `targets` | List registered export targets from `TargetRegistry`. |
| `import` | Call `import_sd7_to_project`, save a `.barproj` file, report map dimensions. |

## Data Ownership
No persistent state. Each command opens inputs, processes them, writes outputs,
and exits. There are no long-lived structs beyond the scope of a single command
handler.

## Key Internal Types
| Type | Description |
|---|---|
| `Cli` | `clap::Parser` root struct |
| `Commands` | `clap::Subcommand` enum: `Run{…}`, `Validate{…}`, `Info{…}`, `New{…}`, `Targets`, `Import{…}` |

Both types are private to the binary.

## Interaction Surface
**Calls into:**
- `bar-engine`: `CpuExecutor`, `Recipe`, `execute_bundlers`, `find_bundler_nodes`,
  `export_with_target`, `import_sd7_to_project`, `TargetRegistry`
- `bar-graph`: `evaluate_graph`, `GraphEngine::topological_sort`

**Exposes:** The `bar-cli` binary with six subcommands. No library API.

## CLI Behaviour Notes
- `bar-cli` always uses `CpuExecutor` — there is no GPU context in headless
  mode. Do not add a `HybridExecutor` path unless a GPU context lifecycle is
  also added.
- Exit codes: `0` for success, non-zero on any error (use `anyhow::Error`
  propagation through `main() -> anyhow::Result<()>`).
- All user-facing output goes to stdout; errors/diagnostics to stderr via
  `tracing`.

## Boundaries — What This Crate Must NOT Do
- Must not depend on `bar-render`, `bar-gui`, `bar-app`, or `bar-compute`.
- Must not create a wgpu device or GPU context.
- Must not open any GUI windows.
- Must not hold mutable state between subcommand invocations.

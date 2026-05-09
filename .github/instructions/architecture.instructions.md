---
applyTo: "**"
---

# Architecture Principles

Cross-cutting patterns and conventions that apply across the workspace.
Per-crate guidance lives in the corresponding `<crate>.instructions.md`.

## Pre-release stance: no backwards compatibility

The bar-editor is pre-release with one user. There is no installed base.
When renaming a field, restructuring a struct, or reorganising a module:
break cleanly. Don't write `serde(alias = ...)` shims, deprecated method
aliases, or "for backwards compat" branches. They calcify into permanent
maintenance debt.

The one exception is on-disk format compatibility within a published
schema version, which is what `RECIPE_SCHEMA_VERSION` exists for. See
the project crate guidance for that mechanism.

## Single source of truth over mirroring

Live editor state and on-disk state mirror each other in many places
(map dimensions, recipe identity, paint caches). The temptation is to
keep two parallel copies for ergonomic egui binding. Resist it. Mirror
fields drift. Past examples:

- `recipe_shortname` / `recipe_description` / `recipe_author` /
  `recipe_version` mirroring `Recipe` identity fields. Collapsed into
  `BarEditorApp::recipe_meta: RecipeMeta`.
- `start_positions: Vec<[u32; 2]>` mirroring
  `MapSettings::start_positions`. Removed; the editor reads/writes the
  `MapSettings` field directly.
- `inspector_*` paint caches scattered across loose `BarEditorApp`
  fields. Grouped into `PaintSession` with explicit
  `invalidate_on_graph_reset()`.

When you find yourself adding a "live" copy of a serialisable field,
ask whether the editor can bind directly into the source via
`&mut self.field` or a `pub(crate)` accessor instead.

## Concern-owned state

When five-plus fields belong together, give them a typed sub-state
struct, put its methods in the same module, and expose it as a `pub`
field on the owner so callers can reach it directly
(`app.preview.is_open()`, `app.map.dimensions()`).

`BarEditorApp`'s sub-states today:

| Field | Type | Concern |
|---|---|---|
| `visuals` | `editor::VisualsState` | Node positions, groups, hit-test rects. |
| `selection` | `editor::SelectionState` | What's selected on the canvas. |
| `canvas` | `editor::CanvasState` | Pan, tabs, marquee, drag connection. |
| `map` | `editor::MapState` | Dimensions, height range, MapSettings, RecipeMeta. |
| `project` | `project::ProjectState` | File path, dirty, autosave, async I/O channels. |
| `preview` | `editor::PreviewState` | Viewport open, run/test pulses, export status. |
| `dialog` | `DialogState` | Modal/popup/transient feedback flags. |
| `validation` | `editor::ValidationState` | Findings, filter, fingerprint gate. |
| `props` | `editor::PropsPanelState` | Floating props popup binding. |
| `paint` | `PaintSession` | Brush, sculpt-lock, per-layer paint caches. |

Each sub-state lives in the module named for its concern, and the
methods that operate on it live next to it. `PaintSession` further
owns its invariants via `invalidate_on_graph_reset()` rather than
scattering that logic across call sites.

When you add another five-plus-field cluster, do the same. Don't
grow `BarEditorApp` past 30 top-level fields without a structural
reason. See `bar-gui.instructions.md` for the full module map and
the "where does this go?" table.

## Schema versioning at the persistence boundary

Anything that round-trips through a JSON file (`Recipe`, future formats)
gets a `schema_version: u32` field with `serde(default = ...)` for old
files. The loader refuses anything newer than the build supports. New
migrations land in a single `migrate_to_current()` function near the
loader, never as scattered field-level `#[serde(alias)]` patches.

When you bump the schema version, bump the constant AND add the
migration arm in the same commit. Never bump silently.

## Async I/O off the egui main thread

`egui` runs on the main thread. Any blocking syscall on the main thread
freezes the UI for the duration. The two patterns that have come up:

**Modal file dialogs.** `rfd::FileDialog::pick_file()` /
`pick_folder()` block until the user picks or cancels (often hundreds
of ms even before they see the dialog on Windows). Spawn the dialog on
a worker thread, return result via `mpsc::channel`, poll the receiver
each frame. See `BarEditorApp::open_file_dialog_async` and
`AppWrapper::pending_export_dir` for the established pattern.

**Long-running compute.** Graph evaluation, SD7 extraction, export.
Same pattern: spawn on a worker, send result via channel, poll in
`update`. The channel-receive site is responsible for guarding against
stale results from a superseded request (see the cache-key + session-id
gate around `eval_preview`).

Never call `pick_file` / `pick_folder` / `evaluate_graph` /
`extract_sd7_to_work_dir` directly from `update`.

## ParamValue is enum-tagged on disk

`ParamValue` serialises with serde's external tagging — `{"Float": 1.5}`,
`{"String": "foo"}`. When constructing test JSON or hand-editing
recipes, use that shape. Validation at recipe load (see
`bar-graph::param_spec::validate_node_params`) catches type-mismatched
hand edits with a clear error citing the offending node + key.

## Reset session state on every project switch

Project-data fields (graph, project_path, map_settings) and
session-transient fields (undo history, brush, dialog flags, canvas
pan, validation cache, paint caches, palette drag, run/test request
flags, tabs) get cleared whenever the project changes. The single
clearing point is `BarEditorApp::reset_session_state` plus
`PaintSession::invalidate_on_graph_reset`. Both are called from
`do_new_project`, `apply_project`, and `start_with_macro`.

When adding a new transient field to `BarEditorApp`, decide whether it
crosses a project boundary and update `reset_session_state` if not.
Otherwise the next user-reported regression will be "I loaded a new
project and the old project's [thing] was still there."

## Three-layer UI: state, panels, layouts

`bar-gui` has a deliberate three-layer split:

1. **`BarEditorApp` (state).** Single owner of all live editor data
   (graph, paint, dialog, project, undo, settings).
2. **`panels/*` (stateless renderers).** One module per UI surface.
   Panels never own state; they read/write `BarEditorApp` fields via
   `pub(crate)` accessors or distributed `impl BarEditorApp` blocks.
3. **`layouts/*` (panel composers).** A `Layout` enum variant maps to
   one file that decides which panels are visible and how they're
   arranged. The active layout is persisted in user settings.

Adding a new panel or layout is a localised change. See the bar-gui
instructions for the per-panel cookbook.

## Visibility convention

`pub` reaches outside the crate. `pub(crate)` reaches across modules
within the crate. Default visibility (private) is the same-module
default.

For `BarEditorApp` sub-state fields (`preview`, `map`, `project`,
`canvas`, `selection`, `visuals`, `props`, `dialog`, `validation`,
`paint`), the field itself is `pub` and the sub-state struct's
fields are `pub` so callers can do `app.preview.is_open()`.
`BarEditorApp`'s direct fields (`graph`, `history`, `settings`,
`active_layout`, `palette_drag`) stay `pub(crate)` and external
code reaches them via `pub` accessors.

Methods that mutate multiple fields together are `pub` methods on
`BarEditorApp` (e.g. `reset_session_state`, the brush flow,
`finish_open_map`). Helpers that only one panel needs stay private
to that panel module.

The `pub` API of the crate (everything in `pub use ...` from
`lib.rs`) stays narrow. `bar-app` and `bar-cli` are the only
consumers outside `bar-gui` itself; their interface is the public
sub-state fields plus a small set of orchestration methods on
`BarEditorApp`.

## Tests as the build gate

Workspace tests are the contract. After any structural change (panel
extraction, field move, accessor rename), run `cargo test --workspace`
before declaring done. The suite catches:

- Field renames that missed call sites.
- Visibility bumps that didn't take.
- Borrow-checker regressions across module boundaries.
- Schema-version handling regressions.

Don't ship a structural change without the suite green.

## Comment WHY, not WHAT

Identifier names already describe what the code does. Comments earn
their keep when they explain a non-obvious constraint, a workaround
for a specific bug, an invariant that isn't visible from the type
signature, or a decision the reader would otherwise question. Past
incidents and design rationale are the high-value targets.

Don't reference the PR number, the issue number, or "the X stage" —
those rot as the codebase evolves. Don't write multi-paragraph
docstrings on internal helpers. Module-level `//!` docs are the
exception: they earn their length by orienting a new reader to the
file's purpose.

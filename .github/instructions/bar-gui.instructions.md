---
applyTo: "crates/bar-gui/**"
---

# bar-gui -- Node Graph Editor UI

## Role
`bar-gui` is the complete interactive node-graph editor. It owns the
live `GraphEngine` and all editor state for an interactive session.
It communicates with `bar-app` through public sub-state accessors on
`BarEditorApp` (e.g. `app.preview.is_open()`, `app.map.dimensions()`,
`app.project.take_graph_reset()`); it never imports the engine,
compute, or render layers.

## Concern-owned state

`BarEditorApp` is the single owner of every live editor field. To
keep that owner readable, fields are grouped into typed sub-state
structs by concern, and the methods that operate on each sub-state
live in the same module. Cross-cutting orchestration (frame work,
project load/save funnel, brush flow that touches paint+history+
visuals) stays on `BarEditorApp` itself but delegates to sub-state
methods for everything that fits within one concern.

The single-owner / stateless-panel pattern is preserved: panels are
stateless renderers that take `&mut BarEditorApp`. Sub-state structs
are field clusters owned by `BarEditorApp`, not separate top-level
objects.

### Sub-state structs

| Field on `BarEditorApp` | Type | Module | Concern |
|---|---|---|---|
| `graph` | `GraphEngine` | bar-graph | The live DAG. |
| `visuals` | `editor::VisualsState` | `editor/visuals.rs` | Node positions, groups, group/node reverse index, monotonic group id allocator, per-frame hit-test rect caches. |
| `selection` | `editor::SelectionState` | `editor/selection.rs` | Primary node, multi-selection, selected group, selected connection, group queued for deletion. |
| `canvas` | `editor::CanvasState` | `editor/canvas.rs` | Pan offset, open tabs, marquee anchor, in-progress drag connection, viewport rect cache. |
| `map` | `editor::MapState` | `editor/map.rs` | Dimensions, height range, `MapSettings`, `RecipeMeta`, spawn-marker drag pointer. |
| `history` | `UndoHistory` | `undo.rs` | Bounded snapshot stack. |
| `project` | `project::ProjectState` | `project/state.rs` | File path, dirty flag, autosave timer, SD7 extraction handoff, file-dialog poll receiver, inline file editor, graph-reset pulse, map-info file pointer, `loaded_name`. |
| `preview` | `editor::PreviewState` | `editor/preview.rs` | Viewport open flag, driving node, run / test-in-BAR / run-bundler pulses, export status. |
| `dialog` | `DialogState` | `app.rs` (still inline; sub-module pending) | Modal/popup/transient flags, confirm dialogs, file editor, toast, status, pending props open. |
| `validation` | `editor::ValidationState` | `editor/validation.rs` | Findings list, severity filter, mapinfo modal tab, fingerprint gate. |
| `props` | `editor::PropsPanelState` | `editor/props_panel.rs` | Floating properties popup target binding and last-known on-screen rect. |
| `paint` | `PaintSession` | `paint/mod.rs` | Brush, sculpt-lock, per-layer paint caches, inspector mode. |
| `settings` | `Settings` | `settings.rs` | Persistent user prefs. |
| `active_layout` | `Layout` | `app.rs` | Active top-level UI workspace. |
| `parent_window_handles` | `Option<(RawWindow, RawDisplay)>` | `io/dialogs.rs` | Captured per-frame for parenting native dialogs. |

When a new five-plus-field cluster appears in `BarEditorApp`, give
it a sub-state struct and put its methods in the same module. Don't
grow `BarEditorApp` past 30 top-level fields without a structural
reason.

## Module layout

```
crates/bar-gui/src/
  app.rs              # BarEditorApp + Default + frame work + cross-cutting
                      # orchestration (reset_session_state, brush funnel,
                      # eframe::App impl). Distributed `impl BarEditorApp`
                      # blocks in panel/editor/project files attach the rest.
  lib.rs              # Module list and public re-exports.
  state.rs            # Small shared types (e.g. SubgraphPortRuntime).

  editor/             # Canvas-side editor concerns.
    mod.rs
    canvas.rs         # CanvasState + tab/viewport/drag-connection methods.
    selection.rs      # SelectionState.
    visuals.rs        # VisualsState + group runtime + group color allocator.
    props_panel.rs    # PropsPanelState + hover gate + outside-click close.
    validation.rs     # ValidationState + run/refresh + counts + severity.
    preview.rs        # PreviewState + take_run_*, set/get export status,
                      # cache_key composer.
    map.rs            # MapState + dimension/height accessors.

  project/            # Project lifecycle.
    mod.rs            # Re-exports.
    state.rs          # ProjectState struct + small accessors.
    lifecycle.rs      # do_new_project / start_open_path / dispatch_open /
                      # apply_project / open_map_as_project /
                      # finish_open_map / start_with_macro /
                      # reset_session_state.
    persistence.rs    # build_project / pack_assets_for_save /
                      # resolve_relative_paths.
    autosave.rs       # autosave_now + gate.
    sculpt_sidecar.rs # pack/unpack sculpt record + sculpt_export_snapshot.
    path.rs           # Path helpers (resolve, project-relative, packing,
                      # files_equal).

  io/                 # External I/O integration.
    mod.rs            # is_text_file extension check.
    png.rs            # Heightmap / color-buffer PNG load + save.
    dialogs.rs        # ParentWindow wrapper + make_dialog +
                      # open_file_dialog_async.

  paint/              # Brush + paint state and math.
    mod.rs            # PaintSession (re-exported for back-compat).
    brush_math.rs     # Pure brush dab math + tests.

  panels/             # Stateless renderers.
    mod.rs
    canvas/           # Replaces the former panels/node_canvas.rs.
      mod.rs          # Public draw entry point + shared NodeStyle.
      render.rs       # Node bodies, ports, wires, group frames,
                      # collapsed-subgraph blocks.
      groups.rs       # Group create/dissolve, hit-test, collapsed-subgraph
                      # layout + draw.
      layout.rs       # Auto-layout + auto-wire.
      tabs.rs         # Tab strip + open/close/activate.
    properties/       # Replaces the former panels/properties.rs.
      mod.rs          # tick_props_panel, dispatcher, common widgets.
      sculpt.rs       # Sculpt body.
      painted_heightmap.rs
      painted_texture.rs
      pass_through.rs
      group.rs        # Group properties body.
    inspector.rs      # 2D inspector window.
    mapinfo_editor.rs # Structured Map Info modal.
    validation.rs     # Sidebar summary + floating details window.
    palette.rs        # Node palette + drag-into-canvas.
    welcome.rs        # Empty-graph welcome screen.
    dialogs.rs        # Settings, About modals.
    tokens.rs         # Semantic colour constants (single source of truth).
    icons.rs          # Toolbar / canvas icon paint functions.

  layouts/            # Panel composers.
    mod.rs
    dispatch.rs       # draw_active(app, ctx, frame).
    standard.rs       # The default editor layout.

  i18n.rs             # t!() + catalogue init from embedded language/.
  macros.rs           # Built-in graph macros / preset loader.
  settings.rs         # Settings struct + load/save.
  undo.rs             # UndoHistory + Snapshot + bounded ring.
```

`BarEditorApp` is the single owner. Panels are stateless. Layouts
compose panels. Sub-state structs cluster fields by concern.

## Where does this go?

When you're about to add a method or a field, use this table to pick
the file. The rule of thumb: if the method touches one sub-state,
put it next to that sub-state; if it touches multiple sub-states,
keep it on `BarEditorApp` in `app.rs` and delegate.

| You are adding... | Put it in... |
|---|---|
| A field about brush state, paint caches, or sculpt lock | `paint/mod.rs` (`PaintSession`) |
| A field about which thing(s) on the canvas are selected | `editor/selection.rs` |
| A field about node/group visual layout or hit-test rects | `editor/visuals.rs` |
| A field about pan, tabs, marquee, or in-flight wire drag | `editor/canvas.rs` |
| A field about preview viewport, run/export pulses, or export status | `editor/preview.rs` |
| A field about validation findings or filter | `editor/validation.rs` |
| A field about map dimensions, height range, settings, or recipe meta | `editor/map.rs` |
| A field about the floating props popup binding | `editor/props_panel.rs` |
| A field about project file path, dirty, autosave, or async I/O channels | `project/state.rs` |
| A node-rendering primitive (port circle, wire, group frame) | `panels/canvas/render.rs` |
| A node-canvas interaction (hit-test, drag, group dissolve) | `panels/canvas/groups.rs` or `panels/canvas/layout.rs` |
| A new per-NodeType property body | `panels/properties/<node_type>.rs` and registered in `panels/properties/mod.rs::draw_properties` |
| A path/asset packing helper | `project/path.rs` |
| A PNG load/save helper | `io/png.rs` |
| A semantic colour | `panels/tokens.rs` |
| A toolbar/canvas icon paint function | `panels/icons.rs` |

If your method touches more than one of the above, it belongs on
`BarEditorApp` itself in `app.rs`. Examples: brush flow that mutates
`paint` + `visuals` + `history`; project switch that resets every
sub-state; preview cache key composed from `graph` + `map`.

## Layout enum

`Layout` is the persisted UI workspace selection (today: `Standard`,
`Sculpt3D`; future variants drop in as new files + one match arm).
The choice lives on `BarEditorApp::active_layout` and is mirrored in
`Settings::active_layout` so it survives restart.

Layouts are **pure UI/UX**. The same `BarEditorApp` underlies every
layout; switching is instant and never migrates project state.

To add a layout:

1. Add a variant to the `Layout` enum in `app.rs`.
2. Create `crates/bar-gui/src/layouts/<name>.rs` exposing
   `pub fn draw(app: &mut BarEditorApp, ctx: &egui::Context, frame: &mut eframe::Frame)`.
3. Register it in `layouts::mod`.
4. Add a match arm in `layouts::dispatch::draw_active`.
5. Add a UI affordance for switching (top-toolbar dropdown).

A layout reads any `BarEditorApp` field but only calls into
`crate::panels::*` for actual rendering. Panels are reusable across
layouts; never duplicate panel logic in a layout module.

## Panel cookbook

When a chunk of inline UI gets large enough to deserve its own file,
extract it. Pattern:

1. **New file** under `panels/<name>.rs` (or a sub-module like
   `panels/canvas/<name>.rs`) with module-level `//!` docs explaining
   what the panel renders.
2. **Stateless entry point.** Either
   - `pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context)`
     for top-level windows / panels, or
   - `pub(crate) fn draw(app: &mut BarEditorApp, ui: &mut egui::Ui)`
     for panels that render into a parent `Ui`.
3. **Field access via sub-state fields.** With sub-state structs, a
   panel typically reads `app.canvas.tabs`, `app.selection.node`,
   `app.visuals.node_visuals` directly. Add `pub(crate)` accessors
   only where invariants need enforcing.
4. **Distributed `impl BarEditorApp` blocks** are also fine when the
   panel needs heavy `&mut self` access. The block lives in the
   panel file, methods are `pub(crate) fn`, fields they touch are
   `pub` (sub-state) or `pub(crate)` (`BarEditorApp` direct fields).
5. **Replace the call site** in the layout with a single
   `crate::panels::<name>::draw(self, ...)` call.
6. **Run `cargo test --workspace`.** The suite is the contract.

`panels/canvas/` and `panels/properties/` are the largest worked
examples of the sub-module split; `welcome.rs` and `dialogs.rs` are
the simplest references.

Confirm-dialog and unsaved-changes prompts intentionally stay in
`app.rs` because they're tightly coupled to the `PendingAction` flow
control. Don't try to extract those without restructuring the action
queue first.

## Async I/O pattern

The egui main thread must not block. Native file dialogs (`rfd`)
and long-running compute (export, SD7 extract, graph evaluation)
all spawn worker threads and return results via `mpsc::channel`.

Established sites:

- `io::dialogs::open_file_dialog_async` -- spawns the OS file
  picker; result polled via `app.project.pending_open_rx` in
  `update_panels`.
- `AppWrapper::pending_export_dir` -- same pattern for the export
  folder picker, two-phase: dialog spawn -> folder result triggers
  the export thread.
- Preview eval pipeline (in `bar-app`) -- the cache-key + session-id
  gate around `eval_preview` rejects stale results.

Native file dialogs go through `BarEditorApp::make_dialog()` (in
`io/dialogs.rs`) which sets the parent window so the dialog belongs
to the editor, not whatever window happens to be foreground.
`bar-app` can build its own `ParentWindow` via
`app.parent_window()`.

Never call `pick_file` / `pick_folder` / `evaluate_graph` /
`extract_sd7_to_work_dir` directly from `update`. Spawn, poll,
guard against staleness.

## Reset session state

Every project switch (new, open, preset macro) clears transient
state through one funnel:

```
do_new_project -> reset_session_state -> ...
apply_project (file load) -> reset_session_state -> ...
start_with_macro (preset) -> reset_session_state -> ...
```

`reset_session_state` (in `project/lifecycle.rs`) clears history,
dialog flags, validation cache, canvas pan, palette drag, dialogs/
modals, run/test request flags, tabs.
`PaintSession::invalidate_on_graph_reset` is called from inside
`reset_session_state` -- paint state is session-only.

When you add a new transient field to `BarEditorApp` (or any
sub-state), decide whether it crosses a project boundary and update
`reset_session_state` if it shouldn't survive. The next bug report
will be "I loaded a new project and the old project's [thing] was
still there" if you skip this.

## Visibility convention

- Sub-state structs (`PreviewState`, `MapState`, `CanvasState`,
  `SelectionState`, `VisualsState`, `PropsPanelState`,
  `ValidationState`, `ProjectState`, `PaintSession`, `DialogState`):
  fields are `pub` so panels and `bar-app` can access them directly.
- `BarEditorApp` fields that hold sub-state structs are `pub` so
  external callers can do `app.preview.is_open()`.
- `BarEditorApp` direct fields (`graph`, `history`, `settings`,
  `active_layout`, `palette_drag`) are `pub(crate)` and reached via
  `pub` accessors when external code needs them.
- Panel render functions: `pub(crate)`.
- Per-panel free helpers (drawing primitives, layout maths): keep
  private to the panel module.
- The `pub` API of the crate (everything in `pub use ...` from
  `lib.rs`) stays narrow. `bar-app` and `bar-cli` are the only
  outside consumers.

## Localization (i18n)

User-facing strings go through `t!()` from `crate::i18n`, which
looks up keys against a catalogue built from the embedded
`language/` tree. The layout matches `bar-localizations` so files
round-trip without a format transform -- see `language/README.md`
for the full sync convention.

```
language/
  en/
    common.json     # Cross-app strings (top-level key: "common")
    editor.json     # Editor-specific strings (top-level key: "editor")
```

| Namespace  | What goes here                                                      |
|---         |---                                                                  |
| `common.*` | Strings that should read the same across every BAR application. Examples: `Cancel`, `OK`, `Save`, `Open`, `Yes`, `No`, generic dialog labels. If two BAR apps would localise the same English string differently, it's not common. |
| `editor.*` | Strings unique to the bar-editor: menu items, panel headings, validation messages, macro descriptions, status notifications, anything tied to editor concepts (Bundler, Preview, Macro, Sculpt, etc.). |

When in doubt, default to `editor.*`. Promoting later is cheap.

Add a string by editing `language/en/common.json` or
`language/en/editor.json`, picking a stable dotted key
(`editor.menu.file`, `common.cancel`), and referencing it via
`t!("editor.menu.file")`. Interpolation: `%{var}` in the JSON,
`t!("...", name = template.name)` from Rust. Missing keys
return the literal key string at runtime.

## Design Tokens -- `panels/tokens.rs`

All semantic UI colors are defined as named `egui::Color32` constants
in `crates/bar-gui/src/panels/tokens.rs`. This is the **single source
of truth** for the visual palette. Never add an inline
`Color32::from_rgb(...)` or `Color32::from_rgba_unmultiplied(...)` for
a color that has semantic meaning.

The module is grouped into families:

| Group | Prefix | Examples |
|---|---|---|
| Port kinds | `PORT_*` | `PORT_HEIGHTMAP`, `PORT_TEXTURE`, `PORT_MASK` |
| Node categories | `NODE_CAT_*` | `NODE_CAT_GENERATORS`, `NODE_CAT_FILTERS` |
| Node body states | `NODE_*` | `NODE_BG`, `NODE_BG_SEL`, `NODE_BORDER`, `NODE_BORDER_SEL` |
| IO node (SubgraphInput/Output) | `IO_*` | `IO_BODY`, `IO_BORDER_SEL`, `IO_LABEL_PRI` |
| Tab strip | `TAB_*` | `TAB_BG_ACTIVE`, `TAB_LABEL_ACTIVE`, `TAB_LABEL_DIM` |
| Severity | `SEVERITY_*` | `SEVERITY_ERROR`, `SEVERITY_WARN`, `SEVERITY_INFO` |
| Wires | `WIRE_*` | `WIRE_DEFAULT`, `WIRE_SELECTED`, `WIRE_DRAG` |
| Toolbar buttons | `BTN_<NAME>_NORMAL/HOVER/PRESS` | `BTN_VALIDATE_HOVER`, `BTN_EXPORT_PRESS` |
| Misc | -- | `ICON_STROKE`, `CANVAS_BG` |

**Rules:**
- Before adding a `Color32::from_rgb(...)` anywhere in `bar-gui`,
  ask whether the color is already represented in tokens. If yes,
  use the token. If no, add the token first.
- Colors that appear in more than one panel -- especially those that
  pair visually (e.g. validation severity in the sidebar summary AND
  the details window) -- **must** share a token so they can't drift.

## Icon Functions -- `panels/icons.rs`

All toolbar and canvas icon paint functions live in
`crates/bar-gui/src/panels/icons.rs`.

**Rules:**
- New icons go in `icons.rs`.
- All icon functions take `(painter: &egui::Painter, rect: egui::Rect,
  color: egui::Color32)` -- caller supplies tint so the same icon
  renders at hover/active/normal without branching inside the function.
- Icon stroke color defaults to `tokens::ICON_STROKE`; use that
  rather than a hardcoded RGB when the icon is part of a panel (not
  caller-tinted).
- Icon geometry is defined in logical coordinates relative to the
  supplied `rect` -- never absolute screen coordinates.

## Node Canvas Style -- `NodeStyle`

All node body geometry and color constants are grouped in the
`NodeStyle` struct defined in `panels/canvas/mod.rs`.
`NodeStyle::default()` reads from `panels/tokens`.

```rust
pub(crate) struct NodeStyle {
    pub bg:             egui::Color32,  // tokens::NODE_BG
    pub bg_sel:         egui::Color32,  // tokens::NODE_BG_SEL
    pub bg_pri:         egui::Color32,  // tokens::NODE_BG_PRI
    pub border:         egui::Color32,  // tokens::NODE_BORDER
    pub border_sel:     egui::Color32,  // tokens::NODE_BORDER_SEL
    pub border_w:       f32,            // 1.5
    pub border_w_sel:   f32,            // 2.0
    pub rounding:       f32,            // 4.0
    pub title_h:        f32,            // 20.0
    pub title_rounding: egui::CornerRadius, // {nw:4, ne:4, sw:0, se:0}
}
```

**Rules:**
- Node-drawing code obtains geometry and colors via
  `let ns = NodeStyle::default()` -- never via magic literal floats or
  inline `Color32` constructions.
- To change the look of all nodes, change `NodeStyle::default()` or
  the token it reads. To give one node type a different look, either
  call a variant constructor or override individual fields after
  construction.

## Properties Panel -- Layout Patterns

### Two-column label + widget layout

The generic parameter editor in `panels/properties/mod.rs` uses
`egui::Grid` for label/widget alignment. This is the required
pattern wherever a scrollable list of `label | widget` rows appears:

```rust
egui::Grid::new(("params_grid", node_id.0))
    .num_columns(2)
    .spacing([8.0, 4.0])
    .show(ui, |ui| {
        for (key, val) in &params {
            ui.label(key);
            ui.add(widget_for(val));
            ui.end_row();
        }
    });
```

Do not simulate a two-column layout with `ui.horizontal` and
fixed-width `allocate_exact_size` -- that produces fragile pixel
math that breaks at non-default DPI.

### `Bool` rows in a Grid

Bool params sit in a Grid cell using an empty first column so the
checkbox lines up with other widgets:

```rust
ui.label("");          // column 1 -- placeholder for alignment
ui.checkbox(&mut val, key);  // column 2 -- label is inside the checkbox
ui.end_row();
```

### `TextEdit` width

Use `desired_width(f32::INFINITY)` on `TextEdit` in Grid column 2 so
it fills the available cell width.

### `DragValue` width -- egui 0.31 limitation

`egui::DragValue` in egui **0.31** does **not** have a
`desired_width` method. Do not call `.desired_width(...)` on a
`DragValue` -- it will not compile. If you need a wider DragValue,
use `ui.add_sized([width, height], DragValue::new(...))`.

### Per-NodeType bodies

Each non-trivial NodeType has its own file under
`panels/properties/<node_type>.rs`. The dispatcher in
`panels/properties/mod.rs::draw_properties` matches on `NodeType`
and calls into the appropriate file. To add a new per-NodeType body:

1. Create `panels/properties/<node_type>.rs` exposing a
   `pub(crate) fn draw(app: &mut BarEditorApp, ui: &mut egui::Ui,
   node_id: NodeId)`.
2. Register the file in `panels/properties/mod.rs`.
3. Add a match arm in the dispatcher.

The generic fallback (parameter Grid) handles any NodeType not in
the dispatcher.

## Node Layout Constants

```
PORT_Y_BASE = 30.0    // first port Y offset below node top
PORT_Y_STEP = 20.0    // vertical spacing between ports
IO_NODE_SIZE = (160, 52)   // SubgraphInput / SubgraphOutput pill
```

Terminal node specifics:
- **Bundler** (export sink): title bar 20 px; Export button
  top-right, 44 x 36 px, matches the toolbar Run button style.
  No footer -- Bundler is for export only and never drives the 3D
  viewport.
- **Preview** (3D viewport sink): "Open" footer full-width,
  22 px, rounded bottom corners, teal -- clicking it opens or
  re-targets the 3D viewport. The Preview node has explicit
  inputs (`heightmap` required; `texture` / `normal_map` /
  `specular_map` optional) and is the sole driver of the
  viewport.
- **SubgraphInput / SubgraphOutput**: render as a pill (no
  header) with a directional chevron, an arrow icon, and a
  two-line "Input"/"Output" + type label. See
  `panels/canvas/render.rs::draw_io_icon` for the icon geometry.

## UI Interaction Rules

- Clicking the Export button on a Bundler or the "Open" footer
  on a Preview must not select the node. Register the button
  interactions before the node background `interact` call so
  they have priority.
- `egui::TopBottomPanel::bottom` must be declared **before**
  `SidePanel::left/right` for correct sizing.
- The properties panel header shows the node label as
  `ui.heading()` with the type name below -- do not add a
  redundant "Properties" title. SubgraphInput/SubgraphOutput
  nodes specifically suppress the title and type display.
- `push_undo` must be called **after** (not before) the graph
  mutation it describes. Never call it before replacing the
  graph (e.g. on project load) -- that would insert the previous
  graph as undo entry #1 of the new session.

## Boundaries -- What This Crate Must NOT Do

- Must not depend on `bar-compute`, `bar-engine`, `bar-render`,
  `bar-app`, or `bar-cli`.
- Must not create wgpu resources or submit GPU work.
- Must not spawn long-running threads beyond the brief
  file-dialog / SD7-extract workers (preview eval workers live
  in `bar-app`).
- Must not call `bar-engine` export functions directly -- signal
  `bar-app` via `app.preview.take_run_requested()` /
  `app.preview.take_run_bundler_node()` and let the app layer
  handle execution.
- Must not modify `GraphEngine` revision counter logic
  directly -- use the `GraphEngine` mutation API which increments
  the revision.

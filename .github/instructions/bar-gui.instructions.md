---
applyTo: "crates/bar-gui/**"
---

# bar-gui — Node Graph Editor UI

## Role
`bar-gui` is the complete interactive node-graph editor. It owns the
live `GraphEngine` and all editor state for an interactive session.
It communicates with `bar-app` exclusively through a narrow set of
`take_*` / `set_*` methods — it never imports the engine, compute,
or render layers.

## Three-layer architecture

The crate has a deliberate three-layer split. Adding a new UI
surface or workspace mode lands in the layer it belongs to and
nowhere else.

```
crates/bar-gui/src/
  app.rs          # `BarEditorApp` (state). Single owner of every
                  # live editor field. Lifecycle methods
                  # (`apply_project`, `do_new_project`,
                  # `reset_session_state`, `start_with_macro`).
                  # Distributed `impl BarEditorApp` blocks (in
                  # other panel files) attach more methods.
  panels/
    mod.rs        # Panel registry.
    welcome.rs    # Empty-graph welcome screen with preset cards.
    palette.rs    # Node palette + drag-and-drop into canvas.
    properties.rs # Contextual properties popup + per-NodeType
                  # property bodies (Sculpt, PaintedHeightmap,
                  # PaintedTexture, PassThrough, group, etc.).
    inspector.rs  # 2D inspector window: heightmap backdrop,
                  # spawn markers, brush.
    mapinfo_editor.rs # Structured Map Info modal (6 tabs:
                  # Identity / Dimensions / Physics / Atmosphere /
                  # Lighting / Water).
    validation.rs # Sidebar summary + floating details window.
    dialogs.rs    # Settings, About (more on confirm dialogs below).
    node_canvas.rs # Node rendering, wires, ports, selection,
                  # marquee, group frames, collapsed subgraph
                  # blocks, canvas tabs, palette drop handling.
  layouts/
    mod.rs
    dispatch.rs   # `draw_active(app, ctx, frame)`: matches on
                  # `BarEditorApp::active_layout()` and delegates.
    standard.rs   # The today's-editor layout (top toolbar, left
                  # palette, centre canvas, side properties, etc.).
                  # New variants drop in as new files + one match
                  # arm in `dispatch.rs`.
```

`BarEditorApp` is the single owner. Panels are stateless. Layouts
compose panels.

## Encapsulated sub-structs

`BarEditorApp` holds a few cohesive sub-structs rather than
flattening every field. Each owns its invariants and is reset
together when its lifetime boundary is crossed.

| Sub-struct | What's in it | Reset by |
|---|---|---|
| `DialogState` | Modal/popup/transient flags: `show_settings`, `show_about`, `show_inspector`, `show_mapinfo_editor`, `show_validation_panel`, `show_map_info_picker`, `confirm_dialog`, `pending_action`, `file_editor`, `toast`, `status_message`, `pending_props_open`, `allow_close` | `reset_session_state` |
| `PaintSession` | Brush + sculpt-lock + per-layer paint caches: `brush`, `brush_stroking`, `sculpted`, `pending_sculpt_overlay_url`, `paint_brush_radius`, `heightmap`, `heightmap_rev`, `texture`, `texture_rev`, `color_buffer`, `metalmap`, `typemap`, `mask_textures`, `inspector_mode` | `PaintSession::invalidate_on_graph_reset` |
| `RecipeMeta` | Recipe identity: `shortname`, `description`, `author`, `version` | `reset_session_state` (cleared) / `apply_project` (loaded) |

When a new five-plus-field cluster appears in `BarEditorApp`, do
the same. Don't grow `BarEditorApp` past 30 top-level fields without
a structural reason.

## Layout enum

`Layout` is the persisted UI workspace selection (today: only
`Standard`; future: `SculptFocus`, `ExportOnly`, etc.). The choice
lives on `BarEditorApp::active_layout` and is mirrored in
`Settings::active_layout` so it survives restart.

Layouts are **pure UI/UX**. The same `BarEditorApp` underlies every
layout; switching is instant and never migrates project state.

To add a layout:

1. Add a variant to the `Layout` enum in `app.rs`.
2. Create `crates/bar-gui/src/layouts/<name>.rs` exposing
   `pub fn draw(app: &mut BarEditorApp, ctx: &egui::Context, frame: &mut eframe::Frame)`.
3. Register it in `layouts::mod`.
4. Add a match arm in `layouts::dispatch::draw_active`.
5. Add a UI affordance for switching (likely a top-toolbar
   dropdown). Multi-layout switching UI lands once there's a
   second layout — there's no point in a one-item dropdown.

A layout reads any `BarEditorApp` field but only calls into
`crate::panels::*` for actual rendering. Panels are reusable across
layouts; never duplicate panel logic in a layout module.

## Panel extraction cookbook

When a chunk of inline UI in a panel file (or in
`update_panels`) gets large enough to deserve its own file,
extract it. Pattern:

1. **New file** under `panels/<name>.rs` with module-level `//!`
   docs explaining what the panel renders.
2. **Stateless entry point.** Either
   - `pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context)`
     for top-level windows / panels, or
   - `pub(crate) fn draw(app: &mut BarEditorApp, ui: &mut egui::Ui)`
     for panels that render into a parent `Ui`.
3. **Field access via `pub(crate)` accessors** on `BarEditorApp`
   when the field is private. Add the accessors to `app.rs` near
   the existing accessor block.
4. **Distributed `impl BarEditorApp` blocks** are also fine when
   the panel needs heavy `&mut self` access (e.g. `properties.rs`,
   `node_canvas.rs`). The block lives in the panel file, methods
   are `pub(crate) fn`, fields they touch are `pub(crate)`.
5. **Replace the call site** in `app.rs` (typically inside
   `update_panels` or another existing method) with a single
   `crate::panels::<name>::draw(self, ...)` call.
6. **Run `cargo test --workspace`.** The suite is the contract.

The seven existing panels are working templates. `welcome.rs` and
`dialogs.rs` are the simplest references; `properties.rs` and
`node_canvas.rs` show the distributed `impl` pattern at full scale.

Confirm-dialog and unsaved-changes prompts intentionally stay in
`app.rs`'s `update_panels` because they're tightly coupled to the
`PendingAction` flow control. Don't try to extract those without
restructuring the action queue first.

## State ownership

`BarEditorApp` holds:
- `graph: GraphEngine` — the live DAG.
- `node_visuals: HashMap<NodeId, NodeVisual>` — pixel positions
  and sizes per node on the canvas.
- `groups: HashMap<u64, GroupRuntime>`, `node_to_group`,
  `next_group_id` — visual groups (subgraphs and macros).
- `tabs: Vec<CanvasView>`, `active_tab`, `last_active_tab` — open
  canvas tabs (Main + drilldowns).
- `selected_node`, `selected_nodes`, `selected_group`,
  `selected_connection` — mutually-exclusive selection state.
- `history: UndoHistory` — bounded snapshot stack.
- `dialog: DialogState` — see above.
- `paint: PaintSession` — see above.
- `recipe_meta: RecipeMeta` — see above.
- `map_settings: MapSettings` — single source of truth for
  identity-adjacent fields (start_positions lives directly here,
  not in a shadow vec).
- `map_width`, `map_height`, `map_min_height`, `map_max_height` —
  Spring elmo dimensions; `map_min/max_height` shadow the
  `MapSettings` value because the egui DragValue widgets bind to
  them and the structured editor commits on save.
- `project_path`, `loaded_name`, `is_dirty`, `map_info_file` —
  project metadata.
- `settings: Settings` — persistent user prefs.
- `active_layout: Layout` — current UI composition.
- Async receivers: `pending_open_rx`, `sd7_open_request` — see
  the async pattern below.
- Request flags: `run_requested`, `test_in_bar_requested`,
  `run_bundler_node`, `graph_reset` — single-frame pulses polled
  by `bar-app`.

`bar-gui` does **not** own GPU resources, render pipelines, or
background thread handles other than the brief file-dialog and
SD7-extract workers.

## Async I/O pattern

The egui main thread must not block. Native file dialogs (`rfd`)
and long-running compute (export, SD7 extract, graph evaluation)
all spawn worker threads and return results via `mpsc::channel`.

Established sites:

- `BarEditorApp::open_file_dialog_async` — spawns the OS file
  picker; result polled via `pending_open_rx` in
  `update_panels`.
- `AppWrapper::pending_export_dir` — same pattern for the export
  folder picker, two-phase: dialog spawn → folder result triggers
  the export thread.
- Preview eval pipeline (in `bar-app`) — the cache-key + session-id
  gate around `eval_preview` rejects stale results.

Never call `pick_file` / `pick_folder` / `evaluate_graph` /
`extract_sd7_to_work_dir` directly from `update`. Spawn, poll,
guard against staleness.

## Reset session state

Every project switch (new, open, preset macro) clears transient
state through one funnel:

```
do_new_project → reset_session_state → ...
apply_project (file load) → reset_session_state → ...
start_with_macro (preset) → reset_session_state → ...
```

`reset_session_state` clears history, dialog flags, validation
cache, canvas pan, palette drag, dialogs/modals, run/test
request flags, tabs. `PaintSession::invalidate_on_graph_reset`
is called from inside `reset_session_state` — paint state is
session-only.

When you add a new transient field to `BarEditorApp`, decide
whether it crosses a project boundary and update
`reset_session_state` if it shouldn't survive. The next bug
report will be "I loaded a new project and the old project's
[thing] was still there" if you skip this.

## Visibility convention

- `BarEditorApp` and its sub-structs (`DialogState`,
  `PaintSession`, `RecipeMeta`): fields are `pub(crate)` so panel
  modules can access them. The `BarEditorApp::dialog` /
  `paint` / `recipe_meta` fields themselves are private; access
  goes through `pub(crate)` accessors on `BarEditorApp`.
- Panel render functions: `pub(crate)`.
- Per-panel free helpers (drawing primitives, layout maths): keep
  private to the panel module.
- The `pub` API of the crate stays narrow. `bar-app` and `bar-cli`
  are the only outside consumers; their interface is the small
  set of `take_*` / `set_*` methods.

## Localization (i18n)

User-facing strings go through `t!()` from `crate::i18n`, which
looks up keys against a catalogue built from the embedded
`language/` tree. The layout matches `bar-localizations` so files
round-trip without a format transform — see `language/README.md`
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
return the literal key string at runtime; if you see
`"editor.menu.file"` rendered as text, the catalogue doesn't
have it.

## Design Tokens — `panels/tokens.rs`

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
| Misc | — | `ICON_STROKE`, `CANVAS_BG` |

**Rules:**
- Before adding a `Color32::from_rgb(...)` anywhere in `bar-gui`,
  ask whether the color is already represented in tokens. If yes,
  use the token. If no, add the token first.
- Colors that appear in more than one panel — especially those that
  pair visually (e.g. validation severity in the sidebar summary AND
  the details window) — **must** share a token so they can't drift.
- The prior drift incident: `validation.rs` sidebar used
  `(220, 80, 80)` for errors, the details window used
  `(220, 100, 100)` — a bug invisible in code review, only visible
  side-by-side. Tokens make this class of bug impossible.

## Icon Functions — `panels/icons.rs`

All toolbar and canvas icon paint functions live in
`crates/bar-gui/src/panels/icons.rs`. The file is registered in
`panels/mod.rs` as `pub mod icons` and re-exported from `app.rs`:

```rust
// in app.rs
pub(crate) use crate::panels::icons::{
    draw_io_icon, paint_bar_icon, paint_busy_dot, paint_export_icon,
    paint_inspector_icon, paint_map_info_icon, paint_mapinfo_form_icon,
    paint_startbox_icon, paint_validate_icon,
};
```

The re-export means `node_canvas.rs` (which does `use crate::app::*`)
continues to find icon functions without changes to call sites.

**Rules:**
- Never add a new `paint_*` or `draw_*_icon` function to `app.rs`.
  New icons go in `icons.rs`, added to the re-export in `app.rs`.
- All icon functions take `(painter: &egui::Painter, rect: egui::Rect,
  color: egui::Color32)` — caller supplies tint so the same icon
  renders at hover/active/normal without branching inside the function.
- Icon stroke color defaults to `tokens::ICON_STROKE`; use that
  rather than a hardcoded RGB when the icon is part of a panel (not
  caller-tinted). The `draw_io_icon` function does this.
- Icon geometry is defined in logical coordinates relative to the
  supplied `rect` — never absolute screen coordinates.

## Node Canvas Style — `NodeStyle`

All node body geometry and color constants are grouped in the
`NodeStyle` struct defined at the top of `panels/node_canvas.rs`.
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
  `let ns = NodeStyle::default()` — never via magic literal floats or
  inline `Color32` constructions.
- To change the look of all nodes, change `NodeStyle::default()` or
  the token it reads. To give one node type a different look, either
  call a variant constructor or override individual fields after
  construction.
- Do not add new magic number sizing constants inside node draw loops.
  Put them in `NodeStyle` (style concerns) or the existing layout
  constants (`PORT_Y_BASE`, `PORT_Y_STEP`) at the top of
  `node_canvas.rs` (geometric concerns).

## Properties Panel — Layout Patterns

### Two-column label + widget layout

The generic parameter editor in `properties.rs` uses `egui::Grid`
for label/widget alignment. This is the required pattern wherever
a scrollable list of `label | widget` rows appears:

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

`egui::Grid` is the egui-native answer to aligned columns. Do not
simulate a two-column layout with `ui.horizontal` and fixed-width
`allocate_exact_size` — that produces fragile pixel math that breaks
at non-default DPI or when labels differ in length.

### `Bool` rows in a Grid

Bool params sit in a Grid cell using an empty first column so the
checkbox lines up with other widgets:

```rust
ui.label("");          // column 1 — placeholder for alignment
ui.checkbox(&mut val, key);  // column 2 — label is inside the checkbox
ui.end_row();
```

### `TextEdit` width

Use `desired_width(f32::INFINITY)` on `TextEdit` in Grid column 2 so
it fills the available cell width:

```rust
ui.add(egui::TextEdit::singleline(&mut val).desired_width(f32::INFINITY))
```

The fixed `desired_width(120.0)` that existed before the refactor
caused the TextEdit to be narrower than the DragValue column and
made the layout look jagged.

### `DragValue` width — egui 0.31 limitation

`egui::DragValue` in egui **0.31** does **not** have a `desired_width`
method. Do not attempt to call `.desired_width(...)` on a `DragValue`
— it will not compile. Column alignment in a Grid is sufficient for
DragValues; they use their default interact-size width which is
consistent across rows. If you need a wider DragValue, use
`ui.add_sized([width, height], DragValue::new(...))`.

### `ComboBox` in a Grid

`ComboBox::from_id_salt(id).selected_text(...).show_ui(ui, |ui| { ... })`
can appear directly as column 2 of a Grid row. The dropdown button
occupies the column naturally. No explicit `.width(...)` call is needed
unless the default `combo_width` from spacing is wrong for the context.

### Text + Browse button in one Grid cell

When column 2 contains both a `TextEdit` and an adjacent action
button (e.g. the file-path `"…"` browse button), wrap them in a
`ui.horizontal`:

```rust
ui.label(key);
ui.horizontal(|ui| {
    if ui.add(egui::TextEdit::singleline(&mut val)
        .desired_width(f32::INFINITY)).changed() { ... }
    if ui.button("…").clicked() { ... }
});
ui.end_row();
```

## Node Layout Constants

```
PORT_Y_BASE = 30.0    // first port Y offset below node top
PORT_Y_STEP = 20.0    // vertical spacing between ports
IO_NODE_SIZE = (160, 52)   // SubgraphInput / SubgraphOutput pill
```

Terminal node specifics:
- **Bundler** (export sink): title bar 20 px; Export button
  top-right, 44 × 36 px, matches the toolbar Run button style.
  No footer — Bundler is for export only and never drives the 3D
  viewport.
- **Preview** (3D viewport sink): "Open" footer full-width,
  22 px, rounded bottom corners, teal — clicking it opens or
  re-targets the 3D viewport. The Preview node has explicit
  inputs (`heightmap` required; `texture` / `normal_map` /
  `specular_map` optional) and is the sole driver of the
  viewport.
- **SubgraphInput / SubgraphOutput**: render as a pill (no
  header) with a directional chevron, an arrow icon, and a
  two-line "Input"/"Output" + type label. See
  `panels/node_canvas.rs::draw_io_icon` for the icon geometry.

## UI Interaction Rules

- Clicking the Export button on a Bundler or the "Open" footer
  on a Preview must not select the node. Register the button
  interactions before the node background `interact` call so
  they have priority.
- `egui::TopBottomPanel::bottom` must be declared **before**
  `SidePanel::left/right` for correct sizing.
- The properties panel header shows the node label as
  `ui.heading()` with the type name below — do not add a
  redundant "Properties" title. SubgraphInput/SubgraphOutput
  nodes specifically suppress the title and type display.
- `push_undo` must be called **after** (not before) the graph
  mutation it describes. Never call it before replacing the
  graph (e.g. on project load) — that would insert the previous
  graph as undo entry #1 of the new session.

## Boundaries — What This Crate Must NOT Do

- Must not depend on `bar-compute`, `bar-engine`, `bar-render`,
  `bar-app`, or `bar-cli`.
- Must not create wgpu resources or submit GPU work.
- Must not spawn long-running threads beyond the brief
  file-dialog / SD7-extract workers (preview eval workers live
  in `bar-app`).
- Must not call `bar-engine` export functions directly — signal
  `bar-app` via `take_run_requested` / `take_run_bundler_node`
  and let the app layer handle execution.
- Must not modify `GraphEngine` revision counter logic
  directly — use the `GraphEngine` mutation API which increments
  the revision.

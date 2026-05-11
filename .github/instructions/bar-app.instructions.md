---
applyTo: "crates/bar-app/**"
---

# bar-app — Application Shell & Session Lifecycle

## Role
`bar-app` is the root binary (`bar-map-editor`). It owns the eframe event loop,
wires every other crate together, and enforces strict per-project lifetime
boundaries via the `Session` struct. Nothing above `bar-app` exists; it exposes
nothing to other crates.

## Responsibilities
- **Entry point**: initialise `tracing`, install the i18n catalogue
  (`bar_gui::i18n::init()` — call this once before any UI code runs;
  see `crates/bar-gui/src/i18n.rs` and the i18n section of
  `bar-gui.instructions.md`), create the eframe window (1440 × 900),
  attach to eframe's wgpu device/queue, build `GpuContext`, and
  instantiate `HybridExecutor` (GPU) or `CpuExecutor` (no GPU) as
  `Arc<dyn NodeExecutor>`.
- **Frame loop** (`AppWrapper::update`): call `OpenMachineApp::update` (the GUI),
  poll the relevant sub-state pulses (`app.preview.take_run_requested()`,
  `app.preview.take_run_bundler_node()`, `app.preview.take_test_in_bar()`,
  `app.project.take_graph_reset()`, `app.project.sd7_open_request.take()`),
  and dispatch to the appropriate async path.
- **Per-project isolation** via `Session`: all render state for a project
  (`TerrainRenderer`, `Camera`, preview channels, revision counters,
  `session_id`) lives inside `Option<Session>`. Opening a new project replaces
  `self.session` with a fresh `Session::new()`; Rust drop semantics free all
  GPU buffers, old channels, and camera state atomically -- no manual field
  enumeration.
- **Two-pass progressive preview**: when the graph revision changes, immediately
  spawn a low-res (128 px) background thread for fast visual feedback, then
  after a 300 ms cooldown spawn a high-res (512 px) thread for the refined
  result. Both passes share a single `mpsc::channel`. A small loading spinner
  is overlaid in the viewport's bottom-right corner while the high-res pass is
  pending. Results are discarded if `session_id` or `revision` no longer match.
- **Dynamic viewport resize**: the renderer's GPU textures are recreated every
  frame to match the available viewport area (pixel-exact, no stretch/distortion).
- **Export**: spawn a background thread that calls `evaluate_graph` ->
  `execute_bundlers`; poll `export_result_rx` each frame and surface the result
  string via `app.set_status(...)`.
- **SD7 extraction**: spawn a background thread calling
  `extract_sd7_to_work_dir`; return `WorkDirScan` to
  `OpenMachineApp::finish_open_map`.

## Per-Project Session Lifecycle

```
New project / Open project / Open SD7
          │
          ▼
  self.session = Some(Session::new(next_session_id, gpu_context))
          │
          │  Old Session dropped here — GPU buffers freed, old channel
          │  sender dropped (any in-flight thread's send() silently fails)
          ▼
  AppWrapper::update polls preview_rx for PreviewResult
          │
          │  Guard: result.session_id == session.session_id
          │         result.revision   == current_revision
          │  (cross-session or superseded results are structurally discarded)
          ▼
  TerrainRenderer::update_mesh_lod + update_texture
  → register output_view with egui
```

The `graph_reset` flag (consumed via `app.project.take_graph_reset()`)
triggers `self.session = Some(Session::new(...))` at the top of the next
frame.

## Key Internal Types
| Type | Description |
|---|---|
| `AppWrapper` | `eframe::App` impl; owns executor + GPU infrastructure + `Option<Session>` |
| `Session` | Per-project render state; created fresh for every project |
| `PreviewResult` | Thread payload — see fields below |

`Session` fields:
- `terrain_renderer: Option<TerrainRenderer>`
- `camera: Camera` (always `Camera::default()` on `Session::new`)
- `viewport_texture_id: Option<egui::TextureId>`
- `last_low_res_revision: u64` — revision of last applied 128 px pass (starts `u64::MAX`)
- `last_high_res_revision: u64` — revision of last applied 512 px pass (starts `u64::MAX`)
- `low_res_pending: bool` — true while 128 px thread is in flight
- `high_res_pending: bool` — true while 512 px thread is in flight
- `low_res_completed_at: Option<Instant>` — timestamp gates 300 ms cooldown before high-res
- `preview_tx: mpsc::Sender<PreviewResult>`, `preview_rx: mpsc::Receiver<PreviewResult>`
- `session_id: u64` — monotonically increasing per `Session::new` call

`PreviewResult` fields:
- `heightmap: Option<Heightmap>`, `texture: Option<ColorBuffer>`
- `revision: u64`, `session_id: u64`
- `height_scale: f32` -- computed per-frame from `app.map.height_range()` (see formula below)
- `water_y: f32` — render-space Y of the water surface (negative = no water)
- `water_color: [f32; 3]`
- `is_low_res: bool` — distinguishes the two passes
- `x_extent: f32`, `z_extent: f32` — physical aspect ratio for non-square maps

## Height Scale Formula
Heightmap data is always normalised `[0.0, 1.0]`; `map_min_height` / `map_max_height` are in Spring elmos. The formula matches the BAR website's visual scale (1.6× intentional vertical exaggeration):
```
pw = (map_width  - 1).max(1)
ph = (map_height - 1).max(1)
pm = pw.max(ph)
x_extent     = (0.5 * pw / pm).min(0.5)
z_extent     = (0.5 * ph / pm).min(0.5)
height_scale = (max_h - min_h).abs() * 0.2 / pm
```
`water_y = (-min_h / height_range) * height_scale` when `min_h < 0`; otherwise `-1.0` (sentinel).

## Interaction Surface
**Calls into:**
- `bar-gui::OpenMachineApp::update` — drives the UI each frame
- `bar-graph::evaluate_graph` — in background threads
- `bar-engine::execute_bundlers`, `extract_sd7_to_work_dir`, `HybridExecutor`/`CpuExecutor`
- `bar-render::TerrainRenderer` — mesh/texture updates + render
- `bar-compute::GpuContext::from_existing` — attaches to eframe's device

**Exposes:** Nothing — this is the terminal binary.

## Invariants to Maintain
- **One session per project, always.** Never partially reset session state by
  clearing individual fields. Replace the entire `Option<Session>`.
- **Cross-session + cross-revision guard.** Always compare both
  `result.session_id == session.session_id` AND `result.revision == current_revision`
  before applying a `PreviewResult`. The revision counter resets to 0 on every new
  `GraphEngine`; `session_id` is the only globally unique identifier across projects.
- **Always clear the in-flight flag** (`low_res_pending` / `high_res_pending`) on
  receipt, even for stale results, to prevent deadlock waiting for a thread that
  has already exited.
- **No UI on background threads.** `egui::Context::request_repaint()` may be
  called from a thread, but no egui widgets may be created off the main thread.
- **Executor is `Arc`-shared, never cloned structurally.** Clone the `Arc`
  (cheap ref-count bump) into each thread; do not create a second executor.
- **`AppWrapper` infrastructure fields** (`executor`, `gpu_context`,
  `render_state`) are created once at startup and never replaced. They are
  distinct from session state.
- **Mesh LOD is capped** at `(renderer.width.min(renderer.height) / 2).clamp(128, 512)`
  so triangles stay ≥ 2 pixels wide on screen and don't shimmer from sub-pixel aliasing.

## Boundaries — What This Crate Must NOT Do
- Must not define any domain types (graph nodes, project formats, pixel
  buffers) — those all live in the crates below.
- Must not perform graph mutations -- signal `bar-gui` to mutate and pick up
  the result via the sub-state `take_*` accessors (`app.preview.take_*`,
  `app.project.take_*`).
- Must not block the main thread with heavy computation — all evaluation and
  export must happen on background threads.

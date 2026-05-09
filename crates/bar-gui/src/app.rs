use bar_graph::{GraphEngine, NodeId, NodeType, PortPlacement};
use eframe::egui;
use std::collections::HashMap;

use crate::settings::Settings;
use crate::undo::UndoHistory;

// Re-export icon painters so that `use crate::app::*` in panel modules keeps
// finding them after they moved to panels/icons.rs.
pub(crate) use crate::panels::icons::{
    draw_io_icon, paint_bar_icon, paint_busy_dot, paint_export_icon, paint_inspector_icon,
    paint_map_info_icon, paint_mapinfo_form_icon, paint_startbox_icon,
};

// Welcome-panel template list lives in `panels::welcome` now.

/// Port layout constants -- shared between rendering and wire drawing.
/// Title bar is 20 px, inset 8 px from the top edge; ports stack below it.
pub(crate) const PORT_Y_BASE: f32 = 38.0;
pub(crate) const PORT_Y_STEP: f32 = 20.0;
pub(crate) const TITLE_Y_OFFSET: f32 = 8.0;
pub(crate) const TOP_PORT_INSET: f32 = 28.0;
pub(crate) const TOP_PORT_STEP: f32 = 22.0;

/// Default visual size for `SubgraphInput` / `SubgraphOutput` nodes.
/// All other dimensions (chevron width, corner radius, icon size,
/// padding, text size) are computed at render time as proportions
/// of the node's actual height; the user can resize the node from
/// the corner anchors and every feature scales together.
pub(crate) const IO_NODE_SIZE: egui::Vec2 = egui::vec2(160.0, 52.0);

/// Reference height the proportional dimensions are calibrated
/// against. Heights other than this rescale every feature in
/// lock-step.
pub(crate) const IO_REF_H: f32 = 52.0;

/// Screen position of a node port. Replaces the old scalar `node_port_y`.
/// For IO nodes the port is always centered on the appropriate edge
/// regardless of placement or index. For regular nodes the position
/// depends on the placement kind:
///   Left / Right -- stacked at PORT_Y_BASE + side_index * PORT_Y_STEP
///   Top(slot)    -- fixed X slot on the top edge (centered vertically)
///   Bottom       -- centered on the bottom edge
pub(crate) fn node_port_pos(
    node_type: &NodeType,
    node_rect: egui::Rect,
    placement: PortPlacement,
    side_index: usize,
) -> egui::Pos2 {
    if matches!(
        node_type,
        NodeType::SubgraphInput | NodeType::SubgraphOutput
    ) {
        let x = match placement {
            PortPlacement::Right => node_rect.max.x,
            _ => node_rect.min.x,
        };
        return egui::pos2(x, node_rect.center().y);
    }
    match placement {
        PortPlacement::Left => egui::pos2(
            node_rect.min.x,
            node_rect.min.y + PORT_Y_BASE + side_index as f32 * PORT_Y_STEP,
        ),
        PortPlacement::Right => egui::pos2(
            node_rect.max.x,
            node_rect.min.y + PORT_Y_BASE + side_index as f32 * PORT_Y_STEP,
        ),
        PortPlacement::Top(slot) => egui::pos2(
            node_rect.min.x + TOP_PORT_INSET + slot as f32 * TOP_PORT_STEP,
            node_rect.min.y,
        ),
        PortPlacement::Bottom => egui::pos2(node_rect.center().x, node_rect.max.y),
    }
}

// `NodeVisual` and `GroupRuntime` live in `crate::state` so the undo
// module can snapshot them without going through stringly-typed JSON.
// Imported at the top of this file.

/// Parse a persisted `"<node_key>:<port_name>"` binding into runtime
/// `(NodeId, port_name)`. Returns `None` if the binding is missing,
/// malformed, or refers to a node key that didn't load.
pub(crate) fn parse_subgraph_binding(
    raw: Option<&str>,
    key_to_id: &HashMap<String, NodeId>,
) -> Option<(NodeId, String)> {
    let s = raw?;
    let (key, port_name) = s.split_once(':')?;

    let id = *key_to_id.get(key)?;
    Some((id, port_name.to_string()))
}

/// One-shot context-menu action carried out after the menu closes,
/// since the menu closure can't borrow `self` mutably while iterating
/// `self.visuals.groups`.
pub(crate) use crate::editor::{CanvasView, MapInfoTab, ValidationFilter};

pub(crate) enum GroupOp {
    CreateWith(NodeId),
    CreateFromSelection(Vec<NodeId>),
    AddTo(NodeId, u64),
    AddManyTo(Vec<NodeId>, u64),
    RemoveFrom(NodeId),
}

/// Fixed palette of subtle tints for group rectangles. Members store
/// an index into this so the colour serialises as a single u8.
pub(crate) const GROUP_PALETTE: &[(u8, u8, u8)] = &[
    (90, 110, 150), // slate blue
    (110, 130, 90), // moss
    (150, 110, 90), // terracotta
    (130, 90, 130), // mauve
    (90, 140, 130), // teal-grey
    (140, 130, 70), // ochre
];

pub(crate) fn group_color(idx: u8) -> egui::Color32 {
    let (r, g, b) = GROUP_PALETTE[idx as usize % GROUP_PALETTE.len()];
    egui::Color32::from_rgb(r, g, b)
}

/// Linear interpolation between two RGB colours. `t = 0.0` returns
/// `a`, `t = 1.0` returns `b`. Used to mix a SubGraph's identity
/// tint with the neutral tab background so tabs read as both
/// "tab-like" and "this group's tab".
pub(crate) fn blend(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round() as u8;
    egui::Color32::from_rgb(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()))
}

pub(crate) use crate::dialog::{
    confirm_key_display_name, ConfirmAction, ConfirmDialog, DialogState, FileEditor,
    GroupDeleteChoice, PassthroughEdit, PendingAction, UnsavedDecision,
    CONFIRM_KEY_DELETE_CONNECTED_NODE,
};
pub(crate) use crate::editor::DragConnection;

/// What kind of palette item is being dragged. Regular node types
/// drop as a single node; macros drop as a SubGraph block plus a
/// freshly-instantiated cluster of inner nodes.
#[derive(Clone, Debug)]
pub enum PaletteKind {
    Node(NodeType),
    Macro {
        /// Canonical full name of one of the entries in `macros::BUILTIN_MACRO_GROUPS`.
        name: String,
    },
}

/// In-flight drag from the node palette onto the canvas.
#[derive(Clone, Debug)]
pub(crate) struct PaletteDrag {
    pub kind: PaletteKind,
    pub label: String,
}

pub use crate::paint::{
    BrushState, BrushTarget, BrushTool, InspectorMode, PaintSession, SculptState,
};

pub use crate::editor::SmfLightingSnapshot;

pub(crate) use crate::editor::{
    PendingPropsOpen, PropsTarget, PROPS_OPEN_DELAY_MS, PROPS_OPEN_MOVE_TOLERANCE,
};

pub use crate::editor::ExportStatus;

/// Top-level UI composition the user sees. Each variant maps to
/// one file in `crate::layouts::*` that decides which panels are
/// visible and how they're arranged. The active layout is purely a
/// UI/UX concern — the underlying `BarEditorApp` state (graph,
/// paint session, project, undo) is identical across layouts, so
/// switching is instant and never migrates data.
///
/// Today only `Standard` exists; future layouts (a sculpt-focus
/// view that hides the node graph, an export-only view that shows
/// just validation + bundler controls, etc.) drop in as new files
/// + a match arm in `layouts::dispatch::draw_active`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Layout {
    /// Today's editor: top toolbar, left palette, centre canvas,
    /// right contextual properties, bottom status bar, with
    /// floating windows for inspector / map info / validation /
    /// settings / about.
    #[default]
    Standard,
    /// Full-width 3D viewport with brush controls on the right.
    /// Writes brush strokes to `SculptState` directly; the
    /// export pipeline merges them onto graph output at bundle
    /// time.
    Sculpt3D,
}

pub(crate) use crate::editor::RecipeMeta;

/// Main application state for the BAR - Map Editor GUI.
pub struct BarEditorApp {
    pub(crate) graph: GraphEngine,
    /// Visual presentation: node positions, groups, group/node
    /// reverse index, monotonic group id allocator, and per-frame
    /// hit-test rect caches. See `editor::VisualsState`.
    pub visuals: crate::editor::VisualsState,
    /// Canvas selection: primary node, multi-selection, group,
    /// connection, and any group queued for deletion. See
    /// `editor::SelectionState`. (`selected_node` from the old layout
    /// is now `selection.node`.)
    pub selection: crate::editor::SelectionState,
    /// Canvas viewport + interaction: pan offset, open tabs, marquee
    /// anchor, in-progress drag connection. See `editor::CanvasState`.
    pub canvas: crate::editor::CanvasState,
    /// Map metadata: dimensions, height range, MapSettings,
    /// RecipeMeta identity, and the spawn-marker drag pointer. See
    /// `editor::MapState`.
    pub map: crate::editor::MapState,
    /// Undo/redo history.
    pub(crate) history: UndoHistory,
    /// Project lifecycle state: file path, dirty flag, autosave timer,
    /// SD7 extraction handoff, file-dialog poll receiver, inline file
    /// editor state, graph-reset pulse, and map-info file pointer.
    /// See `project::ProjectState`.
    pub project: crate::project::ProjectState,
    /// Raw window + display handles of the editor's main window. Used to
    /// parent native file dialogs so they belong to (and return focus
    /// to) the editor instead of whatever window the OS happens to
    /// consider foreground at the moment the dialog spawns. `bar-app`
    /// populates this each frame from `eframe::Frame`.
    pub(crate) parent_window_handles: Option<(
        raw_window_handle::RawWindowHandle,
        raw_window_handle::RawDisplayHandle,
    )>,
    /// Preview / export concern: viewport open flag, driving node, and
    /// the one-frame "run" / "test in BAR" / "run this bundler" pulses
    /// that `bar-app` polls each frame. See `editor::PreviewState`.
    pub preview: crate::editor::PreviewState,
    /// Modal / popup / transient-feedback flags. See `DialogState`.
    pub dialog: DialogState,
    /// Cached list of validation findings displayed in the panel. Built
    /// Validation cache: findings list, severity filter, mapinfo-modal
    /// tab, and the input fingerprint that gates re-validation. See
    /// `editor::ValidationState`.
    pub validation: crate::editor::ValidationState,
    // groups, node_to_group, next_group_id, group_header_rects,
    // group_body_rects, and collapsed_subgraph_rects moved to
    // `self.visuals` (see `editor::VisualsState`).
    // selected_nodes / selected_group / pending_group_delete /
    // selected_connection moved to `self.selection`
    // (see `editor::SelectionState`).
    // marquee_start, tabs, active_tab, last_active_tab,
    // canvas_rect_last, and pending_auto_layout_all moved to
    // `self.canvas` (see `editor::CanvasState`).
    /// Floating properties popup state: target binding and last-known
    /// on-screen rect. See `editor::PropsPanelState`.
    pub props: crate::editor::PropsPanelState,
    /// Brush, sculpt-lock, and per-layer paint caches. See
    /// `PaintSession`.
    pub paint: PaintSession,
    /// In-flight drag from the node palette (set when pointer starts dragging an item,
    /// cleared on pointer release — either creating a node or cancelling).
    pub(crate) palette_drag: Option<PaletteDrag>,
    /// Persistent user preferences (recent files, autosave config, vertical
    /// exaggeration, etc.).
    pub(crate) settings: Settings,
    // last_autosave_at and map_info_file moved to `self.project`.
    /// Active top-level UI layout (`Layout::Standard` today). Pure
    /// UI/UX concern; switching layouts never migrates data. Loaded
    /// from settings on launch, persisted via `set_active_layout`.
    pub(crate) active_layout: Layout,
}

impl Default for BarEditorApp {
    fn default() -> Self {
        Self {
            graph: GraphEngine::new(),
            visuals: crate::editor::VisualsState {
                next_group_id: 1,
                ..Default::default()
            },
            selection: crate::editor::SelectionState::default(),
            canvas: crate::editor::CanvasState::default(),
            map: crate::editor::MapState {
                width: 256,
                height: 256,
                min_height: 0.0,
                max_height: 800.0,
                ..Default::default()
            },
            history: UndoHistory::default(),
            project: crate::project::ProjectState::default(),
            parent_window_handles: None,
            preview: crate::editor::PreviewState::default(),
            dialog: DialogState::default(),
            validation: crate::editor::ValidationState::default(),
            props: crate::editor::PropsPanelState::default(),
            paint: PaintSession::default(),
            palette_drag: None,
            settings: Settings::default(),
            active_layout: Layout::default(),
        }
    }
}

impl BarEditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Extend egui's default font fallbacks with whatever broad-
        // coverage symbol font the host OS ships. Without this the
        // editor falls back to grey "missing glyph" boxes for things
        // like bullets, arrows, and the multiplication sign in
        // dimension labels.
        crate::state::install_system_symbol_font(&cc.egui_ctx);
        let mut app = Self::default();
        app.settings = Settings::load();
        // Restore the layout the user last had selected, falling
        // back to `Default` when settings are absent or pre-date
        // the field.
        app.active_layout = app.settings.active_layout;
        // Drop recents that no longer exist on disk so the menu stays useful.
        app.settings.recent_files.retain(|p| p.exists());
        // Reopen the most-recently-loaded project on launch so the
        // user picks up where they left off. Skipped when the user
        // turned the preference off, when there are no recent files,
        // or when the most recent file no longer exists. Errors are
        // surfaced as a status message; we don't crash the launch.
        if app.settings.restore_last_project {
            if let Some(last) = app.settings.recent_files.first().cloned() {
                if last.exists() {
                    app.load_project(last);
                }
            }
        }
        app
    }

    pub fn graph(&self) -> &GraphEngine {
        &self.graph
    }

    // `set_parent_window_handles`, `parent_window`, `make_dialog`, and
    // `ParentWindow` itself live in `crate::io::dialogs` (distributed
    // `impl BarEditorApp` block).
}

impl BarEditorApp {
    /// Read-only access to user settings (vertical exaggeration, etc.).
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// True when the user is currently looking at a subgraph tab —
    /// the palette uses this to gate the "SubGraph IO" group so
    /// `SubgraphInput`/`SubgraphOutput` can't be dropped at the
    /// top level by accident.
    pub(crate) fn is_in_subgraph_view(&self) -> bool {
        matches!(
            self.canvas.tabs.get(self.canvas.active_tab),
            Some(CanvasView::SubGraph(_))
        )
    }

    /// Set the in-flight palette drag — populated when the user
    /// starts dragging a palette item; consumed by the canvas drop
    /// handler.
    pub(crate) fn set_palette_drag(&mut self, drag: PaletteDrag) {
        self.palette_drag = Some(drag);
    }

    /// Mutable access to the recipe-identity meta block — the
    /// mapinfo editor's Identity tab binds egui text fields
    /// directly into these.
    pub(crate) fn recipe_meta_mut(&mut self) -> &mut RecipeMeta {
        self.map.recipe_meta_mut()
    }
    /// Mutable access to MapSettings — binds the Physics /
    /// Atmosphere / Lighting / Water tabs.
    pub(crate) fn map_settings_mut(&mut self) -> &mut bar_project::MapSettings {
        self.map.settings_mut()
    }
    pub(crate) fn map_dimensions_mut(&mut self) -> (&mut u32, &mut u32) {
        self.map.dimensions_mut()
    }
    pub(crate) fn map_height_range_mut(&mut self) -> (&mut f32, &mut f32) {
        self.map.height_range_mut()
    }
    pub(crate) fn mapinfo_tab_now(&self) -> MapInfoTab {
        self.validation.mapinfo_tab()
    }
    pub(crate) fn set_mapinfo_tab(&mut self, tab: MapInfoTab) {
        self.validation.set_mapinfo_tab(tab);
    }

    /// Mutable access to the live paint session — brush, sculpt
    /// lock, and per-layer paint caches. The 2D inspector and
    /// properties panel both need to mutate brush state and
    /// inspector caches.
    pub(crate) fn paint_mut(&mut self) -> &mut PaintSession {
        &mut self.paint
    }
    pub(crate) fn paint(&self) -> &PaintSession {
        &self.paint
    }

    /// Spawn-marker drag-index accessor used by the inspector's
    /// drag-and-drop handling.
    pub(crate) fn dragging_spawn(&self) -> Option<usize> {
        self.map.dragging_spawn
    }
    pub(crate) fn set_dragging_spawn(&mut self, idx: Option<usize>) {
        self.map.dragging_spawn = idx;
    }

    /// Mark the project as dirty (unsaved changes pending).
    pub(crate) fn mark_dirty(&mut self) {
        self.project.is_dirty = true;
    }

    /// Status-bar message setter used by panels that need to
    /// surface a result without going through the toast path
    /// (e.g. "Sculpt saved to /path/to/foo.png" after the user
    /// triggers a save dialog inside the inspector).
    pub(crate) fn set_status_message(&mut self, msg: String) {
        self.dialog.status_message = Some(msg);
    }

    /// True if the project has unsaved changes since the last save/load.
    pub fn is_dirty(&self) -> bool {
        self.project.is_dirty()
    }

    /// True for one frame after the user has acknowledged the unsaved-changes
    /// dialog and confirmed a close (or there were no changes to discard).
    /// `bar-app` polls this each frame; `true` means the OS close request can
    /// be allowed to proceed. Also persists settings on the way out so window
    /// state, recents, and prefs survive a clean shutdown.
    pub fn take_allow_close(&mut self) -> bool {
        let v = self.dialog.allow_close;
        self.dialog.allow_close = false;
        if v {
            self.settings.save();
        }
        v
    }

    /// Update the persisted window position/size. Called once per frame from
    /// `bar-app` with the current viewport rect (no-op cost if unchanged —
    /// only the in-memory struct is touched; disk writes happen on close).
    pub fn update_window_state(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        maximized: bool,
    ) {
        // When the window is maximized, the OS-reported rect is the
        // full screen — useless for restoring a sane "windowed" size on
        // next launch. Keep whatever rect was last saved while
        // unmaximized so the user's preferred restored dimensions
        // survive a maximize/unmaximize cycle.
        let new = if maximized {
            let prev = self.settings.window.as_ref();
            crate::settings::WindowState {
                x: prev.map(|w| w.x).unwrap_or(x),
                y: prev.map(|w| w.y).unwrap_or(y),
                width: prev.map(|w| w.width).unwrap_or(width),
                height: prev.map(|w| w.height).unwrap_or(height),
                maximized: true,
            }
        } else {
            crate::settings::WindowState {
                x,
                y,
                width,
                height,
                maximized: false,
            }
        };
        let differs = self
            .settings
            .window
            .as_ref()
            .map(|w| {
                (w.x - new.x).abs() > 0.5
                    || (w.y - new.y).abs() > 0.5
                    || (w.width - new.width).abs() > 0.5
                    || (w.height - new.height).abs() > 0.5
                    || w.maximized != new.maximized
            })
            .unwrap_or(true);
        if differs {
            self.settings.window = Some(new);
        }
    }

    /// Tell the GUI that the OS / user requested to close the window. If the
    /// project is dirty, this opens a save/discard/cancel modal. Otherwise
    /// the close is approved immediately (next `take_allow_close` returns
    /// true).
    pub fn request_close(&mut self) {
        if self.project.is_dirty {
            self.dialog.pending_action = Some(PendingAction::Close);
        } else {
            self.dialog.allow_close = true;
        }
    }

    /// Open a path queued externally (e.g. a drag-drop drop target). Routes
    /// through the unsaved-changes dialog if needed.
    pub fn open_path_external(&mut self, path: std::path::PathBuf) {
        self.start_open_path(path);
    }

    /// Begin a New Project, deferring through unsaved-changes confirmation
    /// when the current project is dirty.
    pub(crate) fn start_new_project(&mut self) {
        if self.project.is_dirty {
            self.dialog.pending_action = Some(PendingAction::NewProject);
        } else {
            self.do_new_project();
        }
    }

    /// Handle a click on the Edit Map Info toolbar button. If the user has
    /// already designated a map-info file for this project, open it in the
    /// in-app floating editor. Otherwise, prompt them to pick from the
    /// bundle's passthrough files.
    /// Render the 2D inspector window. Backdrop is the latest evaluated
    /// heightmap (rendered grayscale-with-water-tint); start positions are
    /// draggable markers on top.
    /// 2D Inspector window - see `crate::panels::inspector`.
    pub(crate) fn draw_inspector_window(&mut self, ctx: &egui::Context) {
        crate::panels::inspector::draw(self, ctx);
    }

    /// Render the structured Map Info editor: a single window with one
    /// `CollapsingHeader` per major section of the recipe + `MapSettings`.
    /// Edits write directly into `self.map.settings` and the recipe-side
    /// mirror fields. On save those values are folded into the project's
    /// `Recipe` and `MapSettings`.
    /// Map Info modal - see `crate::panels::mapinfo_editor`.
    pub(crate) fn draw_mapinfo_editor_window(&mut self, ctx: &egui::Context) {
        crate::panels::mapinfo_editor::draw(self, ctx);
    }

    pub(crate) fn handle_edit_map_info_clicked(&mut self) {
        // mapinfo.lua is generated by the bundler from `MapSettings`,
        // not picked from a passed-through file anymore (the older
        // "designate a file in your project as the map info" flow
        // is gone). Clicking the toolbar button toggles the
        // structured Map Settings modal — that's where every
        // mapinfo field is edited now.
        self.dialog.show_mapinfo_editor = !self.dialog.show_mapinfo_editor;
    }

    /// Load `abs_path` from disk and open the in-app editor for it.
    pub(crate) fn open_file_editor(&mut self, abs_path: String, archive_path: String) {
        let content = match std::fs::read_to_string(&abs_path) {
            Ok(s) => s,
            Err(e) => {
                self.dialog.status_message = Some(format!("Failed to read file: {e}"));
                return;
            }
        };
        self.dialog.file_editor = Some(FileEditor {
            abs_path,
            archive_path,
            content,
            is_dirty: false,
        });
    }

    /// Perform the actual node deletion (the destructive-confirm path runs
    /// the dialog first; the no-confirm path calls this directly).
    /// Delete a SubGraph + every member node it owns, in one undo
    /// step. Used by the Delete-key path, the collapsed-block right-
    /// click "Delete subgraph", and the properties-panel "Delete"
    /// button when the selected group is a SubGraph. Skips the
    /// "delete with members or just dissolve?" modal because for a
    /// SubGraph the inner nodes are conceptually part of the same
    /// unit — leaving them orphaned is almost never what the user
    /// wants. Push_undo takes the full editor-state snapshot, so
    /// undo restores the SubGraph with all its inner nodes,
    /// connections, ports, bindings, and macro params intact.
    pub(crate) fn delete_subgraph_with_contents(&mut self, gid: u64) {
        if !self.visuals.groups.contains_key(&gid) {
            return;
        }
        self.push_undo("Delete subgraph");
        let members: Vec<NodeId> = self
            .visuals
            .groups
            .get(&gid)
            .map(|g| g.member_ids.iter().copied().collect())
            .unwrap_or_default();
        self.dissolve_group(gid);
        if self.selection.group == Some(gid) {
            self.selection.group = None;
        }
        for nid in &members {
            let _ = self.graph.remove_node(*nid);
            self.visuals.node_visuals.remove(nid);
            self.remove_node_from_group(*nid);
            if self.preview.node == Some(*nid) {
                self.preview.node = None;
                self.preview.open = false;
            }
        }
        self.project.passthrough_edit = None;
        self.clear_selection();
    }

    pub(crate) fn delete_selected_node(&mut self) {
        // Snapshot the IDs to delete: the primary plus everything else
        // in the multi-selection set. (The set always includes the
        // primary by invariant.)
        let to_delete: Vec<NodeId> = if !self.selection.nodes.is_empty() {
            self.selection.nodes.iter().copied().collect()
        } else if let Some(id) = self.selection.node {
            vec![id]
        } else {
            return;
        };
        self.push_undo("Delete node");
        for node_id in &to_delete {
            let _ = self.graph.remove_node(*node_id);
            self.visuals.node_visuals.remove(node_id);
            self.remove_node_from_group(*node_id);
            if self.preview.node == Some(*node_id) {
                self.preview.node = None;
                self.preview.open = false;
            }
        }
        self.project.passthrough_edit = None;
        self.clear_selection();
    }

    /// Resolve a queued PendingAction now that the unsaved-changes prompt
    /// has been answered.
    pub(crate) fn apply_pending_action(&mut self, action: PendingAction) {
        match action {
            PendingAction::Close => {
                self.dialog.allow_close = true;
            }
            PendingAction::NewProject => {
                self.do_new_project();
            }
            PendingAction::OpenPath(p) => {
                self.dispatch_open(p);
            }
            PendingAction::LoadMacro { name } => {
                self.start_with_macro(&name);
            }
        }
    }

    pub fn graph_mut(&mut self) -> &mut GraphEngine {
        &mut self.graph
    }

    /// SMF ground shading inputs sourced from `MapSettings.lighting`
    /// and `MapSettings.water`. Snapshot of the values an in-engine
    /// renderer would read for the same map. Consumers (bar-app's
    /// preview pipeline) clone this each frame; never store.
    pub fn smf_lighting(&self) -> SmfLightingSnapshot {
        let lit = &self.map.settings.lighting;
        let w = &self.map.settings.water;
        SmfLightingSnapshot {
            sun_dir: lit.sun_dir,
            ground_ambient: lit.ground_ambient,
            ground_diffuse: lit.ground_diffuse,
            ground_specular: lit.ground_specular,
            specular_exponent: lit.spec_exponent,
            water_absorb: w.absorb,
            water_base: w.base_color,
            water_min: w.min_color,
        }
    }

    /// Composite cache key for the 3D preview's input state. Bumps
    /// whenever ANY input that the preview depends on changes — graph
    /// revision, the active preview-target node, map dimensions, or
    /// the height range. The preview pipeline gates re-evaluations on
    /// this single value rather than on `graph.revision()` alone, so
    /// switching the preview target or resizing the map triggers a
    /// fresh eval the same way a graph mutation does.
    ///
    /// Hash output rather than tuple comparison so the gate value
    /// stays a single u64 — keeps the in-flight `PreviewResult` tag
    /// the same shape as before and the renderer's frame-revision
    /// arithmetic compatible.
    pub fn preview_cache_key(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        // Hash the upstream subgraph of the preview node so that changes
        // to disconnected nodes don't trigger a re-render.
        if let Some(pn) = self.preview.node {
            self.graph.upstream_content_hash(pn).hash(&mut h);
        } else {
            self.graph.revision().hash(&mut h);
        }
        self.preview
            .node
            .map(|n| n.0)
            .unwrap_or(u64::MAX)
            .hash(&mut h);
        self.map.width.hash(&mut h);
        self.map.height.hash(&mut h);
        self.map.min_height.to_bits().hash(&mut h);
        self.map.max_height.to_bits().hash(&mut h);
        h.finish()
    }

    /// Build a `Recipe` snapshot with the live identity / dimensions /
    /// `MapSettings` the user has been editing. Used by `bar-app` when it
    /// fires off an export thread — the bundler reads identity from
    /// `Recipe`, so the simple "MapSettings::default()" shortcut no
    /// longer suffices. The graph nodes/connections are left empty
    /// because `execute_bundlers` walks the live `GraphEngine` directly,
    /// not the recipe — but identity, dimensions, and `MapSettings`
    /// must come from this snapshot.
    pub fn recipe_for_export(&self) -> bar_project::Recipe {
        bar_project::Recipe {
            schema_version: bar_project::RECIPE_SCHEMA_VERSION,
            name: self
                .project
                .path
                .as_ref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().to_string())
                .or_else(|| self.project.loaded_name.clone())
                .unwrap_or_else(|| "Untitled".to_string()),
            shortname: self.map.recipe_meta.shortname.clone(),
            description: self.map.recipe_meta.description.clone(),
            author: self.map.recipe_meta.author.clone(),
            version: self.map.recipe_meta.version.clone(),
            nodes: Vec::new(),
            connections: Vec::new(),
            output: bar_project::OutputConfig {
                width: self.map.width,
                height: self.map.height,
                map_settings: bar_project::MapSettings {
                    min_height: self.map.min_height,
                    max_height: self.map.max_height,
                    ..self.map.settings.clone()
                },
            },
        }
    }

    /// Set a status message to show in the status bar.
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.dialog.status_message = Some(msg.into());
    }

    /// Clear the status message.
    pub fn clear_status(&mut self) {
        self.dialog.status_message = None;
    }

    /// Returns a display label for the preview window title. Lives on
    /// `BarEditorApp` (rather than `PreviewState`) because it needs the
    /// node label out of `self.graph`.
    pub fn preview_node_label(&self) -> String {
        self.preview
            .node()
            .and_then(|id| self.graph.get_node(id))
            .map(|n| format!("3D Preview — {}", n.label))
            .unwrap_or_else(|| "3D Preview".to_string())
    }

    /// Push the latest evaluated heightmap so the 2D inspector can show
    /// it as a backdrop. Cheap when called every frame: only stores a
    /// clone and bumps a revision counter; the texture re-upload happens
    /// lazily inside the inspector's draw path.
    ///
    /// If the user has sculpted since the last reset, we keep their
    /// edits and ignore the incoming heightmap. Cleared by starting a
    /// new project.
    pub fn set_inspector_heightmap(&mut self, hm: bar_data::Heightmap) {
        if self.paint.sculpt.dirty {
            return;
        }
        self.paint.heightmap = Some(hm);
        self.paint.heightmap_rev = self.paint.heightmap_rev.wrapping_add(1);
    }

    /// True when the viewport's primary mouse button should sculpt rather
    /// than orbit. The Sculpt3D layout is unconditionally a sculpting
    /// surface; otherwise requires the inspector to be open in Sculpt mode.
    pub fn is_sculpt_input_active(&self) -> bool {
        if self.active_layout == Layout::Sculpt3D {
            return true;
        }
        self.paint.inspector_mode == InspectorMode::Sculpt
            && (self.dialog.show_inspector || self.paint.sculpt.dirty)
    }

    /// Which data layer the brush currently writes to. The 3D viewport
    /// uses this to dispatch dabs to the right paint pipeline.
    pub fn active_brush_target(&self) -> BrushTarget {
        self.paint.brush.target
    }

    /// Current brush radius in heightmap pixels. The 3D viewport
    /// reads this to compute the world-space radius for the cursor
    /// ring overlay.
    pub fn brush_radius_px(&self) -> f32 {
        self.paint.brush.radius_px
    }

    /// Read-only access to the current inspector heightmap. Used by the
    /// 3D viewport for ray-cast picking against the surface the user
    /// actually sees.
    pub fn inspector_heightmap_ref(&self) -> Option<&bar_data::Heightmap> {
        self.paint.heightmap.as_ref()
    }

    /// Returns a fresh clone of the current inspector heightmap so the
    /// caller can re-upload it to the 3D mesh without holding a borrow
    /// on `self`. Cheap-ish: `Heightmap` is just a `Vec<f32>` clone.
    pub fn inspector_heightmap_clone(&self) -> Option<bar_data::Heightmap> {
        self.paint.heightmap.clone()
    }

    /// Set the live colour-buffer cache. Called by `bar-app` whenever
    /// a fresh eval result arrives so the brush has a base layer to
    /// stamp on top of. Replaces the cache wholesale; in-flight
    /// brush state is kept (the user's stroke composes with the new
    /// upstream output the next time their brush moves).
    pub fn set_inspector_color_buffer(&mut self, cb: bar_data::ColorBuffer) {
        self.paint.color_buffer = Some(cb);
    }

    /// Live metalmap cache setter. Same shape as
    /// `set_inspector_color_buffer` for the metalmap brush target.
    pub fn set_inspector_metalmap(&mut self, hm: bar_data::Heightmap) {
        self.paint.metalmap = Some(hm);
    }

    /// Live typemap cache setter.
    pub fn set_inspector_typemap(&mut self, hm: bar_data::Heightmap) {
        self.paint.typemap = Some(hm);
    }

    /// Clone of the live colour-buffer cache. None when no eval
    /// has produced one yet — the brush is then a no-op until the
    /// graph yields a base layer to overlay onto.
    pub fn inspector_color_buffer_clone(&self) -> Option<bar_data::ColorBuffer> {
        self.paint.color_buffer.clone()
    }

    /// Clone of the live metalmap cache.
    pub fn inspector_metalmap_clone(&self) -> Option<bar_data::Heightmap> {
        self.paint.metalmap.clone()
    }

    /// Clone of the live typemap cache.
    pub fn inspector_typemap_clone(&self) -> Option<bar_data::Heightmap> {
        self.paint.typemap.clone()
    }

    /// Node palette - see `crate::panels::palette`.
    pub(crate) fn draw_node_palette(&mut self, ui: &mut egui::Ui) {
        crate::panels::palette::draw(self, ui);
    }
}

impl eframe::App for BarEditorApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Single dispatch point: the active `Layout` decides which
        // panels render and how they're arranged. Future layouts
        // plug in here without touching this body.
        crate::layouts::dispatch::draw_active(self, ctx, frame);
    }
}

impl BarEditorApp {
    /// The currently selected top-level UI layout. Today only
    /// `Layout::Standard` exists; future variants are surfaced via
    /// the toolbar layout-switcher and persisted in user settings.
    pub fn active_layout(&self) -> Layout {
        self.active_layout
    }

    /// Switch to a different layout. Layouts are pure UI/UX, so the
    /// switch is instant and never migrates project state. Persists
    /// the choice via `Settings` so the user picks up where they
    /// left off after a restart.
    pub fn set_active_layout(&mut self, layout: Layout) {
        if self.active_layout == layout {
            return;
        }
        self.active_layout = layout;
        self.settings.active_layout = layout;
        self.settings.save();
        // Close windows that are only meaningful in the layout we just left.
        // Map info editor is the exception -- it spans all layouts.
        self.dialog.show_inspector = false;
        self.dialog.file_editor = None;
        self.dialog.show_validation_panel = false;
        self.props.active = None;
        self.dialog.pending_props_open = None;
    }
}

// Brush dab math + tests live in `crate::paint::brush_math`. Re-exported
// here under the historical name so existing callers don't break.
pub(crate) use crate::paint::brush_math::apply_brush_dab;

pub(crate) use crate::io::dialogs::make_path_dialog;
pub(crate) use crate::io::png::{heightmap_to_color_image, save_heightmap_as_png16};
pub(crate) use crate::panels::canvas::passthrough::{
    build_path_tree, draw_passthrough_body, draw_path_tree,
};
pub(crate) use crate::panels::canvas::style::{
    build_io_outline, cubic_bezier, node_type_color, polyline_distance, port_kind_color,
};

/// Encode a byte slice as a lowercase hex string (2 chars per byte).
pub(crate) fn mask_hex_encode(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(data.len() * 2);
    for b in data {
        let _ = write!(out, "{:02x}", b);
    }
    out
}

/// Parse a 6-character `RRGGBB` hex string into an `[r, g, b]` array.
/// Returns `None` if the string isn't six valid hex digits.
pub(crate) fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let bytes = s.as_bytes();
    if bytes.len() != 6 {
        return None;
    }
    let mut out = [0u8; 3];
    for i in 0..3 {
        let hi = (bytes[i * 2] as char).to_digit(16)?;
        let lo = (bytes[i * 2 + 1] as char).to_digit(16)?;
        out[i] = (hi << 4 | lo) as u8;
    }
    Some(out)
}

/// Decode a hex string into bytes. Non-hex or odd-length trailing chars are skipped.
pub(crate) fn mask_hex_decode(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i + 1 < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16);
        let lo = (bytes[i + 1] as char).to_digit(16);
        if let (Some(h), Some(l)) = (hi, lo) {
            out.push((h << 4 | l) as u8);
        }
        i += 2;
    }
    out
}

// Pure path helpers (PassThrough files, asset packing, bar:// URL
// resolution) live in `crate::project::path`. Re-export the names that
// callers outside this module use historically.
pub(crate) use crate::project::path::parse_passthrough_files;

// brush_tests module relocated to `crate::paint::brush_math`.
// session_reset_tests module relocated to `crate::project::lifecycle`.

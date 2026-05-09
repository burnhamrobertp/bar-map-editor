use bar_graph::{GraphEngine, Node, NodeId, NodeType, ParamValue, PortId, PortKind, PortPlacement};
use eframe::egui;
use std::collections::HashMap;
use std::time::Instant;

use crate::settings::Settings;
use crate::state::{EditorState, NodeVisual};
use crate::undo::{Snapshot, UndoHistory};

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
/// One tab in the canvas-area tab bar. The user can keep multiple
/// editing contexts open and switch between them — much faster than
/// "exit confined mode, scroll to the relevant region, double-click
/// back in". Tabs are session-scoped state; they don't persist
/// through save/load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanvasView {
    /// The whole graph. Always present, can't be closed.
    Main,
    /// Edit-in-isolation view of one sub-graph's contents — the
    /// previous "confined edit mode" lifted into a tab so the user
    /// can keep the Main tab open alongside.
    SubGraph(u64),
}

/// Active filter tab in the validation details window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationFilter {
    All,
    Error,
    Warning,
    Info,
}

/// Active section in the Map Settings modal — replaces the per-section
/// CollapsingHeaders so only one section's controls are on screen at a
/// time, switched via a tab strip across the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapInfoTab {
    Identity,
    Dimensions,
    Physics,
    Atmosphere,
    Lighting,
    Water,
}

// `ValidationFingerprint` lives in `crate::editor::validation`.
pub(crate) use crate::editor::ValidationFingerprint;

pub(crate) enum GroupOp {
    CreateWith(NodeId),
    CreateFromSelection(Vec<NodeId>),
    AddTo(NodeId, u64),
    AddManyTo(Vec<NodeId>, u64),
    RemoveFrom(NodeId),
}

/// Sort key for barycentric ordering inside an auto-layout column.
/// A unit whose sources have already been placed in earlier columns
/// gets a key equal to the mean of those source Y positions
/// (centred on its `y_center` to align by mid-row, not top-edge);
/// a unit with no in-target sources falls back to its current
/// canvas Y so the user's manual "this row before that row" hint is
/// preserved for unconnected units.
pub(crate) fn barycentric_key(
    idx: usize,
    incoming: &[Vec<usize>],
    sizes: &[egui::Vec2],
    placed_top_y: &std::collections::HashMap<usize, f32>,
    units: &[LayoutUnit],
    app: &BarEditorApp,
) -> f32 {
    let preds = &incoming[idx];
    let mut placed_centres: Vec<f32> = Vec::new();
    for &p in preds {
        if let Some(top) = placed_top_y.get(&p) {
            placed_centres.push(top + sizes[p].y * 0.5);
        }
    }
    if placed_centres.is_empty() {
        units[idx].current_top_left(app).y
    } else {
        placed_centres.iter().sum::<f32>() / placed_centres.len() as f32
    }
}

/// One target of the Auto Layout pass. Either a standalone graph
/// node (movable on its own) or a subgraph (treated as one rigid
/// unit — the whole block moves and members keep their relative
/// positions).
pub(crate) enum LayoutUnit {
    Node(NodeId),
    Subgraph { members: Vec<NodeId> },
}

impl LayoutUnit {
    /// Stable identifier for use as a HashMap key during depth
    /// computation. For a standalone node it's the node itself; for
    /// a subgraph it's the smallest member id (subgraphs always
    /// have ≥1 member by construction).
    pub(crate) fn representative_id(&self) -> NodeId {
        match self {
            LayoutUnit::Node(id) => *id,
            LayoutUnit::Subgraph { members } => members
                .iter()
                .min_by_key(|n| n.0)
                .copied()
                .expect("subgraph must have at least one member"),
        }
    }

    /// Every NodeId associated with this unit. For a standalone
    /// node, just itself; for a subgraph, all members. Used to map
    /// NodeId → unit during depth computation.
    pub(crate) fn member_ids(&self) -> Vec<NodeId> {
        match self {
            LayoutUnit::Node(id) => vec![*id],
            LayoutUnit::Subgraph { members } => members.clone(),
        }
    }

    /// Top-left corner of the unit's current bounding rect on the
    /// canvas. For a standalone node this is its visual.position;
    /// for a subgraph it's the min over member positions.
    pub(crate) fn current_top_left(&self, app: &BarEditorApp) -> egui::Pos2 {
        match self {
            LayoutUnit::Node(id) => app
                .visuals
                .node_visuals
                .get(id)
                .map(|v| v.position)
                .unwrap_or(egui::pos2(0.0, 0.0)),
            LayoutUnit::Subgraph { members } => members
                .iter()
                .filter_map(|m| app.visuals.node_visuals.get(m))
                .map(|v| v.position)
                .reduce(|a, b| egui::pos2(a.x.min(b.x), a.y.min(b.y)))
                .unwrap_or(egui::pos2(0.0, 0.0)),
        }
    }

    /// Width × height of the unit's current bounding box, in canvas
    /// pixels. Drives auto-layout's "no overlap" guarantee — column
    /// widths and row stacks are sized off this rather than a fixed
    /// pitch that could let a tall node bleed into its neighbour.
    pub(crate) fn bounding_size(&self, app: &BarEditorApp) -> egui::Vec2 {
        match self {
            LayoutUnit::Node(id) => app
                .visuals
                .node_visuals
                .get(id)
                .map(|v| v.size)
                .unwrap_or(egui::vec2(150.0, 80.0)),
            LayoutUnit::Subgraph { members } => {
                let mut min = egui::pos2(f32::INFINITY, f32::INFINITY);
                let mut max = egui::pos2(f32::NEG_INFINITY, f32::NEG_INFINITY);
                for m in members {
                    if let Some(v) = app.visuals.node_visuals.get(m) {
                        let p0 = v.position;
                        let p1 = egui::pos2(p0.x + v.size.x, p0.y + v.size.y);
                        min.x = min.x.min(p0.x);
                        min.y = min.y.min(p0.y);
                        max.x = max.x.max(p1.x);
                        max.y = max.y.max(p1.y);
                    }
                }
                if min.x.is_finite() {
                    egui::vec2(max.x - min.x, max.y - min.y)
                } else {
                    egui::vec2(180.0, 100.0)
                }
            }
        }
    }

    /// Translate the unit's nodes by `delta`. For a standalone node
    /// this moves just it; for a subgraph every member shifts by
    /// the same delta so internal layout is preserved.
    pub(crate) fn translate(&self, app: &mut BarEditorApp, delta: egui::Vec2) {
        match self {
            LayoutUnit::Node(id) => {
                if let Some(v) = app.visuals.node_visuals.get_mut(id) {
                    v.position += delta;
                }
            }
            LayoutUnit::Subgraph { members } => {
                for m in members {
                    if let Some(v) = app.visuals.node_visuals.get_mut(m) {
                        v.position += delta;
                    }
                }
            }
        }
    }
}

/// Outcome of the "delete group" confirmation modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GroupDeleteChoice {
    /// Dissolve the group; member nodes stay where they are.
    GroupOnly,
    /// Dissolve the group AND delete its member nodes from the graph.
    GroupAndMembers,
    /// Close the dialog without changing anything.
    Cancel,
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

/// State for an in-progress connection drag. Outputs always emit from a
/// Right placement, so the wire's tangent at the source end is always +X.
#[derive(Clone, Debug)]
pub struct DragConnection {
    pub from_node: NodeId,
    pub from_port: String,
    pub from_pos: egui::Pos2,
}

/// State for the inline text editor inside the PassThrough properties panel.
#[derive(Debug, Clone)]
pub struct PassthroughEdit {
    pub node_id: NodeId,
    pub abs_path: String,
    pub archive_path: String,
    pub content: String,
    pub is_dirty: bool,
}

/// Floating in-app text editor — used by the Edit Map Info button and any
/// future "open this file" action. Lives outside the side panels so it can
/// be resized and scrolled freely.
#[derive(Debug, Clone)]
pub struct FileEditor {
    /// Absolute path on disk; what we read from and write back to.
    pub(crate) abs_path: String,
    /// Bundle-relative path (forward slashes) for display.
    pub(crate) archive_path: String,
    pub(crate) content: String,
    pub(crate) is_dirty: bool,
}

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

/// Plain-data snapshot of SMF ground-shading inputs (lighting +
/// water-absorption). Returned by `BarEditorApp::smf_lighting` and
/// consumed by `bar-app` to populate the renderer's per-frame
/// `SmfLighting`. Lives in `bar-gui` so callers can read it without
/// pulling in `bar-render` as a transitive dep.
#[derive(Clone, Copy, Debug)]
pub struct SmfLightingSnapshot {
    pub sun_dir: [f32; 3],
    pub ground_ambient: [f32; 3],
    pub ground_diffuse: [f32; 3],
    pub ground_specular: [f32; 3],
    pub specular_exponent: f32,
    pub water_absorb: [f32; 3],
    pub water_base: [f32; 3],
    pub water_min: [f32; 3],
}

/// Action waiting on the user's response to an unsaved-changes confirmation.
/// Once the dialog resolves, the chosen action is performed (after Save when
/// the user picks Save, or directly when they pick Discard).
#[derive(Clone, Debug)]
pub enum PendingAction {
    /// The OS or the user asked to close the window.
    Close,
    /// The user clicked New Project (Ctrl+N or menu).
    NewProject,
    /// The user picked an Open target (file path) and we need to load it after
    /// resolving the unsaved-changes prompt.
    OpenPath(std::path::PathBuf),
    /// The user picked a built-in macro from the File menu.
    LoadMacro { name: String },
}

/// Generic yes/no/cancel modal state.
#[derive(Clone, Debug)]
pub struct ConfirmDialog {
    pub(crate) title: String,
    pub(crate) message: String,
    /// Action label for the affirmative button (e.g. "Delete", "Discard").
    pub(crate) affirm_label: String,
    /// What the affirmative button should trigger.
    pub(crate) on_affirm: ConfirmAction,
    /// When `Some`, render a "Don't ask again" checkbox; ticking it
    /// while affirming adds this key to `settings.suppressed_
    /// confirmations` so the matching modal type stops appearing.
    /// Suppression is per-key — flipping the toggle on the
    /// delete-node modal only affects the delete-node modal, not
    /// other confirms. Cleared via Preferences.
    pub(crate) suppression_key: Option<String>,
    /// Live state of the "Don't ask again" checkbox.
    pub(crate) dont_ask_again: bool,
}

/// What the contextual Properties panel is currently editing. Each
/// variant resolves to a screen-space rect at render time so the
/// panel can anchor itself relative to the target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropsTarget {
    Node(NodeId),
    Group(u64),
}

impl PropsTarget {
    /// Stable per-target value used to seed the popup's egui Id so
    /// switching between targets doesn't reuse the same window
    /// state (which would carry over scroll position etc.).
    pub(crate) fn id_hash(&self) -> u64 {
        match self {
            PropsTarget::Node(n) => n.0,
            PropsTarget::Group(g) => g ^ 0xA5A5_A5A5_A5A5_A5A5,
        }
    }
}

/// In-flight gate for opening the contextual Properties panel. The
/// user clicks a node, the click-position and time get captured
/// here, and `update_pending_props_open` checks each frame whether
/// the cursor has held still on the target long enough to actually
/// pop the panel open. Drags / motion / new clicks elsewhere clear
/// the pending state without ever opening anything.
#[derive(Clone, Debug)]
pub struct PendingPropsOpen {
    pub target: PropsTarget,
    pub armed_at: Instant,
    pub armed_pos: egui::Pos2,
}

/// Delay between releasing a click on a target and the contextual
/// Properties panel opening. Tuned to feel "intentional, not
/// trigger-happy" — instant feels like a flicker, anything over
/// ~150 ms feels sluggish.
pub(crate) const PROPS_OPEN_DELAY_MS: u64 = 100;
/// Maximum cursor drift, in screen pixels, allowed during the
/// post-click hover before the gate resets. Anything beyond this is
/// treated as the user moving on.
pub(crate) const PROPS_OPEN_MOVE_TOLERANCE: f32 = 4.0;

/// Suppression keys for the confirmation modals that support
/// "Don't ask again". One per modal type — extending: add a new
/// const here, set it on the dialog when opening, give it a
/// display name in `confirm_key_display_name`, and the
/// preferences "clear" button picks it up automatically.
pub(crate) const CONFIRM_KEY_DELETE_CONNECTED_NODE: &str = "delete_connected_node";

/// Friendly label for one of the confirmation keys, used by the
/// preferences panel. Falls back to the raw key for any keys not
/// listed (so adding a new modal still shows up sensibly even if
/// the developer forgets to update this).
pub(crate) fn confirm_key_display_name(key: &str) -> String {
    match key {
        CONFIRM_KEY_DELETE_CONNECTED_NODE => "Delete a node that has wires connected".to_string(),
        other => other.to_string(),
    }
}

#[derive(Clone, Debug)]
pub(crate) enum ConfirmAction {
    /// Delete the selected node (already captured in app state).
    DeleteSelected,
}

/// Result of the unsaved-changes modal.
#[derive(Clone, Copy, Debug)]
pub(crate) enum UnsavedDecision {
    Save,
    Discard,
    Cancel,
}

/// Current export status, supplied each frame by `bar-app` so the GUI can
/// render busy state on the bundle buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportStatus {
    /// No export in flight.
    #[default]
    Idle,
    /// Export running for all bundlers in the graph (toolbar click).
    All,
    /// Export running for a single bundler.
    One(NodeId),
}

impl ExportStatus {
    /// True if any export is currently in flight.
    pub fn is_running(self) -> bool {
        !matches!(self, ExportStatus::Idle)
    }

    /// True if the bundler with `id` should render in busy state.
    pub fn affects(self, id: NodeId) -> bool {
        matches!(self, ExportStatus::All) || matches!(self, ExportStatus::One(x) if x == id)
    }
}

/// In-memory sculpt data — the live accumulator for brush strokes
/// across all four layers. Written by brush operations; read by
/// `pack_sculpt_record` at save time and by the renderer for live

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

/// Identity fields the bundler reads when generating `mapinfo.lua`
/// — name, shortname, description, author, version. These mirror
/// the same-named fields on `Recipe`. Keeping them in one struct
/// rather than five loose fields on `BarEditorApp` makes the
/// "this is a single source of truth that the recipe is built
/// from" relationship explicit, and shrinks `apply_project` /
/// `build_project` / `reset_project` accordingly.
#[derive(Default, Clone, Debug)]
pub struct RecipeMeta {
    /// Optional shortname (`mapinfo.shortname`). When `None` the
    /// bundler falls back to the project name (the long display
    /// name). Lets a long display name like "Kolmog Estuary 1v1"
    /// coexist with a tighter id like `kolmog_1v1`.
    pub shortname: Option<String>,
    /// Free-form description (`mapinfo.description`). Empty string
    /// is allowed.
    pub description: String,
    /// Optional author. When `None` the bundler falls back to
    /// `"bar-editor"`.
    pub author: Option<String>,
    /// Optional map version string (`"3"`, `"playtest-2"`). When
    /// `None` the bundler falls back to `"1.0"`.
    pub version: Option<String>,
}

/// Modal / popup / transient-feedback UI state, grouped here so the
/// god-object on top doesn't have to mention every individual flag.
/// Anything that's "is some dialog or transient overlay currently
/// visible / queued?" lives here. Things that are *targets* of a
/// dialog (e.g. `active_props`) or that *configure* a panel's
/// content (e.g. `validation_filter`, `mapinfo_tab`) stay on
/// `BarEditorApp` because their lifetimes extend past the dialog.
#[derive(Default)]
pub struct DialogState {
    /// Whether the validation panel window is open.
    pub show_validation_panel: bool,
    /// Whether the 2D inspector window is open.
    pub show_inspector: bool,
    /// Whether the structured Map Info editor window is open.
    pub show_mapinfo_editor: bool,
    /// Whether the Preferences window is open.
    pub show_settings: bool,
    /// Whether the About dialog is open.
    pub show_about: bool,
    /// True while the "pick which file is the map info" picker
    /// modal is open.
    pub show_map_info_picker: bool,
    /// True for one frame after the user accepts an unsaved-changes
    /// close so `bar-app` can let the window actually close.
    pub allow_close: bool,
    /// Generic confirm-dialog state (delete confirmation, etc.).
    pub(crate) confirm_dialog: Option<ConfirmDialog>,
    /// Pending action that's blocked on the unsaved-changes confirm
    /// dialog. `Some` means a modal is currently open.
    pub(crate) pending_action: Option<PendingAction>,
    /// In-app floating text editor (Edit Map Info / future "open
    /// file" triggers). `None` when no editor is open.
    pub(crate) file_editor: Option<FileEditor>,
    /// Transient toast message shown over the canvas
    /// (e.g. "Autosaved 2s ago"). `(message, until_instant)`.
    pub toast: Option<(String, Instant)>,
    /// Status bar message — replaces toast for non-time-bound
    /// feedback (last save path, error from a failed load, etc.).
    pub status_message: Option<String>,
    /// In-flight click waiting on the 100 ms post-click hover gate
    /// before the contextual properties panel pops open. Cleared
    /// by any pointer movement away from the target, by a drag
    /// start, or once the gate elapses (whichever comes first).
    pub pending_props_open: Option<PendingPropsOpen>,
}

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

    /// Mutable access to user settings — used by the Preferences
    /// panel. Callers who change a setting should call
    /// `self.settings().save()` to persist; the dialog panel does
    /// this once after every changed-pass.
    pub(crate) fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    // Dialog accessors for the panel module. `dialog` is a
    // `pub(crate)` field but accessing it across modules costs a
    // few extra characters; these methods keep panel call sites
    // tidy and let `app.rs` retain control over the invariant
    // (today: just a plain bool, but a future "dialog stack"
    // refactor lands cleanly behind these accessors).
    pub(crate) fn dialog_show_settings(&self) -> bool {
        self.dialog.show_settings
    }
    pub(crate) fn set_dialog_show_settings(&mut self, open: bool) {
        self.dialog.show_settings = open;
    }
    pub(crate) fn dialog_show_about(&self) -> bool {
        self.dialog.show_about
    }
    pub(crate) fn set_dialog_show_about(&mut self, open: bool) {
        self.dialog.show_about = open;
    }
    pub(crate) fn dialog_show_validation_panel(&self) -> bool {
        self.dialog.show_validation_panel
    }
    pub(crate) fn set_dialog_show_validation_panel(&mut self, open: bool) {
        self.dialog.show_validation_panel = open;
    }

    // Validation accessors used by `panels::validation`. The
    // findings vec is read-only from the panel side; the filter is
    // mutable because clicking a tab updates it. `validation_last_
    // fingerprint` is bumped after a manual refresh so the
    // continuous-validation gate doesn't immediately re-run.
    pub(crate) fn validation_findings(&self) -> &[bar_project::Finding] {
        self.validation.findings()
    }
    pub(crate) fn validation_filter(&self) -> ValidationFilter {
        self.validation.filter()
    }
    pub(crate) fn set_validation_filter(&mut self, f: ValidationFilter) {
        self.validation.set_filter(f);
    }
    pub(crate) fn refresh_validation_fingerprint(&mut self) {
        self.validation.last_fingerprint = self.validation_inputs_fingerprint();
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

    // ── Accessors for `panels::*` ─────────────────────────────────────
    // These bridge the panels module's `&mut BarEditorApp` view to
    // the private fields it needs to read/write. Each is a thin
    // wrapper; they exist so panels stay in their own module
    // without `pub` leaking out to the whole crate API.

    pub(crate) fn dialog_show_inspector(&self) -> bool {
        self.dialog.show_inspector
    }
    pub(crate) fn set_dialog_show_inspector(&mut self, open: bool) {
        self.dialog.show_inspector = open;
    }
    pub(crate) fn dialog_show_mapinfo_editor(&self) -> bool {
        self.dialog.show_mapinfo_editor
    }
    pub(crate) fn set_dialog_show_mapinfo_editor(&mut self, open: bool) {
        self.dialog.show_mapinfo_editor = open;
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

    /// Re-run project validation and stash the findings for the panel.
    /// True iff the current cached validation has any blocking
    /// errors. Cheap — just scans the cached findings list.
    pub fn validation_has_errors(&self) -> bool {
        bar_project::has_errors(&self.validation.findings)
    }

    /// Count cached findings by severity for the sidebar display.
    pub fn validation_counts(&self) -> (usize, usize, usize) {
        let mut errors = 0;
        let mut warnings = 0;
        let mut infos = 0;
        for f in &self.validation.findings {
            match f.severity {
                bar_project::Severity::Error => errors += 1,
                bar_project::Severity::Warning => warnings += 1,
                bar_project::Severity::Info => infos += 1,
            }
        }
        (errors, warnings, infos)
    }

    /// Re-run validation iff any input that feeds it has changed since
    /// the last run. Runs at the top of every frame so the sidebar
    /// counts and the export gate are always in sync with the editor
    /// state — no manual click needed.
    /// Walk every subgraph in the project and rebuild its
    /// `subgraph_inputs/outputs` from the `SubgraphInput` /
    /// `SubgraphOutput` nodes inside it. Each IO node contributes one
    /// external port to the collapsed block:
    ///
    /// - `SubgraphInput` → an entry in `subgraph_inputs`, bound to its
    ///   `value` *input* port (so an outer wire connects directly to
    ///   the IO node from the outside).
    /// - `SubgraphOutput` → an entry in `subgraph_outputs`, bound to
    ///   its `value` *output* port (so the outer graph reads from the
    ///   IO node).
    ///
    /// Idempotent and cheap; safe to call every frame. Replaces the
    /// previous "subgraph ports are edited via a modal form" model.
    pub(crate) fn recompute_all_subgraph_io(&mut self) {
        // Snapshot member sets and node descriptors first so we can
        // iterate without holding a borrow on `self.graph`.
        let groups: Vec<(u64, Vec<NodeId>)> = self
            .visuals
            .groups
            .iter()
            .filter(|(_, g)| g.is_subgraph)
            .map(|(gid, g)| (*gid, g.member_ids.iter().copied().collect()))
            .collect();
        for (gid, members) in groups {
            // Sort members by NodeId so the per-kind fallback
            // suffix (when an IO node has no explicit name) is
            // stable across save/load.
            let mut sorted = members;
            sorted.sort_by_key(|nid| nid.0);

            // Pre-pass: collect IO nodes with their kinds so we
            // can disambiguate same-kind ports with a numeric
            // suffix. Names that the user has explicitly set
            // bypass the numbering — they win.
            #[derive(Clone)]
            struct IoEntry {
                nid: NodeId,
                is_input: bool,
                /// Display label inferred from the connected port (e.g. "Slope").
                /// Empty string means nothing is connected ("Unknown" display).
                kind_display: String,
                /// Underlying PortKind for type enforcement, inferred from the
                /// connected port's actual kind.
                port_kind: PortKind,
                explicit_name: Option<String>,
            }
            // Track all SubgraphOutput nids so we can reset disconnected ones.
            let all_output_nids: Vec<NodeId> = sorted
                .iter()
                .filter(|&&nid| {
                    self.graph
                        .get_node(nid)
                        .map(|n| n.node_type == NodeType::SubgraphOutput)
                        .unwrap_or(false)
                })
                .copied()
                .collect();

            let mut entries: Vec<IoEntry> = Vec::new();
            for nid in sorted {
                let Some(node) = self.graph.get_node(nid) else {
                    continue;
                };
                let (is_input, is_output) = match node.node_type {
                    NodeType::SubgraphInput => (true, false),
                    NodeType::SubgraphOutput => (false, true),
                    _ => (false, false),
                };
                if !(is_input || is_output) {
                    continue;
                }
                // Both input and output: infer from whatever is wired into
                // the "value" input port. For outputs, nothing connected means
                // no external port. For inputs, nothing connected is valid but
                // shows as "Unknown".
                let conn_info = self
                    .graph
                    .connections()
                    .iter()
                    .find(|c| c.to.node_id == nid && c.to.port_name == "value")
                    .and_then(|c| {
                        self.graph
                            .get_node(c.from.node_id)?
                            .outputs
                            .iter()
                            .find(|p| p.name == c.from.port_name)
                            .map(|p| (p.label.clone(), p.kind))
                    });
                let (kind_display, port_kind) = match conn_info {
                    Some((label, pk)) => (label, pk),
                    None if is_output => continue, // no connection: no external port
                    None => (String::new(), PortKind::Heightmap), // input: Unknown
                };
                let explicit_name = match node.params.get("name") {
                    Some(ParamValue::String(s)) if !s.is_empty() => Some(s.clone()),
                    _ => None,
                };
                entries.push(IoEntry {
                    nid,
                    is_input,
                    kind_display,
                    port_kind,
                    explicit_name,
                });
            }

            // Count auto-named ports per (role, kind) so we know
            // which need a "2", "3", … suffix. Empty kind ("Unknown")
            // is counted separately. Explicit names don't contribute.
            let mut auto_counts: std::collections::HashMap<(bool, String), usize> =
                std::collections::HashMap::new();
            for e in &entries {
                if e.explicit_name.is_none() {
                    *auto_counts
                        .entry((e.is_input, e.kind_display.clone()))
                        .or_insert(0) += 1;
                }
            }

            // Collect (nid, kind_display, port_kind) for all IO nodes so
            // we can sync their node params and port kinds after the loop.
            let io_kind_syncs: Vec<(NodeId, String, PortKind)> = entries
                .iter()
                .map(|e| (e.nid, e.kind_display.clone(), e.port_kind))
                .collect();

            let mut inputs: Vec<crate::state::SubgraphPortRuntime> = Vec::new();
            let mut outputs: Vec<crate::state::SubgraphPortRuntime> = Vec::new();
            let mut auto_seen: std::collections::HashMap<(bool, String), usize> =
                std::collections::HashMap::new();
            for e in entries {
                let display_kind = if e.kind_display.is_empty() {
                    "Unknown"
                } else {
                    &e.kind_display
                };
                let label = if let Some(ref n) = e.explicit_name {
                    n.clone()
                } else {
                    let total = *auto_counts
                        .get(&(e.is_input, e.kind_display.clone()))
                        .unwrap_or(&1);
                    let idx = auto_seen
                        .entry((e.is_input, e.kind_display.clone()))
                        .or_insert(0);
                    *idx += 1;
                    if total > 1 {
                        format!("{} {}", display_kind, idx)
                    } else {
                        display_kind.to_string()
                    }
                };
                let port = crate::state::SubgraphPortRuntime {
                    name: label.clone(),
                    label,
                    kind: format!("{:?}", e.port_kind),
                    binding: Some((e.nid, "value".to_string())),
                };
                if e.is_input {
                    inputs.push(port);
                } else {
                    outputs.push(port);
                }
            }
            if let Some(g) = self.visuals.groups.get_mut(&gid) {
                g.subgraph_inputs = inputs;
                g.subgraph_outputs = outputs;
            }
            // Sync kind_display param and port kind for all IO nodes
            // that had a connection this frame.
            let synced_nids: std::collections::HashSet<NodeId> =
                io_kind_syncs.iter().map(|(nid, _, _)| *nid).collect();
            for (nid, kind_display, port_kind) in io_kind_syncs {
                if let Some(node) = self.graph.get_node_mut(nid) {
                    node.params
                        .insert("kind".to_string(), ParamValue::String(kind_display));
                    node.set_io_port_kind(port_kind);
                }
            }
            // Reset disconnected SubgraphOutput nodes to Unknown state.
            for nid in all_output_nids {
                if !synced_nids.contains(&nid) {
                    if let Some(node) = self.graph.get_node_mut(nid) {
                        node.params
                            .insert("kind".to_string(), ParamValue::String(String::new()));
                        node.set_io_port_kind(PortKind::Heightmap);
                    }
                }
            }
        }
    }

    pub(crate) fn refresh_validation_if_dirty(&mut self) {
        let fp = self.validation_inputs_fingerprint();
        if fp != self.validation.last_fingerprint {
            self.run_validation();
            self.validation.last_fingerprint = fp;
        }
    }

    /// Compact fingerprint of every input `validate_project` reads.
    /// Used to decide whether the cached findings are still valid.
    /// Cheap: small struct, cheap to compare.
    pub(crate) fn validation_inputs_fingerprint(&self) -> ValidationFingerprint {
        ValidationFingerprint {
            graph_revision: self.graph.revision(),
            map_width: self.map.width,
            map_height: self.map.height,
            min_h_bits: self.map.settings.min_height.to_bits(),
            max_h_bits: self.map.settings.max_height.to_bits(),
            n_spawns: self.map.settings.start_positions.len(),
        }
    }

    /// Compact "Validation" summary in the left sidebar: live error /
    /// warning / info counts plus a Details button that opens the
    /// findings panel. Replaces the "Nodes: N / Connections: N"
    /// stats that used to live in the status bar — the per-severity
    /// counts are far more actionable.
    ///
    /// Validation itself runs at the top of every frame from
    /// `update`'s `refresh_validation_if_dirty`; this method just
    /// reads the cached findings.
    /// Sidebar validation summary - see `crate::panels::validation`.
    pub(crate) fn draw_validation_summary(&mut self, ui: &mut egui::Ui) {
        crate::panels::validation::draw_summary(self, ui);
    }

    /// Validation gate for the export flow. Runs validation, then:
    /// - if there are errors, opens the panel and refuses to start
    ///   the export (returns `false`);
    /// - otherwise, the caller is cleared to set `run_requested` /
    ///   `run_bundler_node` (returns `true`).
    pub(crate) fn validate_before_export(&mut self, action_label: &str) -> bool {
        self.run_validation();
        if self.validation_has_errors() {
            self.dialog.show_validation_panel = true;
            self.dialog.status_message =
                Some(format!("{action_label}: fix validation errors first."));
            false
        } else {
            true
        }
    }

    pub(crate) fn run_validation(&mut self) {
        // We construct a temporary MapSettings with current min/max
        // height so the validator sees what the project will export
        // with. Other fields use defaults — full structured-mapinfo
        // editing comes in M1.1.
        let settings = bar_project::MapSettings {
            min_height: self.map.min_height,
            max_height: self.map.max_height,
            ..Default::default()
        };
        self.validation.findings =
            bar_project::validate_project(&self.graph, &settings, self.map.width, self.map.height);
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

    /// Apply the current brush at heightmap pixel coordinates. Call
    /// this once per dab. Setting `stroke_starting = true` captures
    /// the Flatten target at stroke start. Returns true iff the
    /// heightmap actually changed.
    ///
    /// Two effects per dab:
    /// 1. `sculpt.height_delta` is updated for persistent save/export.
    /// 2. The inspector heightmap is mutated in-place for instant feedback.
    pub fn apply_brush_at_heightmap(&mut self, hx: f32, hy: f32, stroke_starting: bool) -> bool {
        let (hm_w, hm_h) = match self.paint.heightmap.as_ref() {
            Some(hm) => (hm.width() as f32, hm.height() as f32),
            None => return false,
        };
        let dim_w = hm_w as u32;
        let dim_h = hm_h as u32;
        if stroke_starting && self.paint.brush.tool == BrushTool::Flatten {
            let hm = self.paint.heightmap.as_ref().unwrap();
            let ix = (hx.round() as i32).clamp(0, hm.width() as i32 - 1) as u32;
            let iy = (hy.round() as i32).clamp(0, hm.height() as i32 - 1) as u32;
            self.paint.brush.flatten_target = hm.get(ix, iy);
        }
        // Write to the persistent sculpt height delta.
        if self.paint.sculpt.height_delta.is_none() {
            self.paint.sculpt.height_delta = bar_data::Heightmap::new(dim_w, dim_h).ok();
        }
        if let Some(ref mut delta) = self.paint.sculpt.height_delta {
            apply_brush_dab(delta, hx, hy, &self.paint.brush);
        }
        // Mutate the inspector heightmap for instant visual feedback.
        if let Some(hm) = self.paint.heightmap.as_mut() {
            apply_brush_dab(hm, hx, hy, &self.paint.brush);
            self.paint.heightmap_rev = self.paint.heightmap_rev.wrapping_add(1);
        }
        self.paint.sculpt.dirty = true;
        self.paint.brush_stroking = true;
        self.project.is_dirty = true;
        true
    }

    /// Mark the end of a 3D-viewport sculpt stroke. Pairs with
    /// `apply_brush_at_heightmap`. Releases the per-stroke Flatten
    /// target so the next stroke captures a fresh one.
    pub fn end_brush_stroke(&mut self) {
        self.paint.brush_stroking = false;
        self.paint.brush.flatten_target = None;
    }

    /// Paint one colour brush dab at heightmap-pixel coordinates.
    /// Routes to a `TextureSculpt` overlay node inserted between the
    /// existing `Bundler.texture` source and the Bundler. The dab is
    /// recorded as a normalised-space entry in the node's `dabs`
    /// param; on next eval the executor reads the upstream Color,
    /// replays every recorded dab on top, and outputs the composite.
    /// Upstream texture pipelines (AutoTexture, imported, painted)
    /// flow through unchanged — the brush is purely additive overlay
    /// in the same shape as the heightmap `Sculpt` node.
    ///
    /// Returns true iff a dab was recorded. False when no upstream
    /// texture exists yet (the user needs to wire one in first).
    pub fn apply_color_brush_at_heightmap(&mut self, hx: f32, hy: f32) -> bool {
        let (hm_w, hm_h) = match self.paint.heightmap.as_ref() {
            Some(hm) => (hm.width(), hm.height()),
            None => return false,
        };
        let map_dim = (hm_w.max(hm_h) as f32).max(1.0);
        let u = (hx / hm_w as f32).clamp(0.0, 1.0);
        let v = (hy / hm_h as f32).clamp(0.0, 1.0);
        let ru = (self.paint.brush.radius_px / map_dim).max(0.001);
        let [r, g, b] = self.paint.brush.color_rgb;
        // Write to the persistent sculpt texture overlay.
        if self.paint.sculpt.texture_overlay.is_none() {
            self.paint.sculpt.texture_overlay = bar_data::ColorBuffer::new(hm_w, hm_h).ok();
        }
        if let Some(ref mut cb) = self.paint.sculpt.texture_overlay {
            stamp_color_dab_in_buffer(cb, u, v, ru, [r, g, b]);
        }
        // Mirror into the live cache for instant viewport feedback.
        if let Some(ref mut cb) = self.paint.color_buffer {
            stamp_color_dab_in_buffer(cb, u, v, ru, [r, g, b]);
        }
        self.paint.sculpt.dirty = true;
        self.project.is_dirty = true;
        true
    }

    /// Paint one metalmap dab into the sculpt metal overlay.
    pub fn apply_metal_brush_at_heightmap(&mut self, hx: f32, hy: f32) -> bool {
        let (hm_w, hm_h) = match self.paint.heightmap.as_ref() {
            Some(hm) => (hm.width(), hm.height()),
            None => return false,
        };
        let map_dim = (hm_w.max(hm_h) as f32).max(1.0);
        let u = (hx / hm_w as f32).clamp(0.0, 1.0);
        let v = (hy / hm_h as f32).clamp(0.0, 1.0);
        let ru = (self.paint.brush.radius_px / map_dim).max(0.001);
        let value = self.paint.brush.paint_value.clamp(0.0, 1.0);
        if self.paint.sculpt.metal_overlay.is_none() {
            self.paint.sculpt.metal_overlay = bar_data::Heightmap::new(hm_w, hm_h).ok();
        }
        if self.paint.sculpt.metal_alpha.is_none() {
            self.paint.sculpt.metal_alpha = bar_data::Heightmap::new(hm_w, hm_h).ok();
        }
        if let Some(ref mut hm) = self.paint.sculpt.metal_overlay {
            stamp_value_dab_in_heightmap(hm, u, v, ru, value);
        }
        if let Some(ref mut hm) = self.paint.sculpt.metal_alpha {
            stamp_value_dab_in_heightmap(hm, u, v, ru, 1.0);
        }
        // Mirror into the live metalmap cache for instant feedback.
        if self.paint.metalmap.is_none() {
            self.paint.metalmap = bar_data::Heightmap::new(hm_w, hm_h).ok();
        }
        if let Some(ref mut hm) = self.paint.metalmap {
            stamp_value_dab_in_heightmap(hm, u, v, ru, value);
        }
        self.paint.sculpt.dirty = true;
        self.project.is_dirty = true;
        true
    }

    /// Paint one typemap dab into the sculpt type overlay.
    pub fn apply_type_brush_at_heightmap(&mut self, hx: f32, hy: f32) -> bool {
        let (hm_w, hm_h) = match self.paint.heightmap.as_ref() {
            Some(hm) => (hm.width(), hm.height()),
            None => return false,
        };
        let map_dim = (hm_w.max(hm_h) as f32).max(1.0);
        let u = (hx / hm_w as f32).clamp(0.0, 1.0);
        let v = (hy / hm_h as f32).clamp(0.0, 1.0);
        let ru = (self.paint.brush.radius_px / map_dim).max(0.001);
        let value = self.paint.brush.paint_value.clamp(0.0, 1.0);
        if self.paint.sculpt.type_overlay.is_none() {
            self.paint.sculpt.type_overlay = bar_data::Heightmap::new(hm_w, hm_h).ok();
        }
        if self.paint.sculpt.type_alpha.is_none() {
            self.paint.sculpt.type_alpha = bar_data::Heightmap::new(hm_w, hm_h).ok();
        }
        if let Some(ref mut hm) = self.paint.sculpt.type_overlay {
            stamp_value_dab_in_heightmap(hm, u, v, ru, value);
        }
        if let Some(ref mut hm) = self.paint.sculpt.type_alpha {
            stamp_value_dab_in_heightmap(hm, u, v, ru, 1.0);
        }
        // Mirror into the live typemap cache for instant feedback.
        if self.paint.typemap.is_none() {
            self.paint.typemap = bar_data::Heightmap::new(hm_w, hm_h).ok();
        }
        if let Some(ref mut hm) = self.paint.typemap {
            stamp_value_dab_in_heightmap(hm, u, v, ru, value);
        }
        self.paint.sculpt.dirty = true;
        self.project.is_dirty = true;
        true
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

    /// Capture the entire undoable editor state.
    pub(crate) fn snapshot(&self, description: &str) -> Snapshot {
        Snapshot {
            state: EditorState {
                graph: self.graph.clone(),
                node_visuals: self.visuals.node_visuals.clone(),
                groups: self.visuals.groups.clone(),
                node_to_group: self.visuals.node_to_group.clone(),
                next_group_id: self.visuals.next_group_id,
            },
            description: description.to_string(),
        }
    }

    /// Push the current state onto the undo stack before a mutation.
    /// Pair every user-visible mutation with one of these calls; the
    /// pairing is what lets undo restore "the state before X happened".
    pub(crate) fn push_undo(&mut self, description: &str) {
        let snap = self.snapshot(description);
        self.history.push(snap);
        self.project.is_dirty = true;
    }

    /// Swap the editor's state with a captured snapshot. Resets
    /// transient UI state (selections, the preview-open hint when its
    /// target node is gone) so the user doesn't see stale highlights
    /// pointing at deleted things.
    pub(crate) fn restore_snapshot(&mut self, snap: Snapshot) {
        self.graph = snap.state.graph;
        self.visuals.node_visuals = snap.state.node_visuals;
        self.visuals.groups = snap.state.groups;
        self.visuals.node_to_group = snap.state.node_to_group;
        self.visuals.next_group_id = snap.state.next_group_id;
        self.clear_selection();
        if let Some(pn) = self.preview.node {
            if self.graph.get_node(pn).is_none() {
                self.preview.node = None;
                self.preview.open = false;
            }
        }
    }

    /// Perform undo.
    pub fn undo(&mut self) {
        let current = self.snapshot("current");
        if let Some(prev) = self.history.undo(current) {
            self.restore_snapshot(prev);
        }
    }

    /// Perform redo.
    pub fn redo(&mut self) {
        let current = self.snapshot("current");
        if let Some(next) = self.history.redo(current) {
            self.restore_snapshot(next);
        }
    }

    // `build_project`, `save_project`, `resolve_relative_paths`, and
    // `pack_assets_for_save` live in `crate::project::persistence`.

    /// Drop a macro template's contents onto the canvas at `pos`.
    /// Inner nodes get fresh ids, the SubGraph wrapper is registered,
    /// and the new group is selected so its properties immediately
    /// show in the side panel. The whole drop is one undo step.
    pub(crate) fn instantiate_macro(&mut self, macro_name: &str, pos: egui::Pos2) {
        let Some(template) = crate::macros::parse(macro_name) else {
            self.dialog.status_message = Some(format!("Macro '{macro_name}' not found"));
            return;
        };
        self.push_undo(&format!("Drop macro '{}'", template.name));
        // Compute the numbered wrapper label BEFORE instantiation so
        // we don't double-count nodes about to be added.
        let numbered_label = self.next_label_for(&template.name);
        let mut inst = match crate::macros::instantiate(&template, &mut self.graph, pos) {
            Ok(i) => i,
            Err(e) => {
                self.dialog.status_message =
                    Some(format!("Macro '{macro_name}' failed to instantiate: {e}"));
                return;
            }
        };
        inst.group.label = numbered_label;
        for (id, visual) in inst.visuals {
            self.visuals.node_visuals.insert(id, visual);
        }
        let gid = self.visuals.alloc_group_id();
        for nid in &inst.member_ids {
            self.visuals.node_to_group.insert(*nid, gid);
        }
        self.visuals.groups.insert(gid, inst.group);
        self.select_group(gid);
        // Same direct-open as `add_node_at` — drop a macro, see its
        // properties immediately so you can tweak the parameters
        // without a separate click + hover.
        self.props.active = Some(PropsTarget::Group(gid));
        self.dialog.pending_props_open = None;
        self.project.is_dirty = true;
        self.dialog.status_message = Some(format!("Dropped '{}' onto the canvas.", template.name));
    }

    pub(crate) fn add_node_at(&mut self, node_type: NodeType, label: &str, pos: egui::Pos2) {
        self.push_undo("Add node");
        let numbered = self.next_label_for(label);
        let node = Node::new(NodeId(0), node_type.clone(), numbered);
        let id = self.graph.add_node(node);
        // A freshly-dropped node is "what the user wants to look at"
        // — open the contextual properties panel immediately
        // without waiting for the hover gate. Skipping the gate is
        // intentional: there's no ambiguity here (no "did they
        // mean to drag instead?"), the node is the click result.
        self.props.active = Some(PropsTarget::Node(id));
        self.dialog.pending_props_open = None;
        let default_size = match node_type {
            NodeType::PassThrough => egui::vec2(180.0, 200.0),
            NodeType::Bundler => egui::vec2(210.0, 240.0),
            NodeType::SubgraphInput | NodeType::SubgraphOutput => IO_NODE_SIZE,
            _ => egui::vec2(150.0, 80.0),
        };
        self.visuals.node_visuals.insert(
            id,
            NodeVisual {
                position: pos,
                size: default_size,
            },
        );
        self.selection.node = Some(id);
        // If the user is viewing a subgraph tab when they drop a
        // node, the drop goes INTO that subgraph: add it to the
        // group's member set so `hidden_nodes_this_frame` (which
        // hides everything outside the active subgraph in the
        // subgraph view) doesn't immediately hide it. Without this,
        // the new node lives at the top level of the graph and
        // becomes invisible the moment it's dropped — properties
        // panel opens on a node the user can't see.
        if let Some(CanvasView::SubGraph(scope)) =
            self.canvas.tabs.get(self.canvas.active_tab).cloned()
        {
            if let Some(group) = self.visuals.groups.get_mut(&scope) {
                group.member_ids.insert(id);
                self.visuals.node_to_group.insert(id, scope);
            }
        }
        // Auto-open the 3D preview when a Bundler is created so the user
        // immediately sees the viewport associated with this export node.
        if node_type == NodeType::Bundler {
            self.preview.open = true;
            self.preview.node = Some(id);
        }
    }

    /// Node palette - see `crate::panels::palette`.
    pub(crate) fn draw_node_palette(&mut self, ui: &mut egui::Ui) {
        crate::panels::palette::draw(self, ui);
    }

    /// Replace the selection with a single primary node. Clears every
    /// other kind of selection (group, connection) — they share the
    /// side properties panel; the user is editing one thing at a time.
    pub(crate) fn select_only_node(&mut self, id: NodeId) {
        self.selection.nodes.clear();
        self.selection.nodes.insert(id);
        self.selection.node = Some(id);
        self.selection.group = None;
        self.selection.connection = None;
    }

    /// Toggle a node's membership in the multi-selection set. Updates
    /// the primary so it always points at *some* member of the set
    /// (or None if the set ended up empty).
    pub(crate) fn toggle_select_node(&mut self, id: NodeId) {
        if self.selection.nodes.contains(&id) {
            self.selection.nodes.remove(&id);
            if self.selection.node == Some(id) {
                self.selection.node = self.selection.nodes.iter().next().copied();
            }
        } else {
            self.selection.nodes.insert(id);
            self.selection.node = Some(id);
        }
        self.selection.group = None;
        self.selection.connection = None;
    }

    /// Drop every selection (clicking empty canvas, opening a new
    /// project, etc.).
    pub(crate) fn clear_selection(&mut self) {
        self.selection.nodes.clear();
        self.selection.node = None;
        self.selection.group = None;
        self.selection.connection = None;
        // Also drop any open / pending Properties panel — its target
        // is no longer interesting.
        self.dialog.pending_props_open = None;
        self.props.close();
    }

    /// Select a group as the active editing target.
    pub(crate) fn select_group(&mut self, group_id: u64) {
        self.selection.node = None;
        self.selection.nodes.clear();
        self.selection.group = Some(group_id);
        self.selection.connection = None;
    }

    /// Select a single wire as the active editing target.
    pub(crate) fn select_connection(&mut self, from: PortId, to: PortId) {
        self.selection.node = None;
        self.selection.nodes.clear();
        self.selection.group = None;
        self.selection.connection = Some((from, to));
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
// here under the historical names so existing callers don't break.
pub(crate) use crate::paint::brush_math::apply_brush_dab;
use crate::paint::brush_math::{stamp_color_dab_in_buffer, stamp_value_dab_in_heightmap};

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

use bar_graph::{GraphEngine, Node, NodeId, NodeType, ParamValue, PortId, PortKind, PortPlacement};
use eframe::egui;
use std::collections::HashMap;
use std::time::Instant;

use crate::settings::Settings;
use crate::state::{EditorState, NodeVisual};
use crate::t;
use crate::undo::{Snapshot, UndoHistory};

use crate::panels::tokens;

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

/// Returns true for file extensions that can be edited as plain text.
pub(crate) use crate::io::is_text_file;

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
enum GroupDeleteChoice {
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
    abs_path: String,
    /// Bundle-relative path (forward slashes) for display.
    archive_path: String,
    content: String,
    is_dirty: bool,
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

/// What primary action a left-click + drag in the 2D Inspector does.
/// Switched via the radio control at the top of the inspector window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorMode {
    /// Click to add / drag existing markers / right-click to delete.
    Spawns,
    /// Drag-paint with the heightmap brush.
    Sculpt,
}

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

/// Heightmap-sculpting brush mode. Each tool applies a different
/// transformation to the pixels under the brush footprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushTool {
    /// Raise terrain under the brush.
    Raise,
    /// Lower terrain under the brush.
    Lower,
    /// Average pixels with their neighbours (low-pass filter).
    Smooth,
    /// Pull pixels toward a target height (the height under the cursor when the
    /// stroke started).
    Flatten,
}

impl BrushTool {
    pub(crate) fn label(self) -> &'static str {
        match self {
            BrushTool::Raise => "Raise",
            BrushTool::Lower => "Lower",
            BrushTool::Smooth => "Smooth",
            BrushTool::Flatten => "Flatten",
        }
    }
}

/// What kind of data the brush writes to. Heightmap is the existing
/// sculpt path; Color paints into a `PaintedTexture` node; Metalmap /
/// Typemap paint into role-tagged `PaintedHeightmap` nodes (planned —
/// see `docs/3d-painting-plan.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushTarget {
    Heightmap,
    Color,
    Metalmap,
    Typemap,
}

impl BrushTarget {
    pub(crate) fn label(self) -> &'static str {
        match self {
            BrushTarget::Heightmap => "Heightmap",
            BrushTarget::Color => "Colour",
            BrushTarget::Metalmap => "Metal",
            BrushTarget::Typemap => "Type",
        }
    }
    pub(crate) fn is_available(self) -> bool {
        true
    }
}

/// Live brush configuration shared between the 2D Inspector and (later)
/// the 3D viewport. Pixel-radius applies to the heightmap; the inspector
/// scales it to its rendered image size.
#[derive(Clone, Debug)]
pub struct BrushState {
    pub tool: BrushTool,
    /// Which data layer the brush writes to.
    pub target: BrushTarget,
    /// Radius in heightmap pixels (1 px = 8 elmos).
    pub radius_px: f32,
    /// Strength in normalized heightmap units per stroke-application.
    /// (Heightmap is f32 [0,1]; 0.01 = 1% of full range per dab.)
    pub strength: f32,
    /// Falloff exponent (1.0 = linear, 2.0 = squared, sharper centre).
    pub falloff: f32,
    /// Target height for Flatten mode, captured at stroke start.
    /// `None` outside an active flatten stroke.
    pub flatten_target: Option<f32>,
    /// Brush colour for `BrushTarget::Color`. Packed RGB; alpha is
    /// implicit 1.0 — full coverage. Can be edited from the inspector
    /// toolbar (or the PaintedTexture properties panel as today).
    pub color_rgb: [u8; 3],
    /// Stamp value for `BrushTarget::Metalmap` / `BrushTarget::Typemap`.
    /// Range `[0, 1]` — for metal it's density (0 = none, 1 = max);
    /// for type it's a quantised id (multiplied by 255 at export
    /// time). One slider serves both.
    pub paint_value: f32,
}

impl Default for BrushState {
    fn default() -> Self {
        Self {
            tool: BrushTool::Raise,
            target: BrushTarget::Heightmap,
            color_rgb: [0x8B, 0x73, 0x55],
            paint_value: 1.0,
            radius_px: 32.0,
            strength: 0.02,
            falloff: 2.0,
            flatten_target: None,
        }
    }
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
    title: String,
    message: String,
    /// Action label for the affirmative button (e.g. "Delete", "Discard").
    affirm_label: String,
    /// What the affirmative button should trigger.
    on_affirm: ConfirmAction,
    /// When `Some`, render a "Don't ask again" checkbox; ticking it
    /// while affirming adds this key to `settings.suppressed_
    /// confirmations` so the matching modal type stops appearing.
    /// Suppression is per-key — flipping the toggle on the
    /// delete-node modal only affects the delete-node modal, not
    /// other confirms. Cleared via Preferences.
    suppression_key: Option<String>,
    /// Live state of the "Don't ask again" checkbox.
    dont_ask_again: bool,
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
const CONFIRM_KEY_DELETE_CONNECTED_NODE: &str = "delete_connected_node";

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
enum ConfirmAction {
    /// Delete the selected node (already captured in app state).
    DeleteSelected,
}

/// Result of the unsaved-changes modal.
#[derive(Clone, Copy, Debug)]
enum UnsavedDecision {
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
/// preview.
#[derive(Default)]
pub struct SculptState {
    /// Signed height delta. Zero where unmodified. Composite shown in
    /// the inspector is `heightmap + height_delta`.
    pub height_delta: Option<bar_data::Heightmap>,
    /// Metal-density overlay [0..1]. Painted where alpha > 0.
    pub metal_overlay: Option<bar_data::Heightmap>,
    pub metal_alpha: Option<bar_data::Heightmap>,
    /// Type-id overlay. Same shape as metal.
    pub type_overlay: Option<bar_data::Heightmap>,
    pub type_alpha: Option<bar_data::Heightmap>,
    /// RGBA texture overlay. rgb = colour, alpha = coverage.
    pub texture_overlay: Option<bar_data::ColorBuffer>,
    /// True when any layer has been modified since the last save.
    pub dirty: bool,
}

/// Live brush, sculpt-lock, and per-layer paint caches used by the
/// 2D inspector and the 3D viewport for per-stroke feedback. The
/// caches mirror whatever the most recent graph eval produced;
/// brush dabs mutate them in place between evals so the user sees
/// strokes land before the eval has caught up. Grouped here so the
/// god-object on top doesn't have to declare every paint-cache
/// field separately.
///
/// Fields are public to the `app` module — the per-target brush
/// dispatch and the `bar-app` viewport overlay both need direct
/// access. A future tightening pass can wrap these in methods, but
/// the ownership story (one struct, one lifetime, dropped on graph
/// reset) is the load-bearing change.
pub struct PaintSession {
    /// What clicking and dragging in the 2D Inspector does
    /// (placing / editing start positions vs. sculpting the
    /// heightmap). Lives with the brush because the inspector tab
    /// is the only place it's read.
    pub inspector_mode: InspectorMode,
    /// Live brush state — tool, target, color, radius/strength,
    /// flatten anchor, etc.
    pub brush: BrushState,
    /// True while a sculpt stroke is in progress (mouse held
    /// down). Used to capture the Flatten target at stroke start.
    pub brush_stroking: bool,
    /// Project-level in-memory sculpt data. Written by brush
    /// operations across all four layers; saved to sidecar files
    /// by `pack_sculpt_record` and merged at export time.
    pub sculpt: SculptState,
    /// Brush radius (heightmap pixels) for `PaintedHeightmap` /
    /// `PaintedTexture` / `Sculpt` in-node paint canvases. The 2D-inspector
    /// brush uses `brush.radius_px` instead.
    pub paint_brush_radius: f32,
    /// Strength for the Sculpt node's delta brush (0.0-1.0).
    /// Controls how far a single stroke moves the delta from neutral (128).
    /// Not used by PaintedHeightmap or PaintedTexture.
    pub sculpt_brush_strength: f32,
    /// Last heightmap fed in by `bar-app` after a preview eval —
    /// the backdrop image for the 2D inspector and the source the
    /// 3D viewport ray-casts against.
    pub heightmap: Option<bar_data::Heightmap>,
    /// Bumped whenever `heightmap` is replaced; the 2D inspector
    /// uses it to know when to rebuild its egui texture handle.
    pub heightmap_rev: u64,
    /// Cached egui texture for the 2D inspector backdrop.
    pub texture: Option<egui::TextureHandle>,
    pub texture_rev: u64,
    /// Live colour-buffer cache populated from the most recent
    /// eval. Color brush dabs mutate this for instant per-stroke
    /// feedback; the sculpt.texture_overlay holds the persistent
    /// record for save/export.
    pub color_buffer: Option<bar_data::ColorBuffer>,
    /// Live metalmap cache — same shape as color_buffer but for
    /// metal-density paint.
    pub metalmap: Option<bar_data::Heightmap>,
    /// Live typemap cache.
    pub typemap: Option<bar_data::Heightmap>,
    /// Retained egui texture handles for `PaintedHeightmap`
    /// canvases, keyed by NodeId.
    pub mask_textures: HashMap<NodeId, egui::TextureHandle>,
    /// Populated by `pack_sculpt_record` before `build_project` is
    /// called. Taken with `.take()` in `build_project` so the record
    /// lands on the serialised `Project::sculpt`.
    pub pending_sculpt_record: Option<bar_project::SculptRecord>,
}

impl Default for PaintSession {
    fn default() -> Self {
        Self {
            inspector_mode: InspectorMode::Spawns,
            brush: BrushState::default(),
            brush_stroking: false,
            sculpt: SculptState::default(),
            paint_brush_radius: 4.0,
            sculpt_brush_strength: 0.5,
            heightmap: None,
            heightmap_rev: 0,
            texture: None,
            texture_rev: 0,
            color_buffer: None,
            metalmap: None,
            typemap: None,
            mask_textures: HashMap::new(),
            pending_sculpt_record: None,
        }
    }
}

impl PaintSession {
    /// Drop the live caches so the next graph eval repopulates
    /// them from scratch. Called on project switch / new project /
    /// graph reset.
    pub fn invalidate_on_graph_reset(&mut self) {
        self.brush = BrushState::default();
        self.brush_stroking = false;
        self.sculpt = SculptState::default();
        self.heightmap = None;
        self.heightmap_rev = self.heightmap_rev.wrapping_add(1);
        self.texture = None;
        self.texture_rev = self.texture_rev.wrapping_add(1);
        self.color_buffer = None;
        self.metalmap = None;
        self.typemap = None;
        self.mask_textures.clear();
    }
}

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
    fn start_new_project(&mut self) {
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
    fn draw_inspector_window(&mut self, ctx: &egui::Context) {
        crate::panels::inspector::draw(self, ctx);
    }

    /// Render the structured Map Info editor: a single window with one
    /// `CollapsingHeader` per major section of the recipe + `MapSettings`.
    /// Edits write directly into `self.map.settings` and the recipe-side
    /// mirror fields. On save those values are folded into the project's
    /// `Recipe` and `MapSettings`.
    /// Map Info modal - see `crate::panels::mapinfo_editor`.
    fn draw_mapinfo_editor_window(&mut self, ctx: &egui::Context) {
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

    fn refresh_validation_if_dirty(&mut self) {
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
    fn draw_validation_summary(&mut self, ui: &mut egui::Ui) {
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

    fn handle_edit_map_info_clicked(&mut self) {
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
    fn draw_node_palette(&mut self, ui: &mut egui::Ui) {
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

    /// Body of the `eframe::App::update` lifecycle, owning the
    /// pre-frame work (file-dialog poll, validation refresh, …),
    /// the panel composition, and the post-frame work (autosave,
    /// repaint scheduling). Called by every `Layout` variant; will
    /// shrink as panel logic migrates into `crate::panels`.
    pub(crate) fn pre_frame_work(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // OS / window close request — route through the unsaved-changes
        // workflow so accidental clicks on the close button don't lose work.
        // `bar-app` is responsible for blocking the actual viewport close until
        // `take_allow_close()` returns true.
        if ctx.input(|i| i.viewport().close_requested()) {
            self.request_close();
        }

        // Poll any in-flight Open dialog (see `open_file_dialog_async`).
        // Non-blocking; either the worker has produced a result or the
        // user is still picking. When a result arrives, dispatch it
        // through the same `start_open_path` the synchronous code paths
        // used to call directly.
        if let Some(rx) = self.project.pending_open_rx.as_ref() {
            match rx.try_recv() {
                Ok(maybe_path) => {
                    self.project.pending_open_rx = None;
                    if let Some(path) = maybe_path {
                        self.start_open_path(path);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // Dialog still open; redraw next frame so we keep
                    // polling — egui won't otherwise tick on its own.
                    ctx.request_repaint();
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // Worker panicked; drop the receiver so the user
                    // can try again.
                    self.project.pending_open_rx = None;
                }
            }
        }

        // Refresh subgraph IO from contained nodes. Each subgraph's
        // `subgraph_inputs/outputs` list is *derived* from the
        // `SubgraphInput` / `SubgraphOutput` member nodes — the user
        // adds / removes / renames / re-types ports by editing those
        // nodes directly, not via a properties-panel form. Doing
        // this once per frame keeps the collapsed-block port
        // rendering in sync without anyone having to remember to
        // call a refresh function.
        self.recompute_all_subgraph_io();

        // Continuous validation — runs at the start of every frame
        // when any validation-relevant input has changed (graph
        // structure / params, map dimensions, or map settings).
        // Cheap; cached findings drive the sidebar summary and the
        // bundle-button gate without anyone having to click "validate".
        self.refresh_validation_if_dirty();

        // Tick auto-save. Cheap: a single Instant comparison per frame.
        if self.settings.autosave_enabled
            && self.project.is_dirty
            && self.dialog.pending_action.is_none()
        {
            let interval = std::time::Duration::from_secs(self.settings.autosave_interval_secs);
            let due = self
                .project
                .last_autosave_at
                .map(|t| t.elapsed() >= interval)
                .unwrap_or(true);
            if due {
                self.autosave_now();
            }
        }

        // Expire toast notifications.
        if let Some((_, until)) = &self.dialog.toast {
            if Instant::now() >= *until {
                self.dialog.toast = None;
            }
        }

        // Update window title to reflect loaded name and dirty state
        let dirty_marker = if self.project.is_dirty { " *" } else { "" };
        let title = match self
            .project
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .or_else(|| self.project.loaded_name.clone())
        {
            Some(name) => format!("{name}{dirty_marker} — BAR - Map Editor"),
            None => "BAR - Map Editor".to_string(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        // Keyboard shortcuts. Suppress while a modal is open (the dialog has
        // its own buttons) and while a text widget has focus (so typing 'Z'
        // inside a text field doesn't undo the graph).
        let modal_open = self.dialog.pending_action.is_some()
            || self.dialog.confirm_dialog.is_some()
            || self.dialog.show_settings
            || self.dialog.show_about;
        let typing = ctx.wants_keyboard_input();
        if !modal_open {
            let (do_undo, do_redo, do_save, do_save_as, do_open, do_new) = ctx.input(|i| {
                let ctrl = i.modifiers.ctrl || i.modifiers.command;
                let shift = i.modifiers.shift;
                (
                    !typing && ctrl && !shift && i.key_pressed(egui::Key::Z),
                    !typing
                        && ctrl
                        && ((!shift && i.key_pressed(egui::Key::Y))
                            || (shift && i.key_pressed(egui::Key::Z))),
                    !typing && ctrl && !shift && i.key_pressed(egui::Key::S),
                    !typing && ctrl && shift && i.key_pressed(egui::Key::S),
                    !typing && ctrl && i.key_pressed(egui::Key::O),
                    !typing && ctrl && i.key_pressed(egui::Key::N),
                )
            });
            if do_undo {
                self.undo();
            }
            if do_redo {
                self.redo();
            }
            if do_save {
                self.save_or_save_as();
            }
            if do_save_as {
                self.save_as();
            }
            if do_open {
                self.open_file_dialog_async();
            }
            if do_new {
                self.start_new_project();
            }
        }

        // Delete selected node via Delete / Backspace. Routes through the
        // confirm dialog when the user has destructive-confirmation enabled.
        let do_delete = !modal_open
            && !typing
            && ctx
                .input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
        if do_delete {
            // Selection precedence: connection > group > node. A
            // user with a wire highlighted who hits Delete probably
            // wants the wire gone; a group-selected user wants the
            // group gone. The selection helpers keep these mutually
            // exclusive so we never have to disambiguate.
            if let Some((from, to)) = self.selection.connection.clone() {
                self.push_undo("Delete connection");
                self.graph.disconnect(&from, &to);
                self.selection.connection = None;
            } else if let Some(gid) = self.selection.group {
                let is_subgraph = self
                    .visuals
                    .groups
                    .get(&gid)
                    .map(|g| g.is_subgraph)
                    .unwrap_or(false);
                if is_subgraph {
                    // SubGraphs always delete with their members —
                    // splitting the SubGraph wrapper from its inner
                    // pipeline almost never matches user intent.
                    // The full state snapshot taken by push_undo
                    // covers every inner node + connection, so undo
                    // restores the whole subgraph.
                    self.delete_subgraph_with_contents(gid);
                } else {
                    // Visual groups still get the modal: they wrap
                    // arbitrary nodes the user might want to keep.
                    self.selection.pending_group_delete = Some(gid);
                }
            } else if self.selection.node.is_some() {
                // Only ask for confirmation when the user is about to
                // tear down something with wires attached. Lone /
                // recently-dropped nodes vanish straight away — the
                // modal-on-every-Delete pattern was annoying.
                let selection: Vec<NodeId> = if !self.selection.nodes.is_empty() {
                    self.selection.nodes.iter().copied().collect()
                } else if let Some(id) = self.selection.node {
                    vec![id]
                } else {
                    Vec::new()
                };
                let has_connections = selection.iter().any(|nid| {
                    self.graph
                        .connections()
                        .iter()
                        .any(|c| c.from.node_id == *nid || c.to.node_id == *nid)
                });
                let suppressed = self
                    .settings
                    .suppressed_confirmations
                    .contains(CONFIRM_KEY_DELETE_CONNECTED_NODE);
                if has_connections && !suppressed {
                    let msg = if selection.len() > 1 {
                        format!(
                            "Delete {} nodes and disconnect all of their wires?",
                            selection.len()
                        )
                    } else {
                        "Delete this node and disconnect all of its wires?".to_string()
                    };
                    self.dialog.confirm_dialog = Some(ConfirmDialog {
                        title: "Delete node?".to_string(),
                        message: msg,
                        affirm_label: "Delete".to_string(),
                        on_affirm: ConfirmAction::DeleteSelected,
                        suppression_key: Some(CONFIRM_KEY_DELETE_CONNECTED_NODE.to_string()),
                        dont_ask_again: false,
                    });
                } else {
                    self.delete_selected_node();
                }
            }
        }
    }

    pub(crate) fn draw_shell(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top menu bar — desktop-app styling. The panel itself has no
        // inner margin so the first entry sits flush with the left
        // edge of the window. Inside the bar we zero out horizontal
        // item_spacing (entries butt up against each other, no gap)
        // and bump button_padding so each entry's hover/click hit
        // box covers the full vertical span of the bar instead of
        // tightly hugging the text.
        egui::TopBottomPanel::top("menu_bar")
            .frame(
                egui::Frame::default()
                    .fill(ctx.style().visuals.panel_fill)
                    // Asymmetric vertical: button_padding is
                    // symmetric (one Vec2.y for both edges), so we
                    // get the smaller "top" amount from
                    // button_padding and add the extra bottom
                    // distance via the panel's inner margin. Net
                    // effect: 5 px above the text, ~6.7 px below.
                    .inner_margin(egui::Margin {
                        left: 0,
                        right: 0,
                        top: 0,
                        bottom: 2,
                    }),
            )
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    // `menu::bar` resets spacing/button_padding on its
                    // internal Ui — these overrides have to live INSIDE
                    // the closure to survive that reset.
                    //
                    // Asymmetric top/bottom: button_padding.y = 5 puts
                    // 5 px above and 5 px below the text inside each
                    // entry's rect (so hover highlights cover both
                    // bands). The extra ~1.7 px of bottom margin lives
                    // on the panel itself (above) — totals 5 above,
                    // ~6.7 below.
                    //
                    // Symmetric left/right at 7.78 px: the hover rect
                    // fully owns the padding on both sides, adjacent
                    // entries butt edge-to-edge with no panel-fill
                    // strip between them.
                    ui.style_mut().spacing.button_padding = egui::vec2(7.78, 5.0);
                    ui.style_mut().spacing.item_spacing.x = 0.0;
                    let v = &mut ui.style_mut().visuals;
                    v.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(60, 70, 90);
                    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(60, 70, 90);
                    v.widgets.active.weak_bg_fill = egui::Color32::from_rgb(80, 105, 145);
                    v.widgets.active.bg_fill = egui::Color32::from_rgb(80, 105, 145);
                    v.widgets.open.weak_bg_fill = egui::Color32::from_rgb(80, 105, 145);
                    v.widgets.open.bg_fill = egui::Color32::from_rgb(80, 105, 145);
                    // Square corners so adjacent entries look like one
                    // continuous strip rather than rounded chips.
                    v.widgets.hovered.corner_radius = egui::CornerRadius::ZERO;
                    v.widgets.active.corner_radius = egui::CornerRadius::ZERO;
                    v.widgets.open.corner_radius = egui::CornerRadius::ZERO;
                    ui.menu_button(t!("editor.menu.file"), |ui| {
                        // Submenu min width keeps label and shortcut
                        // text from crowding even when the localised
                        // label runs longer than English. Applied at
                        // every top-level submenu below; nested submenus
                        // (Open Recent, New from Preset) get their own
                        // smaller `set_min_width` since their entries
                        // tend to be shorter.
                        ui.set_min_width(320.0);
                        if ui
                            .add(
                                egui::Button::new(t!("editor.menu.new_project"))
                                    .shortcut_text("Ctrl+N"),
                            )
                            .clicked()
                        {
                            self.start_new_project();
                            ui.close_menu();
                        }
                        let mut macro_to_load: Option<String> = None;
                        ui.menu_button(t!("editor.menu.new_from_preset"), |ui| {
                            ui.set_min_width(180.0);
                            for group in crate::macros::BUILTIN_MACRO_GROUPS {
                                ui.menu_button(group.name, |ui| {
                                    ui.set_min_width(140.0);
                                    for entry in group.entries {
                                        if ui.button(entry.display_name).clicked() {
                                            macro_to_load = Some(entry.full_name.to_string());
                                            ui.close_menu();
                                        }
                                    }
                                });
                            }
                        });
                        if let Some(name) = macro_to_load {
                            self.start_load_macro(&name);
                        }
                        ui.separator();
                        if ui
                            .add(egui::Button::new(t!("editor.menu.open")).shortcut_text("Ctrl+O"))
                            .clicked()
                        {
                            self.open_file_dialog_async();
                            ui.close_menu();
                        }
                        let mut recent_pick: Option<std::path::PathBuf> = None;
                        let recent_empty = self.settings.recent_files.is_empty();
                        ui.add_enabled_ui(!recent_empty, |ui| {
                            ui.menu_button(t!("editor.menu.open_recent"), |ui| {
                                ui.set_min_width(280.0);
                                for p in self.settings.recent_files.iter() {
                                    let label = p
                                        .file_name()
                                        .map(|s| s.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| p.display().to_string());
                                    let parent = p
                                        .parent()
                                        .map(|s| s.display().to_string())
                                        .unwrap_or_default();
                                    let response = ui.button(&label).on_hover_text(&parent);
                                    if response.clicked() {
                                        recent_pick = Some(p.clone());
                                        ui.close_menu();
                                    }
                                }
                                ui.separator();
                                if ui.button(t!("editor.menu.clear_recent")).clicked() {
                                    self.settings.recent_files.clear();
                                    self.settings.save();
                                    ui.close_menu();
                                }
                            });
                        });
                        if let Some(p) = recent_pick {
                            self.start_open_path(p);
                        }
                        ui.separator();
                        let in_project = self.has_project();
                        if ui
                            .add_enabled(
                                in_project,
                                egui::Button::new(t!("editor.menu.save_project"))
                                    .shortcut_text("Ctrl+S"),
                            )
                            .clicked()
                        {
                            self.save_or_save_as();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                in_project,
                                egui::Button::new(t!("editor.menu.save_project_as"))
                                    .shortcut_text("Ctrl+Shift+S"),
                            )
                            .clicked()
                        {
                            self.save_as();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button(t!("editor.menu.exit")).clicked() {
                            // Route through the dirty-check path; don't slam the
                            // window shut on unsaved work.
                            self.request_close();
                            ui.close_menu();
                        }
                    });
                    ui.menu_button(t!("editor.menu.edit"), |ui| {
                        ui.set_min_width(320.0);
                        let undo_label = if self.history.can_undo() {
                            format!("{} ({})", t!("editor.menu.undo"), self.history.undo_depth())
                        } else {
                            t!("editor.menu.undo").to_string()
                        };
                        if ui
                            .add_enabled(
                                self.history.can_undo(),
                                egui::Button::new(undo_label).shortcut_text("Ctrl+Z"),
                            )
                            .clicked()
                        {
                            self.undo();
                            ui.close_menu();
                        }
                        let redo_label = if self.history.can_redo() {
                            format!("{} ({})", t!("editor.menu.redo"), self.history.redo_depth())
                        } else {
                            t!("editor.menu.redo").to_string()
                        };
                        if ui
                            .add_enabled(
                                self.history.can_redo(),
                                egui::Button::new(redo_label).shortcut_text("Ctrl+Shift+Z"),
                            )
                            .clicked()
                        {
                            self.redo();
                            ui.close_menu();
                        }
                        ui.separator();
                        // Auto Layout — disabled when there's no
                        // project (nothing to lay out).
                        if ui
                            .add_enabled(
                                self.has_project(),
                                egui::Button::new(t!("editor.menu.auto_layout")),
                            )
                            .clicked()
                        {
                            self.auto_layout_selection();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button(t!("editor.menu.preferences")).clicked() {
                            self.dialog.show_settings = true;
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("View", |ui| {
                        ui.set_min_width(220.0);
                        let has_proj = self.has_project();
                        if ui
                            .add_enabled(
                                has_proj,
                                egui::SelectableLabel::new(
                                    has_proj && self.active_layout == Layout::Standard,
                                    "Node Graph",
                                ),
                            )
                            .clicked()
                        {
                            self.set_active_layout(Layout::Standard);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                has_proj,
                                egui::SelectableLabel::new(
                                    has_proj && self.active_layout == Layout::Sculpt3D,
                                    "3D Sculpt",
                                ),
                            )
                            .clicked()
                        {
                            self.set_active_layout(Layout::Sculpt3D);
                            ui.close_menu();
                        }
                    });
                    ui.menu_button(t!("editor.menu.help"), |ui| {
                        ui.set_min_width(280.0);
                        if ui.button(t!("editor.app.about")).clicked() {
                            self.dialog.show_about = true;
                            ui.close_menu();
                        }
                    });
                });
            });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Clickable map size opens the unified Map Settings modal
                // (the same one the toolbar's "Edit Map Info" opens), so
                // dimensions and the rest of the map metadata live in one
                // place instead of a separate side dialog.
                if ui
                    .small_button(format!("Map: {}×{}", self.map.width, self.map.height))
                    .on_hover_text(t!("editor.status.open_map_settings"))
                    .clicked()
                {
                    self.dialog.show_mapinfo_editor = !self.dialog.show_mapinfo_editor;
                }
                ui.separator();
                if let Some(ref msg) = self.dialog.status_message {
                    ui.colored_label(tokens::PORT_HEIGHTMAP, msg);
                } else if let Some(id) = self.selection.node {
                    ui.label(format!("Selected: {:?}", id));
                } else {
                    ui.label("No selection");
                }
            });
        });

        // Properties no longer live in a permanent right-side panel —
        // they pop up next to the selected node / group in a floating
        // panel that opens after a short hover-after-click delay,
        // and closes on click-outside. See `tick_props_panel`.
        self.tick_props_panel(ctx);

        // ── Modal: unsaved-changes prompt ────────────────────────────────────
        if let Some(action) = self.dialog.pending_action.clone() {
            let mut close = false;
            let mut decision: Option<UnsavedDecision> = None;
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    let action_label = match &action {
                        PendingAction::Close => "close BAR - Map Editor",
                        PendingAction::NewProject => "start a new project",
                        PendingAction::OpenPath(_) => "open this file",
                        PendingAction::LoadMacro { .. } => "load this preset",
                    };
                    ui.label(format!(
                        "Your project has unsaved changes. Save before you {action_label}?"
                    ));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            decision = Some(UnsavedDecision::Save);
                        }
                        if ui.button("Discard").clicked() {
                            decision = Some(UnsavedDecision::Discard);
                        }
                        if ui.button("Cancel").clicked() {
                            decision = Some(UnsavedDecision::Cancel);
                        }
                    });
                });
            if let Some(d) = decision {
                close = true;
                match d {
                    UnsavedDecision::Save => {
                        self.save_or_save_as();
                        // If the save succeeded, is_dirty is now false; if the
                        // user cancelled the Save As dialog it's still true and
                        // we keep the prompt open.
                        if !self.project.is_dirty {
                            self.apply_pending_action(action);
                        } else {
                            close = false;
                        }
                    }
                    UnsavedDecision::Discard => {
                        // Skip dirty check; force-apply.
                        self.project.is_dirty = false;
                        self.apply_pending_action(action);
                    }
                    UnsavedDecision::Cancel => {
                        // Just dismiss — keep editing.
                    }
                }
            }
            if close {
                self.dialog.pending_action = None;
            }
        }

        // ── Modal: group delete (three-way: keep nodes / delete all / cancel) ─
        if let Some(gid) = self.selection.pending_group_delete {
            let label = self
                .visuals
                .groups
                .get(&gid)
                .map(|g| {
                    if g.label.is_empty() {
                        format!("Group {gid}")
                    } else {
                        g.label.clone()
                    }
                })
                .unwrap_or_else(|| format!("Group {gid}"));
            let member_count = self
                .visuals
                .groups
                .get(&gid)
                .map(|g| g.member_ids.len())
                .unwrap_or(0);
            let mut decision: Option<GroupDeleteChoice> = None;
            egui::Window::new(format!("Delete '{label}'?"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(format!(
                        "This group contains {member_count} node(s). What should happen to them?"
                    ));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Delete group only").clicked() {
                            decision = Some(GroupDeleteChoice::GroupOnly);
                        }
                        if ui.button("Delete group and its nodes").clicked() {
                            decision = Some(GroupDeleteChoice::GroupAndMembers);
                        }
                        if ui.button("Cancel").clicked() {
                            decision = Some(GroupDeleteChoice::Cancel);
                        }
                    });
                });
            if let Some(choice) = decision {
                self.selection.pending_group_delete = None;
                match choice {
                    GroupDeleteChoice::GroupOnly => {
                        self.push_undo("Dissolve group");
                        self.dissolve_group(gid);
                        if self.selection.group == Some(gid) {
                            self.selection.group = None;
                        }
                    }
                    GroupDeleteChoice::GroupAndMembers => {
                        // Push once for the whole "delete group + nodes"
                        // action so undo treats it atomically. The
                        // delete_selected_node path below would push
                        // its own undo entry; suppress that by
                        // stashing the snapshot here.
                        self.push_undo("Delete group with members");
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
                        self.selection.nodes = members.iter().copied().collect();
                        self.selection.node = members.first().copied();
                        // Delete nodes inline (don't go through
                        // delete_selected_node, which would push another
                        // undo and split the action).
                        let to_delete: Vec<NodeId> = self.selection.nodes.iter().copied().collect();
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
                    GroupDeleteChoice::Cancel => {}
                }
            }
        }

        // ── Modal: generic confirm (delete, etc.) ────────────────────────────
        if let Some(mut dialog) = self.dialog.confirm_dialog.clone() {
            let mut decision: Option<bool> = None;
            egui::Window::new(&dialog.title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(&dialog.message);
                    if dialog.suppression_key.is_some() {
                        ui.add_space(6.0);
                        ui.checkbox(&mut dialog.dont_ask_again, "Don't ask again");
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(&dialog.affirm_label).clicked() {
                            decision = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            decision = Some(false);
                        }
                    });
                });
            if let Some(affirm) = decision {
                // If the user ticked "Don't ask again" while
                // confirming, add this modal's key to the suppressed
                // set and persist. Suppression is per-key: this
                // affects only this modal type, not other confirms.
                if affirm && dialog.dont_ask_again {
                    if let Some(key) = dialog.suppression_key.as_ref() {
                        self.settings.suppressed_confirmations.insert(key.clone());
                        self.settings.save();
                    }
                }
                self.dialog.confirm_dialog = None;
                if affirm {
                    match dialog.on_affirm {
                        ConfirmAction::DeleteSelected => self.delete_selected_node(),
                    }
                }
            }
        }

        // ── Modal: Preferences ───────────────────────────────────────────────
        crate::panels::dialogs::draw_settings(self, ctx);

        // ── Modal: Edit Map Info picker ──────────────────────────────────────
        if self.dialog.show_map_info_picker {
            let candidates = collect_all_passthrough_files(&self.graph);
            // Heuristic: text files first, with .lua nudged to the top so
            // mapinfo.lua appears at the obvious spot for BAR/Spring users.
            let mut sorted = candidates.clone();
            sorted.sort_by_key(|(_, archive)| {
                let lua = archive.to_lowercase().ends_with("mapinfo.lua");
                let text = is_text_file(archive);
                (!lua, !text, archive.clone())
            });

            let mut open = self.dialog.show_map_info_picker;
            let mut chosen: Option<(String, String)> = None;
            let mut cleared = false;
            egui::Window::new("Choose map info file")
                .open(&mut open)
                .resizable(true)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    if sorted.is_empty() {
                        ui.label(
                            "No passthrough files in this project. Open or import a map \
                             with a mapinfo.lua first.",
                        );
                    } else {
                        ui.label("Pick the file that holds this project's map configuration:");
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical()
                            .max_height(280.0)
                            .show(ui, |ui| {
                                for (abs, archive) in &sorted {
                                    let label_text = if is_text_file(archive) {
                                        archive.clone()
                                    } else {
                                        format!("{archive} (binary — won't open in text editor)")
                                    };
                                    if ui.button(label_text).on_hover_text(abs).clicked() {
                                        chosen = Some((abs.clone(), archive.clone()));
                                    }
                                }
                            });
                    }
                    ui.add_space(8.0);
                    if self.project.map_info_file.is_some()
                        && ui.button("Clear current selection").clicked()
                    {
                        cleared = true;
                    }
                });
            self.dialog.show_map_info_picker = open;
            if cleared {
                self.project.map_info_file = None;
                self.project.is_dirty = true;
                self.dialog.show_map_info_picker = false;
            }
            if let Some((abs, archive)) = chosen {
                self.project.map_info_file = Some(archive.clone());
                self.project.is_dirty = true;
                self.dialog.show_map_info_picker = false;
                self.open_file_editor(abs, archive);
            }
        }

        // ── Modal: in-app file editor ────────────────────────────────────────
        if self.dialog.file_editor.is_some() {
            let mut save_request = false;
            let mut close_request = false;
            // Take ownership briefly so we can borrow the editor mutably for
            // the text widget while still calling self.* methods after.
            let mut editor = self.dialog.file_editor.take().expect("checked Some above");
            let dirty_marker = if editor.is_dirty { " *" } else { "" };
            let title = format!("Edit — {}{}", editor.archive_path, dirty_marker);
            let mut open = true;
            egui::Window::new(title)
                .id(egui::Id::new("file_editor_window"))
                .resizable(true)
                .collapsible(false)
                .default_size(egui::vec2(640.0, 480.0))
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.weak(&editor.abs_path);
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            let resp = ui.add_sized(
                                ui.available_size() - egui::vec2(0.0, 32.0),
                                egui::TextEdit::multiline(&mut editor.content)
                                    .code_editor()
                                    .desired_width(f32::INFINITY),
                            );
                            if resp.changed() {
                                editor.is_dirty = true;
                            }
                        });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(editor.is_dirty, egui::Button::new("Save"))
                            .clicked()
                        {
                            save_request = true;
                        }
                        if ui.button("Close").clicked() {
                            close_request = true;
                        }
                    });
                });

            // The X-button on the window translates to !open; treat it as Close.
            if !open {
                close_request = true;
            }

            if save_request {
                match std::fs::write(&editor.abs_path, &editor.content) {
                    Ok(()) => {
                        editor.is_dirty = false;
                        self.dialog.status_message = Some(format!("Saved {}", editor.archive_path));
                    }
                    Err(e) => {
                        self.dialog.status_message = Some(format!("Save failed: {e}"));
                    }
                }
            }

            if close_request {
                // If unsaved, drop the changes silently for now — user explicitly
                // dismissed. (We could prompt later if this becomes a footgun.)
                self.dialog.file_editor = None;
            } else {
                self.dialog.file_editor = Some(editor);
            }
        }

        // ── Modal: About ─────────────────────────────────────────────────────
        crate::panels::dialogs::draw_about(self, ctx);

        if self.dialog.show_inspector {
            self.draw_inspector_window(ctx);
        }

        if self.dialog.show_mapinfo_editor {
            self.draw_mapinfo_editor_window(ctx);
        }

        crate::panels::validation::draw_details(self, ctx);

        // Action bar -- only shown inside a project.
        if self.has_project() {
            egui::TopBottomPanel::top("action_bar").show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(37.0, 30.0);
                    let busy = self.preview.export_status == ExportStatus::All;
                    let any_running = self.preview.export_status.is_running();
                    let sense = if any_running {
                        egui::Sense::hover()
                    } else {
                        egui::Sense::click()
                    };
                    let (rect, response) = ui.allocate_exact_size(btn_size, sense);

                    if ui.is_rect_visible(rect) {
                        let bg = if busy {
                            tokens::BTN_EXPORT_BUSY
                        } else if any_running {
                            tokens::BTN_EXPORT_BLOCKED
                        } else if response.is_pointer_button_down_on() {
                            tokens::BTN_EXPORT_PRESS
                        } else if response.hovered() {
                            tokens::BTN_EXPORT_HOVER
                        } else {
                            tokens::BTN_EXPORT_NORMAL
                        };
                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 5.0, bg);
                        paint_export_icon(&painter, rect, egui::Color32::WHITE);
                        if busy {
                            // Tiny corner spinner so the busy state reads clearly.
                            paint_busy_dot(&painter, rect, ui.input(|i| i.time));
                        }
                    }

                    let tooltip = if busy {
                        "Exporting…"
                    } else if any_running {
                        "Another export is running"
                    } else {
                        "Export all Bundler nodes"
                    };
                    let response = response.on_hover_text(tooltip);
                    if !any_running
                        && response.clicked()
                        && self.validate_before_export("Bundle all")
                    {
                        self.preview.run_requested = true;
                    }

                    // Edit Map Info button — opens the project's designated map
                    // info file in the OS default editor. Prompts for the file
                    // on first use.
                    ui.add_space(4.0);
                    let (info_rect, info_resp) =
                        ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(info_rect) {
                        let bg = if info_resp.is_pointer_button_down_on() {
                            tokens::BTN_MAPINFO_PRESS
                        } else if info_resp.hovered() {
                            tokens::BTN_MAPINFO_HOVER
                        } else {
                            tokens::BTN_MAPINFO_NORMAL
                        };
                        let painter = ui.painter_at(info_rect);
                        painter.rect_filled(info_rect, 5.0, bg);
                        paint_map_info_icon(&painter, info_rect, egui::Color32::WHITE);
                    }
                    let info_resp = info_resp.on_hover_text(
                        "Edit Map Info — open the project's map info file (e.g. mapinfo.lua)",
                    );
                    if info_resp.clicked() {
                        self.handle_edit_map_info_clicked();
                    }

                    // Test in BAR — export the current project, copy the .sd7
                    // into BAR's maps directory, open the lobby. The user
                    // navigates to skirmish from there. Greyed out while an
                    // export is already running so we don't double-fire.
                    ui.add_space(4.0);
                    let (bar_rect, bar_resp) =
                        ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(bar_rect) {
                        let bg = if any_running {
                            tokens::BTN_BAR_BLOCKED
                        } else if bar_resp.is_pointer_button_down_on() {
                            tokens::BTN_BAR_PRESS
                        } else if bar_resp.hovered() {
                            tokens::BTN_BAR_HOVER
                        } else {
                            tokens::BTN_BAR_NORMAL
                        };
                        let painter = ui.painter_at(bar_rect);
                        painter.rect_filled(bar_rect, 5.0, bg);
                        paint_bar_icon(&painter, bar_rect, egui::Color32::WHITE);
                    }
                    let bar_resp = bar_resp.on_hover_text(
                        "Test in BAR — export this project and open it in the BAR lobby",
                    );
                    if !any_running && bar_resp.clicked() {
                        // Run validation first; refuse to launch if there
                        // are blocking errors so the user can't ship a
                        // broken map to BAR. Warnings are advisory and let
                        // the launch proceed.
                        self.run_validation();
                        if bar_project::has_errors(&self.validation.findings) {
                            self.dialog.show_validation_panel = true;
                            self.dialog.status_message =
                                Some("Test in BAR: fix validation errors first.".to_string());
                        } else {
                            self.preview.test_in_bar_requested = true;
                        }
                    }

                    // The toolbar Validate button used to live here. It's
                    // been removed in favour of the live Validation panel
                    // in the left sidebar (counts auto-refresh as you
                    // edit) and an automatic validation gate on the
                    // bundle / bundle-all buttons. The "Show details"
                    // button in the sidebar opens the same findings
                    // window the toolbar button used to open.

                    // 2D Inspector — top-down heightmap view with draggable
                    // start-position markers.
                    ui.add_space(4.0);
                    let (insp_rect, insp_resp) =
                        ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(insp_rect) {
                        let bg = if insp_resp.is_pointer_button_down_on() {
                            tokens::BTN_INSPECTOR_PRESS
                        } else if insp_resp.hovered() {
                            tokens::BTN_INSPECTOR_HOVER
                        } else {
                            tokens::BTN_INSPECTOR_NORMAL
                        };
                        let painter = ui.painter_at(insp_rect);
                        painter.rect_filled(insp_rect, 5.0, bg);
                        paint_inspector_icon(&painter, insp_rect, egui::Color32::WHITE);
                    }
                    let insp_resp = insp_resp
                        .on_hover_text("2D Inspector — top-down map view, place start positions");
                    if insp_resp.clicked() {
                        self.dialog.show_inspector = !self.dialog.show_inspector;
                    }

                    // Structured Map Info editor — form for atmosphere /
                    // lighting / water / physics / heights. The Edit Map
                    // Info button (pencil icon, opens raw lua) stays for
                    // power users; this is the friendly path.
                    ui.add_space(4.0);
                    let (mi_rect, mi_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(mi_rect) {
                        let bg = if mi_resp.is_pointer_button_down_on() {
                            tokens::BTN_MAPSET_PRESS
                        } else if mi_resp.hovered() {
                            tokens::BTN_MAPSET_HOVER
                        } else {
                            tokens::BTN_MAPSET_NORMAL
                        };
                        let painter = ui.painter_at(mi_rect);
                        painter.rect_filled(mi_rect, 5.0, bg);
                        paint_mapinfo_form_icon(&painter, mi_rect, egui::Color32::WHITE);
                    }
                    let mi_resp = mi_resp.on_hover_text(t!("editor.toolbar.map_settings"));
                    if mi_resp.clicked() {
                        self.dialog.show_mapinfo_editor = !self.dialog.show_mapinfo_editor;
                    }

                    // Startboxes — opens the 2D inspector at Spawns mode so
                    // the user can drag spawn markers. Lives in its own
                    // button (rather than a tab inside Map Settings) because
                    // box-authoring is a spatial task that wants the full
                    // inspector canvas, not a side-panel form.
                    ui.add_space(4.0);
                    let (sb_rect, sb_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(sb_rect) {
                        let bg = if sb_resp.is_pointer_button_down_on() {
                            tokens::BTN_SPAWNS_PRESS
                        } else if sb_resp.hovered() {
                            tokens::BTN_SPAWNS_HOVER
                        } else {
                            tokens::BTN_SPAWNS_NORMAL
                        };
                        let painter = ui.painter_at(sb_rect);
                        painter.rect_filled(sb_rect, 5.0, bg);
                        paint_startbox_icon(&painter, sb_rect, egui::Color32::WHITE);
                    }
                    let sb_resp = sb_resp.on_hover_text(t!("editor.toolbar.startboxes"));
                    if sb_resp.clicked() {
                        self.dialog.show_inspector = true;
                        self.paint.inspector_mode = InspectorMode::Spawns;
                    }
                });
                ui.add_space(4.0);
            });
        }

        // ── Toast notification (e.g. "Autosaved …") ─────────────────────────
        if let Some((msg, _)) = self.dialog.toast.clone() {
            let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("toast"));
            let painter = ctx.layer_painter(layer);
            let screen = ctx.screen_rect();
            let font = egui::FontId::proportional(13.0);
            let text_color = egui::Color32::from_rgb(220, 230, 230);
            let pad = egui::vec2(14.0, 8.0);
            let galley = painter.layout_no_wrap(msg.clone(), font.clone(), text_color);
            let size = galley.size() + pad * 2.0;
            // Bottom-center, lifted 30 px above the status bar.
            let center = egui::pos2(screen.center().x, screen.bottom() - size.y / 2.0 - 50.0);
            let rect = egui::Rect::from_center_size(center, size);
            painter.rect_filled(rect, 6.0, egui::Color32::from_black_alpha(210));
            painter.rect_stroke(
                rect,
                6.0,
                egui::Stroke::new(1.0, tokens::BTN_INSPECTOR_HOVER),
                egui::StrokeKind::Outside,
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                &msg,
                font,
                text_color,
            );
            // Toast expires on its own; request a repaint so the timer ticks.
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    pub(crate) fn draw_node_palette_panel(&mut self, ctx: &egui::Context) {
        if self.has_project() {
            egui::SidePanel::left("node_palette")
                .default_width(200.0)
                .show(ctx, |ui| {
                    // Validation summary anchors to the bottom of the
                    // sidebar; the node palette fills everything above it.
                    // Override the default panel frame so the summary's
                    // left/right padding lines up with the palette items
                    // above (default frame adds 8px asymmetric margins).
                    let frame = {
                        let mut f = egui::Frame::side_top_panel(ui.style());
                        f.inner_margin = egui::Margin {
                            left: 4,
                            right: 4,
                            top: 6,
                            bottom: 6,
                        };
                        f
                    };
                    egui::TopBottomPanel::bottom("validation_summary")
                        .resizable(false)
                        .frame(frame)
                        .show_inside(ui, |ui| {
                            self.draw_validation_summary(ui);
                        });
                    self.draw_node_palette(ui);
                });
        }
    }

    pub(crate) fn draw_standard_central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_node_graph(ui);
        });

        // ── Palette drag: ghost preview + drop/cancel ────────────────────────────────
        // This runs AFTER all panels so the ghost paints on top of everything and
        // pointer position reflects the final state of the frame.

        if let Some(ref drag) = self.palette_drag {
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);

            if let Some(pos) = ctx.pointer_latest_pos() {
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("palette_drag_ghost"),
                ));
                // IO nodes drop at their tag size so the ghost
                // matches; other node types use the generic 150×60
                // preview rect.
                let is_io_input = matches!(drag.kind, PaletteKind::Node(NodeType::SubgraphInput));
                let is_io_output = matches!(drag.kind, PaletteKind::Node(NodeType::SubgraphOutput));
                let is_io = is_io_input || is_io_output;
                let ghost_size = if is_io {
                    IO_NODE_SIZE
                } else {
                    egui::vec2(150.0, 60.0)
                };
                let ghost_rect =
                    egui::Rect::from_min_size(pos + egui::vec2(10.0, 10.0), ghost_size);
                let is_over_canvas =
                    self.canvas.rect_last.is_positive() && self.canvas.rect_last.contains(pos);
                let border_col = if is_over_canvas {
                    egui::Color32::from_rgba_unmultiplied(100, 200, 100, 220)
                } else {
                    egui::Color32::from_rgba_unmultiplied(220, 80, 80, 220)
                };

                if is_io {
                    // Match the on-canvas IO render: chevron-tipped
                    // tag with two-line text and a directional icon.
                    let h = ghost_rect.height();
                    let scale = h / IO_REF_H;
                    let chevron_w = h * 0.30;
                    let body_radius = (h / 6.0).min(ghost_rect.width() / 4.0);
                    let inner_pad = 6.0 * scale;
                    let icon_size = 48.0 * scale;
                    let icon_text_gap = 8.0 * scale;
                    let top_text_size = 18.0 * scale;
                    let bottom_text_size = 15.0 * scale;
                    let mid_y = ghost_rect.center().y;
                    let body_color = egui::Color32::from_rgba_unmultiplied(0x2F, 0x39, 0x45, 220);
                    let outline_pts =
                        build_io_outline(ghost_rect, chevron_w, body_radius, is_io_input);
                    painter.add(egui::Shape::convex_polygon(
                        outline_pts,
                        body_color,
                        egui::Stroke::new(1.5, border_col),
                    ));
                    let icon_rect = if is_io_input {
                        egui::Rect::from_min_size(
                            egui::pos2(ghost_rect.left() + inner_pad, mid_y - icon_size / 2.0),
                            egui::vec2(icon_size, icon_size),
                        )
                    } else {
                        egui::Rect::from_min_size(
                            egui::pos2(
                                ghost_rect.right() - inner_pad - icon_size,
                                mid_y - icon_size / 2.0,
                            ),
                            egui::vec2(icon_size, icon_size),
                        )
                    };
                    draw_io_icon(&painter, icon_rect, is_io_input);
                    let top_text = if is_io_input { "Input" } else { "Output" };
                    let bottom_text = "Heightmap";
                    let text_left = if is_io_input {
                        icon_rect.right() + icon_text_gap
                    } else {
                        ghost_rect.left() + chevron_w + inner_pad
                    };
                    let line_gap = 6.0 * scale;
                    let stack_h = top_text_size + line_gap + bottom_text_size;
                    let text_top = mid_y - stack_h / 2.0;
                    painter.text(
                        egui::pos2(text_left, text_top),
                        egui::Align2::LEFT_TOP,
                        top_text,
                        egui::FontId::proportional(top_text_size),
                        egui::Color32::from_rgb(0xE6, 0xED, 0xF3),
                    );
                    painter.text(
                        egui::pos2(text_left, text_top + top_text_size + line_gap),
                        egui::Align2::LEFT_TOP,
                        bottom_text,
                        egui::FontId::proportional(bottom_text_size),
                        egui::Color32::from_rgb(0x9A, 0xA7, 0xB2),
                    );
                } else {
                    // Generic node ghost: rect body + title bar.
                    painter.rect_filled(
                        ghost_rect,
                        4.0,
                        egui::Color32::from_rgba_unmultiplied(45, 50, 60, 200),
                    );
                    painter.rect_stroke(
                        ghost_rect,
                        4.0,
                        egui::Stroke::new(1.5, border_col),
                        egui::StrokeKind::Outside,
                    );
                    let title_rect = egui::Rect::from_min_size(
                        ghost_rect.min,
                        egui::vec2(ghost_rect.width(), 20.0),
                    );
                    let title_color = match &drag.kind {
                        PaletteKind::Node(t) => node_type_color(t),
                        PaletteKind::Macro { .. } => egui::Color32::from_rgb(180, 90, 200),
                    };
                    painter.rect_filled(
                        title_rect,
                        egui::CornerRadius {
                            nw: 4,
                            ne: 4,
                            sw: 0,
                            se: 0,
                        },
                        title_color,
                    );
                    painter.text(
                        title_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        &drag.label,
                        egui::FontId::proportional(12.0),
                        egui::Color32::WHITE,
                    );
                }
                // Cancel hint when not over canvas
                if !is_over_canvas {
                    painter.text(
                        egui::pos2(ghost_rect.center().x, ghost_rect.center().y + 6.0),
                        egui::Align2::CENTER_CENTER,
                        "✕",
                        egui::FontId::proportional(18.0),
                        egui::Color32::from_rgb(220, 80, 80),
                    );
                }
            }

            ctx.request_repaint();
        }

        // Handle drop on primary pointer release
        let released = ctx.input(|i| i.pointer.primary_released());
        if released && self.palette_drag.is_some() {
            if let Some(drag) = self.palette_drag.take() {
                if let Some(pos) = ctx.pointer_latest_pos() {
                    if self.canvas.rect_last.is_positive() && self.canvas.rect_last.contains(pos) {
                        // Convert screen position → graph-space (accounts for canvas pan)
                        let graph_pos = pos - self.canvas.offset;
                        let drop_at = egui::pos2(graph_pos.x, graph_pos.y);
                        match drag.kind {
                            PaletteKind::Node(t) => {
                                self.add_node_at(t, &drag.label, drop_at);
                            }
                            PaletteKind::Macro { name } => {
                                self.instantiate_macro(&name, drop_at);
                            }
                        }
                    }
                    // else: released outside canvas → cancel
                }
            }
        }
    }
}

/// Minimum distance from a point to a polyline (segment-by-segment).
/// Used by wire hit-testing on the canvas — bezier wires are pre-
/// flattened to a 21-point polyline before drawing, so we can reuse
/// that polyline as the hit shape.
pub(crate) fn polyline_distance(p: egui::Pos2, points: &[egui::Pos2]) -> f32 {
    if points.len() < 2 {
        return f32::INFINITY;
    }
    let mut best = f32::INFINITY;
    for i in 0..points.len() - 1 {
        let a = points[i];
        let b = points[i + 1];
        let ab = b - a;
        let len2 = ab.x * ab.x + ab.y * ab.y;
        let t = if len2 > 1e-6 {
            ((p - a).dot(ab)) / len2
        } else {
            0.0
        };
        let t = t.clamp(0.0, 1.0);
        let proj = egui::pos2(a.x + ab.x * t, a.y + ab.y * t);
        let d = proj.distance(p);
        if d < best {
            best = d;
        }
    }
    best
}

pub(crate) fn cubic_bezier(
    p0: egui::Pos2,
    p1: egui::Pos2,
    p2: egui::Pos2,
    p3: egui::Pos2,
    t: f32,
) -> egui::Pos2 {
    let u = 1.0 - t;
    let tt = t * t;
    let uu = u * u;
    let uuu = uu * u;
    let ttt = tt * t;

    let x = uuu * p0.x + 3.0 * uu * t * p1.x + 3.0 * u * tt * p2.x + ttt * p3.x;
    let y = uuu * p0.y + 3.0 * uu * t * p1.y + 3.0 * u * tt * p2.y + ttt * p3.y;
    egui::pos2(x, y)
}

/// Labelled drag-value for an f32 with bounds + speed. Returns true when
/// the value changed (so callers can mark the project dirty in one place).
/// Map-Settings validation findings keyed by (tab_id, field_id).
/// `tab_id` matches the lowercase form of `MapInfoTab` variants; `field_id`
/// matches the names tagged onto findings in `bar-project::validation`.
pub(crate) struct FieldFindings {
    by_field: HashMap<(String, String), bar_project::Severity>,
    by_tab: HashMap<String, bar_project::Severity>,
}

impl FieldFindings {
    pub(crate) fn from(findings: &[bar_project::Finding]) -> Self {
        let mut by_field: HashMap<(String, String), bar_project::Severity> = HashMap::new();
        let mut by_tab: HashMap<String, bar_project::Severity> = HashMap::new();
        for f in findings {
            let cat = f.category.clone();
            // Worst-severity wins per slot: error > warning > info.
            by_tab
                .entry(cat.clone())
                .and_modify(|s| *s = worst_severity(*s, f.severity))
                .or_insert(f.severity);
            if let Some(field) = f.field.as_deref() {
                by_field
                    .entry((cat, field.to_string()))
                    .and_modify(|s| *s = worst_severity(*s, f.severity))
                    .or_insert(f.severity);
            }
        }
        Self { by_field, by_tab }
    }

    pub(crate) fn tab(&self, tab: &str) -> Option<bar_project::Severity> {
        self.by_tab.get(tab).copied()
    }

    pub(crate) fn field(&self, tab: &str, field: &str) -> Option<bar_project::Severity> {
        self.by_field
            .get(&(tab.to_string(), field.to_string()))
            .copied()
    }
}

fn worst_severity(a: bar_project::Severity, b: bar_project::Severity) -> bar_project::Severity {
    use bar_project::Severity::*;
    match (a, b) {
        (Error, _) | (_, Error) => Error,
        (Warning, _) | (_, Warning) => Warning,
        _ => Info,
    }
}

pub(crate) fn severity_color(sev: bar_project::Severity) -> egui::Color32 {
    match sev {
        bar_project::Severity::Error => tokens::SEVERITY_ERROR,
        bar_project::Severity::Warning => tokens::SEVERITY_WARN,
        bar_project::Severity::Info => tokens::SEVERITY_INFO,
    }
}

/// Wrap a row in a thin coloured outline whose colour matches the
/// finding's severity. No-op when `sev` is `None`. Used to mark
/// individual map-property fields that the validator flagged.
pub(crate) fn outline_finding<R>(
    ui: &mut egui::Ui,
    sev: Option<bar_project::Severity>,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    match sev {
        Some(s) => {
            let color = severity_color(s);
            egui::Frame::default()
                .stroke(egui::Stroke::new(1.0, color))
                .corner_radius(2.0)
                .inner_margin(egui::Margin::symmetric(2, 1))
                .show(ui, body)
                .inner
        }
        None => body(ui),
    }
}

pub(crate) fn drag_f32(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    lo: f32,
    hi: f32,
    speed: f32,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    egui::DragValue::new(value)
                        .range(lo..=hi)
                        .speed(speed as f64),
                )
                .changed()
            {
                changed = true;
            }
        });
    });
    changed
}

/// Labelled drag-value for a u32 with bounds.
pub(crate) fn drag_u32(ui: &mut egui::Ui, label: &str, value: &mut u32, lo: u32, hi: u32) -> bool {
    let mut changed = false;
    let mut v = *value as i64;
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::DragValue::new(&mut v).range((lo as i64)..=(hi as i64)))
                .changed()
            {
                *value = v.max(0) as u32;
                changed = true;
            }
        });
    });
    changed
}

/// Labelled text field for an `Option<String>`. Treats the empty
/// string as `None` so the bundler-side fallback kicks in. The
/// placeholder hint communicates what that fallback will be.
/// Returns true on change.
pub(crate) fn edit_optional_string(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<String>,
    placeholder: &str,
) -> bool {
    let mut changed = false;
    let mut buf = value.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let edit = egui::TextEdit::singleline(&mut buf)
                .desired_width(220.0)
                .hint_text(placeholder);
            if ui.add(edit).changed() {
                let trimmed = buf.trim();
                let new_value = if trimmed.is_empty() {
                    None
                } else {
                    Some(buf.clone())
                };
                if &new_value != value {
                    *value = new_value;
                    changed = true;
                }
            }
        });
    });
    changed
}

/// Labelled RGB colour picker for a `[f32; 3]`. Returns true on change.
pub(crate) fn color_rgb(ui: &mut egui::Ui, label: &str, value: &mut [f32; 3]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.color_edit_button_rgb(value).changed() {
                changed = true;
            }
        });
    });
    changed
}

// Brush dab math + tests live in `crate::paint::brush_math`. Re-exported
// here under the historical names so existing callers don't break.
pub(crate) use crate::paint::brush_math::apply_brush_dab;
use crate::paint::brush_math::{stamp_color_dab_in_buffer, stamp_value_dab_in_heightmap};

pub(crate) use crate::io::png::save_heightmap_as_png16;

pub(crate) fn heightmap_to_color_image(
    hm: &bar_data::Heightmap,
    min_h: f32,
    max_h: f32,
) -> egui::ColorImage {
    let w = hm.width() as usize;
    let h = hm.height() as usize;
    let span = (max_h - min_h).max(1.0);
    let waterline_norm = if min_h < 0.0 {
        (-min_h / span).clamp(0.0, 1.0)
    } else {
        -1.0
    };
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let n = hm.get(x as u32, y as u32).unwrap_or(0.0).clamp(0.0, 1.0);
            let pixel = if waterline_norm >= 0.0 && n < waterline_norm {
                // Underwater — depth-darkened blue.
                let depth = (waterline_norm - n) / waterline_norm.max(0.001);
                let dim = (1.0 - depth * 0.6).clamp(0.3, 1.0);
                egui::Color32::from_rgb((40.0 * dim) as u8, (90.0 * dim) as u8, (160.0 * dim) as u8)
            } else {
                // Above water — gray with a subtle warm tint as elevation rises.
                let above = if waterline_norm >= 0.0 {
                    (n - waterline_norm) / (1.0 - waterline_norm).max(0.001)
                } else {
                    n
                };
                let v = (above * 220.0 + 35.0) as u8;
                let warm = (above * 25.0) as u8;
                egui::Color32::from_rgb(v.saturating_add(warm), v, v.saturating_sub(warm / 2))
            };
            pixels.push(pixel);
        }
    }
    egui::ColorImage {
        size: [w, h],
        pixels,
    }
}

// Icon painting functions moved to panels/icons.rs; re-exported above.
// Stamp helpers (stamp_color_dab_in_buffer / stamp_value_dab_in_heightmap)
// are defined in `crate::paint::brush_math` and imported above.

/// Build a file-picker dialog appropriate for the given node type's
/// `path` param. The dialog is parented to the editor window when a
/// handle has been pushed via `BarEditorApp::set_parent_window_handle`.
pub(crate) fn make_path_dialog(app: &BarEditorApp, node_type: &NodeType) -> rfd::FileDialog {
    let base = app.make_dialog();
    match node_type {
        NodeType::SmfImport => base
            .set_title("Select .smf Map File")
            .add_filter("Spring Map File", &["smf"]),
        NodeType::SmtImport => base
            .set_title("Select .smt Tile File")
            .add_filter("Spring Map Tiles", &["smt"]),
        NodeType::FileInput => base
            .set_title("Select Image File")
            .add_filter("Image", &["png", "tiff", "tif", "jpg", "jpeg"]),
        _ => base.set_title("Select File"),
    }
}

pub(crate) fn node_type_color(node_type: &NodeType) -> egui::Color32 {
    match node_type {
        NodeType::PerlinNoise
        | NodeType::SimplexNoise
        | NodeType::WorleyNoise
        | NodeType::RidgedNoise
        | NodeType::Voronoi
        | NodeType::Gradient
        | NodeType::FileInput
        | NodeType::Constant => tokens::NODE_CAT_GENERATOR,

        NodeType::HydraulicErosion
        | NodeType::ThermalErosion
        | NodeType::Blur
        | NodeType::Sharpen
        | NodeType::Clamp
        | NodeType::Terrace
        | NodeType::Invert
        | NodeType::Curve
        | NodeType::SimpleTransform
        | NodeType::Normalize
        | NodeType::BiasGain
        | NodeType::Displacement
        | NodeType::Sculpt => tokens::NODE_CAT_FILTER,

        NodeType::Blend
        | NodeType::Add
        | NodeType::Subtract
        | NodeType::Multiply
        | NodeType::Max
        | NodeType::Min
        | NodeType::Chooser => tokens::NODE_CAT_COMBINER,

        NodeType::SlopeMap
        | NodeType::HeightSelect
        | NodeType::SplatMap
        | NodeType::AutoTexture
        | NodeType::NormalMap
        | NodeType::GrassMap
        | NodeType::SpecularMap => tokens::NODE_CAT_TEXTURE,

        NodeType::Mask
        | NodeType::PaintedHeightmap
        | NodeType::PaintedTexture
        | NodeType::MaskThreshold
        | NodeType::MaskInvert
        | NodeType::MaskBlur
        | NodeType::MaskApply => tokens::NODE_CAT_MASK,

        NodeType::Bundler | NodeType::FileReference => tokens::NODE_CAT_BUNDLER,

        NodeType::SmfImport | NodeType::SmtImport | NodeType::PassThrough => {
            tokens::NODE_CAT_SOURCE
        }

        NodeType::Preview => tokens::NODE_CAT_PREVIEW,
        // Distinct dark teal — boundary markers, not generators/filters/combiners.
        NodeType::SubgraphInput | NodeType::SubgraphOutput => tokens::NODE_CAT_IO,
    }
}

pub(crate) fn port_kind_color(kind: &PortKind) -> egui::Color32 {
    match kind {
        PortKind::Heightmap => tokens::PORT_HEIGHTMAP,
        PortKind::Mask => tokens::PORT_MASK,
        PortKind::Color => tokens::PORT_COLOR,
        PortKind::Scalar => tokens::PORT_SCALAR,
        PortKind::File => tokens::PORT_FILE,
        PortKind::FileList => tokens::PORT_FILE_LIST,
        PortKind::Control => tokens::PORT_CONTROL,
        PortKind::Density => tokens::PORT_DENSITY,
    }
}

/// Build the closed convex polygon for an IO-node silhouette
/// (rounded rectangle on one side, chevron point on the other).
/// Quarter-arc corners on the rounded side are sampled into line
/// segments; `Shape::convex_polygon` then fills and strokes the
/// whole shape in one pass, so the border and fill stay in
/// register at every size. Vertices are emitted clockwise in
/// screen coordinates as `convex_polygon` requires.
pub(crate) fn build_io_outline(
    rect: egui::Rect,
    chevron_w: f32,
    body_radius: f32,
    is_input: bool,
) -> Vec<egui::Pos2> {
    use std::f32::consts::{FRAC_PI_2, PI};
    // Six segments per quarter-arc reads as smooth at typical zoom
    // levels and stays cheap (≤ 14 extra vertices per node).
    let segments = 6_usize;
    let mid_y = rect.center().y;
    // Clamp the radius so it can never exceed half the height (which
    // would make the corners overlap) or a quarter of the width
    // (so the rounded side doesn't swallow the body).
    let r = body_radius
        .min(rect.height() / 2.0)
        .min(rect.width() / 4.0)
        .max(0.0);
    let mut pts: Vec<egui::Pos2> = Vec::with_capacity(2 * (segments + 1) + 4);
    let sample_arc = |center: egui::Pos2, start: f32, end: f32, pts: &mut Vec<egui::Pos2>| {
        for i in 0..=segments {
            let t = i as f32 / segments as f32;
            let angle = start + t * (end - start);
            pts.push(egui::pos2(
                center.x + r * angle.cos(),
                center.y + r * angle.sin(),
            ));
        }
    };
    if is_input {
        // CW: top-left arc → top edge → chevron tip → bottom edge →
        // bottom-left arc → close (left edge implicit on close).
        sample_arc(
            egui::pos2(rect.left() + r, rect.top() + r),
            PI,
            PI + FRAC_PI_2,
            &mut pts,
        );
        pts.push(egui::pos2(rect.right() - chevron_w, rect.top()));
        pts.push(egui::pos2(rect.right(), mid_y));
        pts.push(egui::pos2(rect.right() - chevron_w, rect.bottom()));
        sample_arc(
            egui::pos2(rect.left() + r, rect.bottom() - r),
            FRAC_PI_2,
            PI,
            &mut pts,
        );
    } else {
        // CW: chevron tip → chevron-top → top edge → top-right arc →
        // right edge implicit → bottom-right arc → bottom edge →
        // chevron-bottom → close.
        pts.push(egui::pos2(rect.left(), mid_y));
        pts.push(egui::pos2(rect.left() + chevron_w, rect.top()));
        sample_arc(
            egui::pos2(rect.right() - r, rect.top() + r),
            PI + FRAC_PI_2,
            2.0 * PI,
            &mut pts,
        );
        sample_arc(
            egui::pos2(rect.right() - r, rect.bottom() - r),
            0.0,
            FRAC_PI_2,
            &mut pts,
        );
        pts.push(egui::pos2(rect.left() + chevron_w, rect.bottom()));
    }
    pts
}

// draw_io_icon moved to panels/icons.rs; re-exported above.

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
pub(crate) use crate::project::path::{collect_all_passthrough_files, parse_passthrough_files};

/// Lightweight directory tree for rendering a hierarchical file list.
/// Children keyed by directory name to keep ordering stable; files are stored
/// as (filename, abs_path, archive_path) tuples on the directory that
/// directly contains them.
#[derive(Default)]
pub(crate) struct PathTree {
    children: std::collections::BTreeMap<String, PathTree>,
    files: Vec<(String, String, String)>,
}

/// Build a `PathTree` from a flat list of `(abs_path, archive_path)` pairs.
pub(crate) fn build_path_tree(files: &[(String, String)]) -> PathTree {
    let mut root = PathTree::default();
    for (abs, archive) in files {
        // archive_path uses forward slashes (validate_bundle_path enforces it).
        let parts: Vec<&str> = archive.split('/').collect();
        let (dirs, file_name) = match parts.split_last() {
            Some((last, dirs)) => (dirs, last.to_string()),
            None => continue,
        };
        let mut node = &mut root;
        for d in dirs {
            if d.is_empty() {
                continue;
            }
            node = node.children.entry((*d).to_string()).or_default();
        }
        node.files.push((file_name, abs.clone(), archive.clone()));
    }
    root
}

/// Recursively render a `PathTree` as nested collapsing headers.
/// `edit_request` is set when the user clicks an edit button next to a file.
pub(crate) fn draw_path_tree(
    ui: &mut egui::Ui,
    tree: &PathTree,
    depth: usize,
    edit_request: &mut Option<(String, String)>,
) {
    // Render directories first, then loose files at this level.
    for (dir_name, child) in &tree.children {
        let id = ui.make_persistent_id(("pt_dir", depth, dir_name));
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
            .show_header(ui, |ui| {
                ui.label(egui::RichText::new(format!("📁 {}", dir_name)).strong());
            })
            .body(|ui| {
                draw_path_tree(ui, child, depth + 1, edit_request);
            });
    }
    for (file_name, abs, archive) in &tree.files {
        ui.horizontal(|ui| {
            ui.label(file_name).on_hover_text(archive.as_str());
            // Right-align the edit button by filling the rest of the row.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if is_text_file(archive)
                    && ui.small_button("✏").on_hover_text("Edit file").clicked()
                {
                    *edit_request = Some((abs.clone(), archive.clone()));
                }
            });
        });
    }
}

/// Render a PassThrough node's file hierarchy directly onto the canvas using the painter.
/// Files are grouped by parent directory and clipped to the node body area.
pub(crate) fn draw_passthrough_body(
    painter: &egui::Painter,
    node_rect: egui::Rect,
    files: &[(String, String)],
) {
    let body_top = node_rect.min.y + 24.0;
    let body_bottom = node_rect.max.y - 4.0;
    let body_left = node_rect.min.x + 6.0;
    let line_height = 13.0;
    let text_color = egui::Color32::from_rgb(190, 190, 190);
    let dir_color = egui::Color32::from_rgb(140, 190, 255);

    let clip_rect = egui::Rect::from_min_max(
        egui::pos2(node_rect.min.x, body_top),
        egui::pos2(node_rect.max.x, body_bottom),
    );
    let p = painter.with_clip_rect(clip_rect);

    if files.is_empty() {
        p.text(
            egui::pos2(body_left, body_top + 2.0),
            egui::Align2::LEFT_TOP,
            "No files",
            egui::FontId::proportional(10.0),
            egui::Color32::GRAY,
        );
        return;
    }

    // Group files by parent directory (preserving stable order via BTreeMap)
    let mut dirs: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (_, rel) in files {
        let path = std::path::Path::new(rel.as_str());
        let dir = path
            .parent()
            .map(|d| d.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| rel.clone());
        dirs.entry(dir).or_default().push(name);
    }

    let mut y = body_top;
    'outer: for (dir, names) in &dirs {
        if y + line_height > body_bottom {
            p.text(
                egui::pos2(body_left, y),
                egui::Align2::LEFT_TOP,
                "…",
                egui::FontId::monospace(10.0),
                text_color,
            );
            break;
        }
        if !dir.is_empty() {
            p.text(
                egui::pos2(body_left, y),
                egui::Align2::LEFT_TOP,
                format!("▸ {}/", dir),
                egui::FontId::monospace(10.0),
                dir_color,
            );
            y += line_height;
        }
        let indent = if dir.is_empty() {
            body_left
        } else {
            body_left + 8.0
        };
        for name in names {
            if y + line_height > body_bottom {
                p.text(
                    egui::pos2(indent, y),
                    egui::Align2::LEFT_TOP,
                    "…",
                    egui::FontId::monospace(10.0),
                    text_color,
                );
                break 'outer;
            }
            p.text(
                egui::pos2(indent, y),
                egui::Align2::LEFT_TOP,
                name.as_str(),
                egui::FontId::monospace(10.0),
                text_color,
            );
            y += line_height;
        }
    }
}

// brush_tests module relocated to `crate::paint::brush_math`.
// session_reset_tests module relocated to `crate::project::lifecycle`.

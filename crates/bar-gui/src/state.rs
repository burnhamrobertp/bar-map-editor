//! Editor-side runtime state types that need to be both rendered and
//! snapshot-able for undo/redo. Kept separate from `app.rs` so the
//! `undo` module can store them directly without JSON round-tripping,
//! giving type-safe undo/redo coverage of node visuals and group state.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use bar_graph::{GraphEngine, NodeId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Try to install a broad-coverage symbol font as a high-priority
/// fallback in egui's font definitions. Looks for whatever the host
/// OS ships with comprehensive symbol coverage:
///
/// - Windows: `seguisym.ttf` (Segoe UI Symbol) and `seguiemj.ttf`
///   (Segoe UI Emoji), both under `%WINDIR%\Fonts\`.
/// - macOS: `Apple Symbols.ttf`, `Apple Color Emoji.ttc`.
/// - Linux: `NotoSansSymbols2-Regular.ttf` (Noto), `DejaVuSans.ttf`.
///
/// Whichever one is found is loaded under a new `"sys_<label>"`
/// font key and inserted at index 1 in the proportional family
/// fallback chain — so any glyph missing from Ubuntu-Light tries
/// the system font before egui's bundled NotoEmoji. Silently does
/// nothing if no candidate is on disk; the existing default
/// fallbacks still apply.
pub fn install_system_symbol_font(ctx: &eframe::egui::Context) {
    let mut defs = FontDefinitions::default();
    let mut installed_any = false;
    for (label, path) in candidate_font_paths() {
        if let Ok(bytes) = std::fs::read(&path) {
            let key = format!("sys_{label}");
            defs.font_data
                .insert(key.clone(), Arc::new(FontData::from_owned(bytes)));
            defs.families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(1, key.clone());
            defs.families
                .entry(FontFamily::Monospace)
                .or_default()
                .insert(1, key);
            installed_any = true;
        }
    }
    if installed_any {
        ctx.set_fonts(defs);
    }
}

#[cfg(target_os = "windows")]
fn candidate_font_paths() -> Vec<(&'static str, std::path::PathBuf)> {
    let win = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string());
    let mut out = Vec::new();
    for (label, name) in [
        ("seguisym", "seguisym.ttf"),
        ("seguiemj", "seguiemj.ttf"),
    ] {
        out.push((label, std::path::PathBuf::from(&win).join("Fonts").join(name)));
    }
    out
}

#[cfg(target_os = "macos")]
fn candidate_font_paths() -> Vec<(&'static str, std::path::PathBuf)> {
    vec![
        (
            "applesymbols",
            std::path::PathBuf::from("/System/Library/Fonts/Apple Symbols.ttf"),
        ),
        (
            "applecolor",
            std::path::PathBuf::from("/System/Library/Fonts/Apple Color Emoji.ttc"),
        ),
    ]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn candidate_font_paths() -> Vec<(&'static str, std::path::PathBuf)> {
    vec![
        (
            "notosymbols2",
            std::path::PathBuf::from(
                "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
            ),
        ),
        (
            "dejavu",
            std::path::PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
        ),
    ]
}

#[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
fn candidate_font_paths() -> Vec<(&'static str, std::path::PathBuf)> {
    Vec::new()
}

/// Position + size of a node on the editor canvas. The node graph
/// itself doesn't carry visual layout information; this lives
/// alongside it and round-trips through the project file separately.
#[derive(Clone, Debug)]
pub struct NodeVisual {
    pub position: egui::Pos2,
    pub size: egui::Vec2,
}

/// Runtime form of a `NodeGroup`. Visually a labelled rectangle behind
/// its member nodes — no semantic effect on graph evaluation when
/// `is_subgraph` is false. When `is_subgraph` is true the group is a
/// reusable container with explicit external ports and can be
/// collapsed to a single node-like block. Members are stored as
/// runtime `NodeId`s here; conversion to/from recipe-key form happens
/// at save/load time.
#[derive(Clone, Debug)]
pub struct GroupRuntime {
    pub label: String,
    pub member_ids: HashSet<NodeId>,
    pub color_idx: u8,
    pub collapsed: bool,
    pub is_subgraph: bool,
    pub subgraph_inputs: Vec<SubgraphPortRuntime>,
    pub subgraph_outputs: Vec<SubgraphPortRuntime>,
    pub macro_params: Vec<MacroParamRuntime>,
}

/// Runtime form of a SubGraph macro parameter. `binding` resolves to
/// the inner node + param this knob drives; reading and writing the
/// param goes through the graph engine, the SubGraph holds no value
/// of its own beyond the binding.
#[derive(Clone, Debug)]
pub struct MacroParamRuntime {
    pub name: String,
    pub label: String,
    /// One of `"Float" | "UInt" | "Int" | "Bool" | "String"`. Stored
    /// as a string for the same reason `SubgraphPortRuntime.kind`
    /// is — keeps the project format independent of `bar-graph`'s
    /// concrete type names.
    pub kind: String,
    pub binding: Option<(NodeId, String)>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// Runtime form of `bar_project::SubgraphPort`.
///
/// `binding` is a runtime `(NodeId, port_name)` pair pointing at the
/// inner node this external port maps to — the editor reroutes outer
/// connections through it so the underlying graph engine sees direct
/// wires to/from the inner node and doesn't need any
/// subgraph-specific evaluation logic.
#[derive(Clone, Debug)]
pub struct SubgraphPortRuntime {
    pub name: String,
    pub label: String,
    pub kind: String,
    pub binding: Option<(NodeId, String)>,
}

/// Snapshot of every piece of editor state that's covered by undo/redo.
///
/// The graph and visual layout were always covered. Group state was
/// added when group operations gained undo support — this struct is
/// the place to add anything else that should follow that same
/// "snapshot before mutation, restore on undo" lifecycle.
#[derive(Clone, Debug)]
pub struct EditorState {
    pub graph: GraphEngine,
    pub node_visuals: HashMap<NodeId, NodeVisual>,
    pub groups: HashMap<u64, GroupRuntime>,
    pub node_to_group: HashMap<NodeId, u64>,
    pub next_group_id: u64,
}

impl EditorState {
    /// Empty state, used as the initial value for a fresh project /
    /// the placeholder before the first push_undo.
    pub fn empty() -> Self {
        Self {
            graph: GraphEngine::new(),
            node_visuals: HashMap::new(),
            groups: HashMap::new(),
            node_to_group: HashMap::new(),
            next_group_id: 1,
        }
    }
}

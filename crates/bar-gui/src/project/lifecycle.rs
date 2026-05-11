//! Project lifecycle: new / load / save / reset orchestration.
//!
//! Distributed `impl BarEditorApp` block. The single big function
//! here is `reset_project`, which is the *only* place that wipes
//! every per-project field on `BarEditorApp` -- every project-
//! switching path (new, open .barproj, open .sd7, load macro,
//! close) calls it first and then installs new state on top of the
//! blank slate. Adding a new per-project field anywhere in
//! `BarEditorApp` should come with a matching reset here.

use bar_graph::{GraphEngine, Node, NodeId, NodeType};
use eframe::egui;

use crate::app::{BarEditorApp, CanvasView, PendingAction, RecipeMeta};
use crate::state::NodeVisual;

impl BarEditorApp {
    /// Wipe every transient + per-project field so the editor is in a
    /// well-defined "no project loaded" state. This is the ONLY
    /// place where project state is cleared en masse. Every
    /// project-switching path (new, open .barproj, open .sd7,
    /// load macro preset, close) calls this first, then installs
    /// new state on top of the blank slate.
    pub(crate) fn reset_project(&mut self) {
        // Graph engine — counter resets to 1 so the next project
        // gets clean NodeIds with no risk of colliding with stale
        // group member_ids from the previous project.
        self.graph = GraphEngine::new();
        self.visuals.node_visuals.clear();

        // Group / subgraph state — must be cleared together with the
        // graph so stale member_ids can never match new NodeIds.
        self.visuals.groups.clear();
        self.visuals.node_to_group.clear();
        self.visuals.next_group_id = 1;

        // Project identity and output configuration.
        self.project.path = None;
        self.project.loaded_name = None;
        self.project.is_dirty = false;
        self.project.map_info_file = None;
        self.map.settings = bar_project::MapSettings::default();
        self.map.width = 256;
        self.map.height = 256;
        self.map.min_height = 0.0;
        self.map.max_height = 800.0;
        self.map.recipe_meta = RecipeMeta::default();

        // Inspector / preview.
        self.preview.node = None;
        self.preview.open = false;

        // Signal renderers to flush stale GPU resources.
        self.project.graph_reset = true;

        // Undo history — never cross a project boundary, otherwise
        // Ctrl+Z would resurrect nodes from a different project.
        self.history.clear();

        // Brush state + live paint caches — owned by `PaintSession`
        // which knows how to drop them all together. Sculpt lock
        // also released; the next graph eval repopulates the
        // heightmap from scratch.
        self.paint.invalidate_on_graph_reset();

        // Validation panel — findings cache from a different graph
        // would lie about the current state. Filter and panel-open
        // flag also reset so the user sees a clean panel state next
        // project.
        self.dialog.show_validation_panel = false;
        self.validation.reset();

        // Modal / window-open flags. These should never persist
        // across a project switch — the user expects the new project
        // to open with no dialogs up.
        self.dialog.show_inspector = false;
        self.dialog.show_mapinfo_editor = false;
        self.dialog.show_map_info_picker = false;
        self.dialog.file_editor = None;
        self.dialog.confirm_dialog = None;
        self.dialog.pending_action = None;
        self.selection.pending_group_delete = None;

        // Selection / drag state — selections from the previous
        // graph would point at NodeIds that no longer exist.
        self.selection.node = None;
        self.selection.nodes.clear();
        self.selection.group = None;
        self.selection.connection = None;
        self.canvas.drag_connection = None;
        self.canvas.marquee_start = None;
        self.map.dragging_spawn = None;
        self.palette_drag = None;
        self.project.passthrough_edit = None;
        self.dialog.pending_props_open = None;
        self.props.close();

        // Transient status / toast — messages from the previous
        // project would mislead the user about what just happened.
        self.dialog.toast = None;
        self.dialog.status_message = None;

        // Preview / export state -- viewport open flag, driving node,
        // run pulses, and export status all reset together. preview_node
        // is cleared earlier in this function (it depends on the graph,
        // which the project replacement clobbers).
        self.preview.reset();

        // Canvas viewport — pan offset and the cached canvas rect
        // from the previous project's layout would land the new
        // graph in the wrong viewport. apply_project re-installs
        // the saved offset AFTER this reset for loaded projects.
        self.canvas.offset = egui::Vec2::ZERO;
        self.canvas.rect_last = egui::Rect::NOTHING;

        // Tabs — only the Main tab survives a project switch; any
        // SubGraph / Sculpt tabs from the previous project refer to
        // NodeIds that no longer exist.
        self.canvas.tabs = vec![CanvasView::Main];
        self.canvas.active_tab = 0;
        self.canvas.last_active_tab = 0;
    }

    pub(crate) fn do_new_project(&mut self) {
        self.reset_project();

        // Drop the two terminal nodes every project ends with: a
        // Bundler for export and a Preview for the 3D viewport.
        // Both are placed near the right edge of the visible canvas
        // so the user can build their pipeline left-to-right toward
        // these sinks. Layout reused by the welcome screen's
        // "Blank Project" path so all entry points produce
        // identical starting state.
        let (bundler_pos, preview_pos) = self.starter_terminal_positions();
        let bundler_id = self
            .graph
            .add_node(Node::new(NodeId(0), NodeType::Bundler, "Bundler"));
        self.visuals.node_visuals.insert(
            bundler_id,
            NodeVisual {
                position: bundler_pos,
                size: egui::vec2(210.0, 240.0),
            },
        );
        let preview_id = self
            .graph
            .add_node(Node::new(NodeId(0), NodeType::Preview, "3D Preview"));
        self.visuals.node_visuals.insert(
            preview_id,
            NodeVisual {
                position: preview_pos,
                size: egui::vec2(180.0, 150.0),
            },
        );
        self.preview.node = Some(preview_id);
    }

    /// Where to place the Bundler / Preview terminal nodes on a
    /// fresh project. Anchors to the right edge of the most-recent
    /// canvas rect (so the user can build left-to-right toward the
    /// sinks); falls back to a sensible default when the canvas
    /// hasn't been laid out yet.
    pub(crate) fn starter_terminal_positions(&self) -> (egui::Pos2, egui::Pos2) {
        let bundler_size = egui::vec2(210.0, 240.0);
        let preview_size = egui::vec2(180.0, 150.0);
        let margin = 40.0_f32;
        let gap = 60.0_f32;
        let canvas_w = if self.canvas.rect_last.is_positive() {
            self.canvas.rect_last.width()
        } else {
            // Welcome → Blank Project on first launch can fire
            // before any canvas frame has run; pick a width that
            // matches the typical default viewport.
            1100.0
        };
        let right_x = canvas_w - margin;
        let bundler_x = right_x - bundler_size.x;
        let preview_x = right_x - preview_size.x;
        let top_y = 80.0;
        let bundler_pos = egui::pos2(bundler_x, top_y);
        let preview_pos = egui::pos2(preview_x, top_y + bundler_size.y + gap);
        (bundler_pos, preview_pos)
    }

    /// Drop the default terminal nodes (Bundler + 3D Preview) onto
    /// an empty graph — the welcome panel's "Empty graph" entry
    /// point. Lives in `BarEditorApp` because it touches private
    /// fields directly; the panel calls it through this shim.
    pub(crate) fn welcome_blank_project(&mut self) {
        let (bundler_pos, preview_pos) = self.starter_terminal_positions();
        let bundler_id = self
            .graph
            .add_node(Node::new(NodeId(0), NodeType::Bundler, "Bundler"));
        self.visuals.node_visuals.insert(
            bundler_id,
            NodeVisual {
                position: bundler_pos,
                size: egui::vec2(210.0, 240.0),
            },
        );
        let preview_id = self
            .graph
            .add_node(Node::new(NodeId(0), NodeType::Preview, "3D Preview"));
        self.visuals.node_visuals.insert(
            preview_id,
            NodeVisual {
                position: preview_pos,
                size: egui::vec2(180.0, 150.0),
            },
        );
        self.preview.node = Some(preview_id);
        self.project.is_dirty = true;
    }

    /// Welcome panel's "Open project / SD7…" button. Same as the
    /// File menu's Open — spawn the OS dialog on a worker so the
    /// egui main loop keeps rendering.
    pub(crate) fn welcome_open_dialog(&mut self) {
        self.open_file_dialog_async();
    }

    /// Welcome panel's "Recent" menu entry click — defers to the
    /// existing dirty-aware open path.
    pub(crate) fn start_open_path_for_panel(&mut self, path: std::path::PathBuf) {
        self.start_open_path(path);
    }

    /// Begin loading a built-in macro preset, routing through
    /// unsaved-changes confirmation when the current project is dirty.
    /// Used by File → New from Preset; the welcome panel calls
    /// `start_with_macro` directly because its precondition (empty
    /// graph, no project loaded) means there's nothing to discard.
    pub(crate) fn start_load_macro(&mut self, name: &str) {
        if self.project.is_dirty {
            self.dialog.pending_action = Some(PendingAction::LoadMacro {
                name: name.to_string(),
            });
        } else {
            self.start_with_macro(name);
        }
    }

    /// True when there's an open project — either loaded from disk
    /// (`project_path` set) or built up in-memory (graph has nodes).
    /// Used to gate the action toolbar, node palette, and validation
    /// panel: those surfaces only make sense once the user has
    /// committed to a project, otherwise the welcome screen is what
    /// they should be looking at.
    pub fn has_project(&self) -> bool {
        self.project.path.is_some() || !self.graph.nodes().is_empty()
    }

    /// Save to the existing project path, or fall back to Save As
    /// when none is set yet (untitled project).
    pub(crate) fn save_or_save_as(&mut self) {
        if let Some(p) = self.project.path.clone() {
            self.save_project(p);
        } else {
            self.save_as();
        }
    }

    pub(crate) fn save_as(&mut self) {
        if let Some(path) = self
            .make_dialog()
            .set_title("Save Project As")
            .add_filter("BAR Map Editor Project", &["barproj"])
            .save_file()
        {
            self.save_project(path);
        }
    }
}

impl BarEditorApp {
    /// Begin an open operation. If the current project is dirty, this
    /// defers the open through an unsaved-changes confirmation;
    /// otherwise it opens immediately. Routes both .barproj and .sd7
    /// paths.
    pub(crate) fn start_open_path(&mut self, path: std::path::PathBuf) {
        if self.project.is_dirty {
            self.dialog.pending_action = Some(PendingAction::OpenPath(path));
        } else {
            self.dispatch_open(path);
        }
    }

    pub(crate) fn dispatch_open(&mut self, path: std::path::PathBuf) {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("sd7") => self.open_map_as_project(path),
            _ => self.load_project(path),
        }
    }

    /// Load a project from a file.
    pub(crate) fn load_project(&mut self, path: std::path::PathBuf) {
        use bar_project::Project;
        let project = match Project::load(&path) {
            Ok(p) => p,
            Err(e) => {
                self.dialog.status_message = Some(format!("Load failed: {e}"));
                // If a recent-files entry pointed at a now-broken file, drop it.
                self.settings.remove_recent(&path);
                self.settings.save();
                return;
            }
        };
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string());
        let display = path.display().to_string();
        self.settings.add_recent(&path);
        self.settings.save();
        self.apply_project(project, Some(path), name, format!("Loaded: {display}"));
    }

    /// Apply a loaded/parsed project as the current session.
    fn apply_project(
        &mut self,
        project: bar_project::Project,
        path: Option<std::path::PathBuf>,
        name: String,
        status: String,
    ) {
        use std::collections::HashMap;

        use bar_graph::ParamValue;

        use crate::app::parse_subgraph_binding;
        use crate::state::GroupRuntime;

        let graph = match project.recipe.build_graph() {
            Ok(g) => g,
            Err(e) => {
                self.dialog.status_message = Some(format!("Invalid project: {e}"));
                return;
            }
        };

        // Wipe all project state before installing the new one.
        self.reset_project();

        // Install the new project's graph (overrides reset_project's
        // GraphEngine::new()).
        self.graph = graph;

        // Install per-project layout, overriding reset_project's
        // zero-offset default.
        self.canvas.offset = egui::vec2(
            project.layout.canvas_offset.0,
            project.layout.canvas_offset.1,
        );
        self.map.width = project.recipe.output.width;
        self.map.height = project.recipe.output.height;
        self.project.map_info_file = project.layout.map_info_file.clone();
        self.map.settings = project.recipe.output.map_settings.clone();
        self.map.recipe_meta = RecipeMeta {
            shortname: project.recipe.shortname.clone(),
            description: project.recipe.description.clone(),
            author: project.recipe.author.clone(),
            version: project.recipe.version.clone(),
        };
        self.map.min_height = self.map.settings.min_height;
        self.map.max_height = self.map.settings.max_height;

        // Resolve any project-relative file paths (`bar://...`)
        // against the .barproj's directory so executors get absolute
        // paths they can read.
        if let Some(project_dir) = path.as_ref().and_then(|p| p.parent()) {
            self.resolve_relative_paths(project_dir);
        }

        // Restore node positions and sizes. Build a key->id map for
        // the groups restoration that follows.
        let mut key_to_id: HashMap<String, NodeId> = HashMap::new();
        for (idx, recipe_node) in project.recipe.nodes.iter().enumerate() {
            let node_id = NodeId((idx + 1) as u64);
            key_to_id.insert(recipe_node.key.clone(), node_id);
            let pos = project
                .layout
                .node_positions
                .get(&recipe_node.key)
                .map(|p| egui::pos2(p.x, p.y))
                .unwrap_or_else(|| egui::pos2(200.0 + (idx as f32 * 180.0), 200.0));
            let size = project
                .layout
                .node_sizes
                .get(&recipe_node.key)
                .map(|s| egui::vec2(s.width, s.height))
                .unwrap_or_else(|| egui::vec2(150.0, 80.0));
            self.visuals.node_visuals.insert(
                node_id,
                NodeVisual {
                    position: pos,
                    size,
                },
            );
        }

        // Restore groups: convert recipe-key references back to
        // NodeIds and rebuild the reverse index. Drop members whose
        // keys no longer resolve (rare; happens if a save was
        // hand-edited). (reset_project already cleared
        // groups/node_to_group above.)
        let mut max_group_id: u64 = 0;
        for g in &project.layout.groups {
            let member_ids: std::collections::HashSet<NodeId> = g
                .member_keys
                .iter()
                .filter_map(|k| key_to_id.get(k).copied())
                .collect();
            for &nid in &member_ids {
                self.visuals.node_to_group.insert(nid, g.id);
            }
            self.visuals.groups.insert(
                g.id,
                GroupRuntime {
                    label: g.label.clone(),
                    member_ids,
                    color_idx: g.color_idx,
                    collapsed: g.collapsed,
                    is_subgraph: g.is_subgraph,
                    subgraph_inputs: g
                        .subgraph_inputs
                        .iter()
                        .map(|p| crate::state::SubgraphPortRuntime {
                            name: p.name.clone(),
                            label: p.label.clone(),
                            kind: p.kind.clone(),
                            binding: parse_subgraph_binding(p.binding.as_deref(), &key_to_id),
                        })
                        .collect(),
                    subgraph_outputs: g
                        .subgraph_outputs
                        .iter()
                        .map(|p| crate::state::SubgraphPortRuntime {
                            name: p.name.clone(),
                            label: p.label.clone(),
                            kind: p.kind.clone(),
                            binding: parse_subgraph_binding(p.binding.as_deref(), &key_to_id),
                        })
                        .collect(),
                    macro_params: g
                        .macro_params
                        .iter()
                        .map(|p| crate::state::MacroParamRuntime {
                            name: p.name.clone(),
                            label: p.label.clone(),
                            kind: p.kind.clone(),
                            binding: parse_subgraph_binding(Some(&p.binding), &key_to_id),
                            min: p.min,
                            max: p.max,
                        })
                        .collect(),
                },
            );
            max_group_id = max_group_id.max(g.id);
        }
        self.visuals.next_group_id = max_group_id + 1;

        // ── Migrate legacy subgraph ports → IO nodes ───────────────
        // Projects saved before the IO-node refactor stored external
        // ports in `subgraph_inputs/outputs`; we now author them as
        // `SubgraphInput` / `SubgraphOutput` nodes inside the
        // subgraph. For each subgraph that has persisted ports but
        // no IO nodes, create one IO node per entry, wire it to the
        // bound inner port, and clear the legacy lists. The next
        // frame's `recompute_all_subgraph_io` rebuilds the runtime
        // port lists from the new nodes.
        let mut migrations: Vec<(
            u64,
            Vec<crate::state::SubgraphPortRuntime>,
            Vec<crate::state::SubgraphPortRuntime>,
        )> = Vec::new();
        for (gid, g) in &self.visuals.groups {
            if !g.is_subgraph {
                continue;
            }
            let already_has_io_nodes = g.member_ids.iter().any(|id| {
                self.graph.get_node(*id).is_some_and(|n| {
                    matches!(
                        n.node_type,
                        NodeType::SubgraphInput | NodeType::SubgraphOutput
                    )
                })
            });
            if already_has_io_nodes {
                continue;
            }
            if g.subgraph_inputs.is_empty() && g.subgraph_outputs.is_empty() {
                continue;
            }
            migrations.push((*gid, g.subgraph_inputs.clone(), g.subgraph_outputs.clone()));
        }
        for (gid, ins, outs) in migrations {
            // Lay legacy-migrated IO nodes near (0, 0) of the
            // subgraph view; the user can rearrange. We pick a
            // position based on the existing members' centroid so
            // they don't land at the origin off-screen.
            let centroid = self
                .visuals
                .groups
                .get(&gid)
                .map(|g| {
                    let pts: Vec<egui::Pos2> = g
                        .member_ids
                        .iter()
                        .filter_map(|id| self.visuals.node_visuals.get(id))
                        .map(|v| v.position)
                        .collect();
                    if pts.is_empty() {
                        egui::pos2(0.0, 0.0)
                    } else {
                        let n = pts.len() as f32;
                        let sx: f32 = pts.iter().map(|p| p.x).sum();
                        let sy: f32 = pts.iter().map(|p| p.y).sum();
                        egui::pos2(sx / n, sy / n)
                    }
                })
                .unwrap_or(egui::pos2(0.0, 0.0));

            for (i, p) in ins.iter().enumerate() {
                let mut node = Node::new(NodeId(0), NodeType::SubgraphInput, p.label.clone());
                node.params
                    .insert("name".to_string(), ParamValue::String(p.name.clone()));
                node.params
                    .insert("kind".to_string(), ParamValue::String(p.kind.clone()));
                node.sync_subgraph_io_kind();
                let id = self.graph.add_node(node);
                self.visuals.node_visuals.insert(
                    id,
                    crate::state::NodeVisual {
                        position: centroid + egui::vec2(-220.0, i as f32 * 90.0),
                        size: egui::vec2(150.0, 80.0),
                    },
                );
                if let Some(g) = self.visuals.groups.get_mut(&gid) {
                    g.member_ids.insert(id);
                }
                self.visuals.node_to_group.insert(id, gid);
                if let Some((inner_id, inner_port)) = p.binding.clone() {
                    let _ = self.graph.connect(
                        bar_graph::PortId {
                            node_id: id,
                            port_name: "value".to_string(),
                        },
                        bar_graph::PortId {
                            node_id: inner_id,
                            port_name: inner_port,
                        },
                    );
                }
            }
            for (i, p) in outs.iter().enumerate() {
                let mut node = Node::new(NodeId(0), NodeType::SubgraphOutput, p.label.clone());
                node.params
                    .insert("name".to_string(), ParamValue::String(p.name.clone()));
                node.params
                    .insert("kind".to_string(), ParamValue::String(p.kind.clone()));
                node.sync_subgraph_io_kind();
                let id = self.graph.add_node(node);
                self.visuals.node_visuals.insert(
                    id,
                    crate::state::NodeVisual {
                        position: centroid + egui::vec2(220.0, i as f32 * 90.0),
                        size: egui::vec2(150.0, 80.0),
                    },
                );
                if let Some(g) = self.visuals.groups.get_mut(&gid) {
                    g.member_ids.insert(id);
                }
                self.visuals.node_to_group.insert(id, gid);
                if let Some((inner_id, inner_port)) = p.binding.clone() {
                    let _ = self.graph.connect(
                        bar_graph::PortId {
                            node_id: inner_id,
                            port_name: inner_port,
                        },
                        bar_graph::PortId {
                            node_id: id,
                            port_name: "value".to_string(),
                        },
                    );
                }
            }
            // Clear legacy lists; recompute fills them from IO nodes.
            if let Some(g) = self.visuals.groups.get_mut(&gid) {
                g.subgraph_inputs.clear();
                g.subgraph_outputs.clear();
            }
        }

        // Restore open tabs. Validate each persisted entry against
        // current state -- drop tabs whose target no longer exists
        // (rare; happens after hand-edits to the project file). Main
        // always lives at index 0; persisted "Main" entries are
        // collapsed away to avoid duplicates.
        let mut restored_tabs: Vec<CanvasView> = vec![CanvasView::Main];
        for view in &project.layout.open_tabs {
            match view {
                bar_project::PersistedCanvasView::Main => {}
                bar_project::PersistedCanvasView::SubGraph { group_id } => {
                    if self.visuals.groups.contains_key(group_id) {
                        let v = CanvasView::SubGraph(*group_id);
                        if !restored_tabs.contains(&v) {
                            restored_tabs.push(v);
                        }
                    }
                }
            }
        }
        self.canvas.tabs = restored_tabs;
        self.canvas.active_tab =
            (project.layout.active_tab as usize).min(self.canvas.tabs.len().saturating_sub(1));

        self.project.path = path;
        self.project.loaded_name = Some(name);
        self.dialog.status_message = Some(status);
        self.project.is_dirty = false;
        self.project.graph_reset = true;
    }

    /// Open a .sd7 map archive as a new project.
    ///
    /// Resets graph state immediately and queues the SD7 path for
    /// extraction. The actual extraction runs in a background thread
    /// managed by `bar-app`, which calls `finish_open_map` when
    /// complete.
    fn open_map_as_project(&mut self, path: std::path::PathBuf) {
        self.reset_project();

        let map_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        self.dialog.status_message = Some(format!("Extracting {}…", map_name));

        self.settings.add_recent(&path);
        self.settings.save();
        self.project.sd7_open_request = Some(path);
    }

    /// Take the pending SD7 open request (if any). Called by `bar-app`
    /// each frame; when Some, bar-app spawns the extraction thread.
    pub fn take_sd7_open_request(&mut self) -> Option<std::path::PathBuf> {
        self.project.sd7_open_request.take()
    }

    /// Build the node graph after a successful .sd7 extraction.
    pub fn finish_open_map(&mut self, scan: bar_project::WorkDirScan) {
        let name = scan.map_name.clone();
        let status = format!("Opened: {}", name);
        let project = bar_project::scan_to_project(&scan);
        self.apply_project(project, None, name, status);
        // Imported project hasn't been saved yet.
        self.project.is_dirty = true;
        // Auto-open the 3D preview at the Preview node.
        if let Some(id) = self
            .graph
            .nodes()
            .values()
            .find(|n| n.node_type == NodeType::Preview)
            .map(|n| n.id)
        {
            self.preview.node = Some(id);
            self.preview.open = true;
        }
    }

    /// Pick a default label for a new entity of the given base type
    /// (e.g. "Perlin Noise" or "Mountain Range"). Scans existing node
    /// and group labels for `"<base> <n>"` and returns the next free
    /// number. So dropping three Perlin Noise nodes gives "Perlin
    /// Noise 1", "Perlin Noise 2", "Perlin Noise 3" -- way easier
    /// to track than three identical labels.
    pub(crate) fn next_label_for(&self, base: &str) -> String {
        let prefix = format!("{} ", base);
        let mut max_n: u32 = 0;
        for node in self.graph.nodes().values() {
            if let Some(rest) = node.label.strip_prefix(&prefix) {
                if let Ok(n) = rest.parse::<u32>() {
                    if n > max_n {
                        max_n = n;
                    }
                }
            }
        }
        for group in self.visuals.groups.values() {
            if let Some(rest) = group.label.strip_prefix(&prefix) {
                if let Ok(n) = rest.parse::<u32>() {
                    if n > max_n {
                        max_n = n;
                    }
                }
            }
        }
        format!("{} {}", base, max_n + 1)
    }

    /// Drop a macro into a fresh project: clears any existing graph
    /// and session state, adds a Bundler and Preview to the right,
    /// and wires the macro's outputs through them. Used by both the
    /// welcome panel's preset cards and File → New from Preset; both
    /// surfaces produce identical starting state because they share
    /// this method.
    pub(crate) fn start_with_macro(&mut self, macro_name: &str) {
        use bar_graph::PortId;

        self.reset_project();

        // Right-edge placement for the terminal nodes; drop the
        // macro to the left of them so wires read left-to-right.
        let (bundler_pos, preview_pos) = self.starter_terminal_positions();
        let macro_pos = egui::pos2(
            (bundler_pos.x.min(preview_pos.x) - 320.0).max(40.0),
            bundler_pos.y + 60.0,
        );
        self.instantiate_macro(macro_name, macro_pos);

        // The macro's IO nodes were just added to a fresh group;
        // its `subgraph_outputs` runtime list is still empty (the
        // per-frame `recompute_all_subgraph_io` runs at the START
        // of `update`, before `start_with_macro` is reached).
        // Refresh now so the wiring loop below sees the actual
        // ports.
        self.recompute_all_subgraph_io();

        // Find the group we just created — it'll be the highest gid.
        let new_gid = match self.visuals.groups.keys().copied().max() {
            Some(g) => g,
            None => return,
        };

        // Bundler — for export. Always present so the project is
        // shippable out of the box.
        let mut bundler = Node::new(NodeId(0), NodeType::Bundler, "Bundler");
        bundler.label = "Bundler".to_string();
        let bundler_id = self.graph.add_node(bundler);
        self.visuals.node_visuals.insert(
            bundler_id,
            NodeVisual {
                position: bundler_pos,
                size: egui::vec2(210.0, 240.0),
            },
        );

        // Preview — drives the 3D viewport. Separate sink so a
        // half-wired Bundler can't be mistaken for a working
        // preview.
        let mut preview = Node::new(NodeId(0), NodeType::Preview, "3D Preview");
        preview.label = "3D Preview".to_string();
        let preview_id = self.graph.add_node(preview);
        self.visuals.node_visuals.insert(
            preview_id,
            NodeVisual {
                position: preview_pos,
                size: egui::vec2(180.0, 150.0),
            },
        );
        self.preview.open = true;
        self.preview.node = Some(preview_id);

        // Wire each subgraph output to BOTH the Bundler (for
        // export) and the Preview (for the viewport). Macro IO
        // nodes are unnamed by default, so we route by *kind*:
        // the first Heightmap port goes to the bundler/preview
        // heightmap input, the first Color port goes to texture,
        // etc. Subsequent ports of the same kind are skipped --
        // there's only one Bundler.heightmap to fill.
        // Collect subgraph outputs with their name so we can distinguish
        // "terrain" from "slope" (both are Heightmap kind).
        let outputs: Vec<(String, String, NodeId, String)> = self
            .visuals
            .groups
            .get(&new_gid)
            .map(|g| {
                g.subgraph_outputs
                    .iter()
                    .filter_map(|p| {
                        let (id, port) = p.binding.clone()?;
                        Some((p.name.clone(), p.kind.clone(), id, port))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut routed: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
        let mut heightmap_src: Option<(NodeId, String)> = None;
        let mut slope_src: Option<(NodeId, String)> = None;
        for (name, kind, src_id, src_port) in outputs {
            let port_name: Option<&'static str> = match kind.as_str() {
                "Heightmap" => Some("heightmap"),
                "Color" => Some("texture"),
                _ => None,
            };
            let Some(port_name) = port_name else { continue };

            // Slope is a second Heightmap output that doesn't map to any
            // Bundler input. Capture it for use by SpecularMap/GrassMap.
            if port_name == "heightmap" && name == "slope" {
                slope_src = Some((src_id, src_port));
                continue;
            }

            if !routed.insert(port_name) {
                continue;
            }
            if port_name == "heightmap" {
                heightmap_src = Some((src_id, src_port.clone()));
            }
            let _ = self.graph.connect(
                PortId {
                    node_id: src_id,
                    port_name: src_port.clone(),
                },
                PortId {
                    node_id: bundler_id,
                    port_name: port_name.to_string(),
                },
            );
            if matches!(port_name, "heightmap" | "texture") {
                let _ = self.graph.connect(
                    PortId {
                        node_id: src_id,
                        port_name: src_port,
                    },
                    PortId {
                        node_id: preview_id,
                        port_name: port_name.to_string(),
                    },
                );
            }
        }

        // Auto-fill the rest of the Bundler's inputs so the preset
        // exports a complete bundle out of the box. NormalMap, SpecularMap,
        // and GrassMap derive from the macro's terrain/slope outputs.
        // Metal and type get Constant(0) -- those are project-specific data
        // the user replaces manually. The user is free to swap any node out.
        let aux_x = bundler_pos.x - 220.0;
        let mut aux_y = bundler_pos.y;
        let aux_step = 70.0_f32;
        let aux_size = egui::vec2(150.0, 80.0);

        if let Some((hm_id, hm_port)) = heightmap_src {
            // NormalMap → Bundler.normalmap
            let nm = Node::new(NodeId(0), NodeType::NormalMap, "Normal Map");
            let nm_id = self.graph.add_node(nm);
            self.visuals.node_visuals.insert(
                nm_id,
                NodeVisual {
                    position: egui::pos2(aux_x, aux_y),
                    size: aux_size,
                },
            );
            let _ = self.graph.connect(
                PortId {
                    node_id: hm_id,
                    port_name: hm_port.clone(),
                },
                PortId {
                    node_id: nm_id,
                    port_name: "input".into(),
                },
            );
            let _ = self.graph.connect(
                PortId {
                    node_id: nm_id,
                    port_name: "output".into(),
                },
                PortId {
                    node_id: bundler_id,
                    port_name: "normalmap".into(),
                },
            );
            aux_y += aux_step;

            // SpecularMap → Bundler.specular (+ optional slope input)
            let sm = Node::new(NodeId(0), NodeType::SpecularMap, "Specular Map");
            let sm_id = self.graph.add_node(sm);
            self.visuals.node_visuals.insert(
                sm_id,
                NodeVisual {
                    position: egui::pos2(aux_x, aux_y),
                    size: aux_size,
                },
            );
            let _ = self.graph.connect(
                PortId {
                    node_id: hm_id,
                    port_name: hm_port.clone(),
                },
                PortId {
                    node_id: sm_id,
                    port_name: "input".into(),
                },
            );
            if let Some((s_id, ref s_port)) = slope_src {
                let _ = self.graph.connect(
                    PortId {
                        node_id: s_id,
                        port_name: s_port.clone(),
                    },
                    PortId {
                        node_id: sm_id,
                        port_name: "slope".into(),
                    },
                );
            }
            let _ = self.graph.connect(
                PortId {
                    node_id: sm_id,
                    port_name: "output".into(),
                },
                PortId {
                    node_id: bundler_id,
                    port_name: "specular".into(),
                },
            );
            aux_y += aux_step;

            // GrassMap → Bundler.grassmap (+ optional slope input)
            let gm = Node::new(NodeId(0), NodeType::GrassMap, "Grass Map");
            let gm_id = self.graph.add_node(gm);
            self.visuals.node_visuals.insert(
                gm_id,
                NodeVisual {
                    position: egui::pos2(aux_x, aux_y),
                    size: aux_size,
                },
            );
            let _ = self.graph.connect(
                PortId {
                    node_id: hm_id,
                    port_name: hm_port,
                },
                PortId {
                    node_id: gm_id,
                    port_name: "input".into(),
                },
            );
            if let Some((s_id, ref s_port)) = slope_src {
                let _ = self.graph.connect(
                    PortId {
                        node_id: s_id,
                        port_name: s_port.clone(),
                    },
                    PortId {
                        node_id: gm_id,
                        port_name: "slope".into(),
                    },
                );
            }
            let _ = self.graph.connect(
                PortId {
                    node_id: gm_id,
                    port_name: "output".into(),
                },
                PortId {
                    node_id: bundler_id,
                    port_name: "grassmap".into(),
                },
            );
            aux_y += aux_step;
        }

        // PaintedHeightmap for metal and type -- starts blank (all
        // zeros) but the map-maker can open either canvas and paint
        // ore spots or terrain-type zones directly.
        for (port, label) in [("metalmap", "Metal Map"), ("typemap", "Type Map")] {
            let node = Node::new(NodeId(0), NodeType::PaintedHeightmap, label);
            let nid = self.graph.add_node(node);
            self.visuals.node_visuals.insert(
                nid,
                NodeVisual {
                    position: egui::pos2(aux_x, aux_y),
                    size: aux_size,
                },
            );
            let _ = self.graph.connect(
                PortId {
                    node_id: nid,
                    port_name: "output".into(),
                },
                PortId {
                    node_id: bundler_id,
                    port_name: port.into(),
                },
            );
            aux_y += aux_step;
        }

        // Clear selection so the deferred auto-layout below treats
        // the call as "lay out everything top-level" rather than
        // "lay out only this group" -- the layout helper inspects
        // selection state. Auto-layout itself is deferred to the
        // next frame because we may still be rendering the welcome
        // panel or the File menu; `canvas_rect_last` won't be valid
        // until `draw_node_graph` runs at least once.
        self.selection.nodes.clear();
        self.selection.node = None;
        self.selection.group = None;
        self.props.active = None;
        self.canvas.pending_auto_layout_all = true;

        self.project.is_dirty = true;
        self.dialog.status_message = Some(format!(
            "Started a new project with the '{}' template.",
            macro_name
        ));
    }
}

#[cfg(test)]
mod session_reset_tests {
    use std::time::Instant;

    use eframe::egui;

    use crate::app::{BarEditorApp, BrushTool, MapInfoTab, ValidationFilter};

    /// Stuff a default app with as many transient session-state fields
    /// as the helper is meant to clear. Used by every test below so
    /// each behaviour is asserted against a richly populated baseline,
    /// not a fresh default.
    fn dirtied_app() -> BarEditorApp {
        let mut app = BarEditorApp::default();
        app.push_undo("seed snapshot");
        app.paint.brush.tool = BrushTool::Lower;
        app.paint.brush.color_rgb = [10, 20, 30];
        app.paint.brush.paint_value = 0.42;
        app.paint.brush_stroking = true;
        app.canvas.offset = egui::vec2(123.0, 456.0);
        app.dialog.show_validation_panel = true;
        app.validation.findings = vec![];
        app.validation.filter = ValidationFilter::Error;
        app.dialog.show_inspector = true;
        app.dialog.show_mapinfo_editor = true;
        app.validation.mapinfo_tab = MapInfoTab::Atmosphere;
        app.dialog.toast = Some(("hi".into(), Instant::now()));
        app.dialog.status_message = Some("from previous project".into());
        app.preview.run_requested = true;
        app.preview.test_in_bar_requested = true;
        app
    }

    #[test]
    fn reset_project_clears_all_fields() {
        let mut app = dirtied_app();
        app.reset_project();
        assert!(!app.history.can_undo(), "history must be cleared");
        assert!(
            matches!(app.paint.brush.tool, BrushTool::Raise),
            "brush tool defaults to Raise"
        );
        assert!(!app.paint.brush_stroking);
        assert_eq!(
            app.canvas.offset,
            egui::Vec2::ZERO,
            "canvas pan offset must reset to zero"
        );
        assert!(!app.dialog.show_validation_panel);
        assert!(matches!(app.validation.filter, ValidationFilter::All));
        assert!(!app.dialog.show_inspector);
        assert!(!app.dialog.show_mapinfo_editor);
        assert!(matches!(app.validation.mapinfo_tab, MapInfoTab::Identity));
        assert!(app.dialog.toast.is_none());
        assert!(app.dialog.status_message.is_none());
        assert!(!app.preview.run_requested);
        assert!(!app.preview.test_in_bar_requested);
        assert!(app.paint.color_buffer.is_none());
        assert!(app.paint.metalmap.is_none());
        assert!(app.paint.typemap.is_none());
    }

    #[test]
    fn start_with_macro_resets_transient_state() {
        let mut app = dirtied_app();
        let prior_depth = app.history.undo_depth();
        app.start_with_macro("Plains");
        // History from the previous project is gone. The macro drop
        // pushes exactly one new undo entry (so the user can undo
        // their first action), so depth is 1, not the pre-reset value.
        assert!(
            app.history.undo_depth() < prior_depth.saturating_add(1) + 1,
            "history must not accumulate the previous project's snapshots"
        );
        assert_eq!(
            app.history.undo_depth(),
            1,
            "after start_with_macro, history holds only the macro-drop snapshot"
        );
        assert!(matches!(app.paint.brush.tool, BrushTool::Raise));
        assert_eq!(app.canvas.offset, egui::Vec2::ZERO);
        assert!(!app.dialog.show_validation_panel);
        assert!(
            !app.graph.nodes().is_empty(),
            "macro should have dropped nodes onto the graph"
        );
        assert!(
            app.project.is_dirty,
            "starting from a macro is a non-empty diff against the empty default"
        );
    }

    #[test]
    fn do_new_project_resets_transient_state() {
        let mut app = dirtied_app();
        app.do_new_project();
        assert!(!app.history.can_undo());
        assert!(matches!(app.paint.brush.tool, BrushTool::Raise));
        assert_eq!(app.canvas.offset, egui::Vec2::ZERO);
        // do_new_project drops a Bundler + Preview by default.
        assert_eq!(app.graph.nodes().len(), 2);
    }

    #[test]
    fn unknown_macro_name_is_a_noop_with_status() {
        let mut app = BarEditorApp::default();
        app.start_with_macro("Definitely Not A Real Macro");
        // The name lookup happens after the reset+graph-clear, so the
        // graph ends up empty and the user sees a status message.
        // (This documents current behaviour -- the menu only feeds in
        // names from BUILTIN_MACRO_GROUPS, so this branch is defensive.)
        assert!(app.dialog.status_message.is_some());
    }
}

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

#[cfg(test)]
mod session_reset_tests {
    use std::time::Instant;

    use eframe::egui;

    use crate::app::{BarEditorApp, BrushTarget, BrushTool, MapInfoTab, ValidationFilter};

    /// Stuff a default app with as many transient session-state fields
    /// as the helper is meant to clear. Used by every test below so
    /// each behaviour is asserted against a richly populated baseline,
    /// not a fresh default.
    fn dirtied_app() -> BarEditorApp {
        let mut app = BarEditorApp::default();
        app.push_undo("seed snapshot");
        app.paint.brush.tool = BrushTool::Lower;
        app.paint.brush.target = BrushTarget::Color;
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
        assert!(
            matches!(app.paint.brush.target, BrushTarget::Heightmap),
            "brush target defaults to Heightmap"
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
        // Project-data state populated.
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

//! Project lifecycle: new / load / save / reset orchestration.
//!
//! Stage 1 only relocates the existing `session_reset_tests` here so
//! the test suite has a natural home as the lifecycle methods migrate
//! out of `app.rs` in Stage 3. The methods themselves
//! (`do_new_project`, `start_new_project`, `reset_project`,
//! `start_open_path`, `apply_project`, `reset_session_state`,
//! `start_with_macro`, `autosave_now`, etc.) still live on
//! `BarEditorApp` in `app.rs` until that stage.

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
        app.canvas_offset = egui::vec2(123.0, 456.0);
        app.dialog.show_validation_panel = true;
        app.validation_findings = vec![];
        app.validation_filter = ValidationFilter::Error;
        app.dialog.show_inspector = true;
        app.dialog.show_mapinfo_editor = true;
        app.mapinfo_tab = MapInfoTab::Atmosphere;
        app.dialog.toast = Some(("hi".into(), Instant::now()));
        app.dialog.status_message = Some("from previous project".into());
        app.run_requested = true;
        app.test_in_bar_requested = true;
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
            app.canvas_offset,
            egui::Vec2::ZERO,
            "canvas pan offset must reset to zero"
        );
        assert!(!app.dialog.show_validation_panel);
        assert!(matches!(app.validation_filter, ValidationFilter::All));
        assert!(!app.dialog.show_inspector);
        assert!(!app.dialog.show_mapinfo_editor);
        assert!(matches!(app.mapinfo_tab, MapInfoTab::Identity));
        assert!(app.dialog.toast.is_none());
        assert!(app.dialog.status_message.is_none());
        assert!(!app.run_requested);
        assert!(!app.test_in_bar_requested);
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
        assert_eq!(app.canvas_offset, egui::Vec2::ZERO);
        assert!(!app.dialog.show_validation_panel);
        // Project-data state populated.
        assert!(
            !app.graph.nodes().is_empty(),
            "macro should have dropped nodes onto the graph"
        );
        assert!(
            app.is_dirty,
            "starting from a macro is a non-empty diff against the empty default"
        );
    }

    #[test]
    fn do_new_project_resets_transient_state() {
        let mut app = dirtied_app();
        app.do_new_project();
        assert!(!app.history.can_undo());
        assert!(matches!(app.paint.brush.tool, BrushTool::Raise));
        assert_eq!(app.canvas_offset, egui::Vec2::ZERO);
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

//! Undo/redo for the editor.
//!
//! Snapshot-based: every mutation pushes a clone of the editor's
//! `EditorState` (graph + node visuals + groups + group reverse index)
//! before applying the mutation. Undo pops the most recent snapshot
//! and swaps it in; redo pops back the other way.
//!
//! Why snapshots, not commands:
//! - Node graphs have complex interdependencies (deleting a node
//!   cascades into removing connections). Reproducing those side-
//!   effects in command form is fiddly; cloning the whole state is
//!   simpler and demonstrably correct.
//! - Groups touch multiple maps in concert (the runtime map +
//!   reverse index). A snapshot covers them atomically.
//! - For OM's typical graph sizes (dozens of nodes), the clone cost
//!   is negligible.
//!
//! Memory is bounded by `max_history`. Each snapshot is small —
//! GraphEngine nodes are just owned data, no GPU handles.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::state::EditorState;

/// A captured editor state plus a human-readable label. The label is
/// shown in undo-history UIs and helps debugging.
///
/// `painted_assets` maps each affected on-disk paint asset path (e.g.
/// `<project>/assets/<id>.bin`) to a content-hash pointer into the
/// editor's `PaintHistoryStore`. Snapshots that revert painting know
/// to write those bytes back to disk during `restore_snapshot`. The
/// hash indirection keeps the snapshot itself compact -- the bytes are
/// stored once per unique content in the side table, deduped across
/// snapshots.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub state: EditorState,
    pub description: String,
    pub painted_assets: HashMap<PathBuf, u64>,
}

/// LIFO stack of snapshots with a paired redo stack. Both are bounded
/// by `max_history`.
pub struct UndoHistory {
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    max_history: usize,
}

impl UndoHistory {
    pub fn new(max_history: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_history,
        }
    }

    /// Push a new state snapshot. Clears the redo stack — redoing past
    /// a fresh mutation isn't meaningful.
    pub fn push(&mut self, snapshot: Snapshot) {
        self.redo_stack.clear();
        self.undo_stack.push(snapshot);
        if self.undo_stack.len() > self.max_history {
            let excess = self.undo_stack.len() - self.max_history;
            self.undo_stack.drain(0..excess);
        }
    }

    /// Pop the most recent snapshot, push `current` onto redo.
    /// Returns the snapshot to restore (i.e. the state from before
    /// the most recent mutation).
    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let prev = self.undo_stack.pop()?;
        self.redo_stack.push(current);
        Some(prev)
    }

    /// Inverse of `undo`.
    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.redo_stack.pop()?;
        self.undo_stack.push(current);
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
    /// Peek at the snapshot that would be popped by `undo` without
    /// modifying the stack. Used so the caller can capture the right
    /// painted-asset paths when building the `current` snapshot.
    pub fn peek_undo(&self) -> Option<&Snapshot> {
        self.undo_stack.last()
    }
    /// Peek at the snapshot that would be popped by `redo`.
    pub fn peek_redo(&self) -> Option<&Snapshot> {
        self.redo_stack.last()
    }
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }
    pub fn redo_depth(&self) -> usize {
        self.redo_stack.len()
    }
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

impl Default for UndoHistory {
    fn default() -> Self {
        Self::new(100)
    }
}

use crate::app::BarEditorApp;

impl BarEditorApp {
    /// Capture the entire undoable editor state (no painted-asset
    /// capture -- non-paint mutations don't need it). Use
    /// `snapshot_with_painted` when a stroke is being recorded. Takes
    /// `&mut self` for symmetry with `snapshot_with_painted`, which
    /// has to mutate the paint-history store; this entry just creates
    /// an empty `painted_assets` map.
    pub(crate) fn snapshot(&mut self, description: &str) -> Snapshot {
        self.snapshot_with_painted(description, std::iter::empty::<PathBuf>())
    }

    /// Capture editor state AND interned bytes for the given asset
    /// paths. The brush flush flow calls this so that undo can write
    /// the pre-stroke bytes back to disk later.
    pub(crate) fn snapshot_with_painted<I>(&mut self, description: &str, paths: I) -> Snapshot
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let mut painted_assets = HashMap::new();
        for path in paths {
            if let Ok(bytes) = std::fs::read(&path) {
                let id = self.paint_history.register(bytes);
                painted_assets.insert(path, id);
            }
        }
        Snapshot {
            state: EditorState {
                graph: self.graph.clone(),
                node_visuals: self.visuals.node_visuals.clone(),
                groups: self.visuals.groups.clone(),
                node_to_group: self.visuals.node_to_group.clone(),
                next_group_id: self.visuals.next_group_id,
                map: crate::state::MapStateSnapshot {
                    width: self.map.width,
                    height: self.map.height,
                    min_height: self.map.min_height,
                    max_height: self.map.max_height,
                    settings: self.map.settings.clone(),
                    recipe_meta: self.map.recipe_meta.clone(),
                    features: self.map.features.clone(),
                },
            },
            description: description.to_string(),
            painted_assets,
        }
    }

    /// Push the current state onto the undo stack before a mutation.
    /// Pair every user-visible mutation with one of these calls.
    pub(crate) fn push_undo(&mut self, description: &str) {
        let snap = self.snapshot(description);
        self.history.push(snap);
        self.project.is_dirty = true;
    }

    /// Push an undo entry that also remembers the pre-mutation contents
    /// of the listed asset files. On undo, those bytes get written back
    /// to the original paths; on redo, the post-mutation bytes (captured
    /// at the moment `undo()` runs) are written.
    pub(crate) fn push_undo_with_painted<I>(&mut self, description: &str, paths: I)
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let snap = self.snapshot_with_painted(description, paths);
        self.history.push(snap);
        self.project.is_dirty = true;
    }

    /// Swap the editor's state with a captured snapshot. Resets
    /// transient UI state so the user doesn't see stale highlights
    /// pointing at deleted things. Also restores any painted-asset
    /// bytes captured with the snapshot back to disk.
    pub(crate) fn restore_snapshot(&mut self, snap: Snapshot) {
        self.graph = snap.state.graph;
        self.visuals.node_visuals = snap.state.node_visuals;
        self.visuals.groups = snap.state.groups;
        self.visuals.node_to_group = snap.state.node_to_group;
        self.visuals.next_group_id = snap.state.next_group_id;
        // Restore map state (dimensions, height range, mapinfo settings,
        // recipe identity, feature placements). Skips transient UI
        // state -- in-progress drags, current selection, and the
        // placement-dirty flag -- which would feel intrusive if reset
        // by an unrelated undo.
        self.map.width = snap.state.map.width;
        self.map.height = snap.state.map.height;
        self.map.min_height = snap.state.map.min_height;
        self.map.max_height = snap.state.map.max_height;
        self.map.settings = snap.state.map.settings;
        self.map.recipe_meta = snap.state.map.recipe_meta;
        self.map.features = snap.state.map.features;
        // Force a GPU instance rebuild on the next layout-manager
        // tick in case features changed (cheap when they didn't).
        self.map.features_placement_dirty = true;
        self.clear_selection();
        // Replay painted-asset captures: write the recorded bytes back
        // to the asset path. Then clear the cached preview heightmap /
        // colour buffer so the next graph eval reloads them from disk.
        let mut touched_any = false;
        for (path, id) in &snap.painted_assets {
            let Some(bytes) = self.paint_history.get(*id) else {
                tracing::error!(?path, id, "Paint undo: blob missing from history store");
                continue;
            };
            if let Err(e) = std::fs::write(path, bytes.as_ref()) {
                tracing::error!(?path, error = %e, "Paint undo: failed to restore asset");
                continue;
            }
            touched_any = true;
        }
        if touched_any {
            // Deliberately do NOT clear `paint.heightmap` /
            // `paint.color_buffer` here. They still hold the
            // post-stroke painted bytes; that's only a few frames
            // out-of-date until the eval triggered by
            // `asset_revision` runs and overwrites them with the
            // restored pre-stroke bytes. Showing the slightly-stale
            // post-stroke state for ~50-100ms is dramatically less
            // jarring than blanking the sculpt view to the
            // "no terrain" spinner while waiting.
            self.paint.asset_revision = self.paint.asset_revision.wrapping_add(1);
        }
    }

    /// Perform undo. Captures the current state (with the same painted
    /// paths the popped snapshot tracks) so redo can recover.
    pub fn undo(&mut self) {
        let Some(prev) = self.history.peek_undo() else {
            return;
        };
        let paint_paths: Vec<PathBuf> = prev.painted_assets.keys().cloned().collect();
        let current = self.snapshot_with_painted("current", paint_paths);
        if let Some(prev) = self.history.undo(current) {
            self.restore_snapshot(prev);
        }
    }

    /// Perform redo. Mirror of `undo` -- captures current state with
    /// the same paint paths the redo target tracks.
    pub fn redo(&mut self) {
        let Some(next) = self.history.peek_redo() else {
            return;
        };
        let paint_paths: Vec<PathBuf> = next.painted_assets.keys().cloned().collect();
        let current = self.snapshot_with_painted("current", paint_paths);
        if let Some(next) = self.history.redo(current) {
            self.restore_snapshot(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::EditorState;
    use bar_graph::{Node, NodeId, NodeType};

    fn make_snapshot(desc: &str, node_count: usize) -> Snapshot {
        let mut state = EditorState::empty();
        for _ in 0..node_count {
            state
                .graph
                .add_node(Node::new(NodeId(0), NodeType::PerlinNoise, "Test"));
        }
        Snapshot {
            state,
            description: desc.to_string(),
            painted_assets: HashMap::new(),
        }
    }

    #[test]
    fn test_undo_redo_basic() {
        let mut history = UndoHistory::new(50);
        history.push(make_snapshot("Add node 1", 1));
        history.push(make_snapshot("Add node 2", 2));
        assert!(history.can_undo());
        assert!(!history.can_redo());

        let current = make_snapshot("Current", 3);
        let restored = history.undo(current).unwrap();
        assert_eq!(restored.description, "Add node 2");
        assert!(history.can_undo());
        assert!(history.can_redo());

        let current2 = make_snapshot("After undo", 2);
        let redone = history.redo(current2).unwrap();
        assert_eq!(redone.description, "Current");
        assert!(!history.can_redo());
    }

    #[test]
    fn test_push_clears_redo() {
        let mut history = UndoHistory::new(50);
        history.push(make_snapshot("A", 1));
        history.push(make_snapshot("B", 2));
        let current = make_snapshot("C", 3);
        history.undo(current).unwrap();
        assert!(history.can_redo());
        history.push(make_snapshot("D", 4));
        assert!(!history.can_redo());
    }

    #[test]
    fn test_max_history_enforced() {
        let mut history = UndoHistory::new(5);
        for i in 0..10 {
            history.push(make_snapshot(&format!("Step {i}"), i));
        }
        assert_eq!(history.undo_depth(), 5);
    }

    #[test]
    fn test_undo_empty() {
        let mut history = UndoHistory::new(50);
        let current = make_snapshot("current", 1);
        assert!(history.undo(current).is_none());
    }

    #[test]
    fn undo_restores_map_state() {
        // End-to-end: dirty MapState, push_undo, mutate further,
        // undo -> MapState reverts to the snapshotted values. This
        // is the core invariant that makes mapinfo-editor edits and
        // feature-placement edits undoable.
        let mut app = BarEditorApp::default();
        app.map.width = 513;
        app.map.height = 513;
        app.map.settings.water.fresnel_power = 4.0;
        app.map.features.clear();
        app.push_undo("baseline");

        // Simulate "user edits mapinfo + adds a feature".
        app.map.settings.water.fresnel_power = 6.5;
        app.map.features.push(bar_project::recipe::PlacedFeature {
            feature_type: "arborreal".into(),
            x: 1.0,
            y: 0.0,
            z: 2.0,
            angle: 0.0,
            taken_damage: 0,
        });

        app.undo();
        assert!(
            (app.map.settings.water.fresnel_power - 4.0).abs() < 1e-6,
            "fresnel_power should revert to baseline 4.0, got {}",
            app.map.settings.water.fresnel_power
        );
        assert!(
            app.map.features.is_empty(),
            "features should revert to empty, got {} entries",
            app.map.features.len()
        );
    }

    #[test]
    fn snapshot_carries_map_state() {
        // Map state (dimensions, height range, mapinfo settings,
        // recipe identity, features) round-trips through the snapshot
        // clone -- this is what makes mapinfo / feature edits undoable.
        let mut state = EditorState::empty();
        state.map.width = 513;
        state.map.height = 1025;
        state.map.min_height = -50.0;
        state.map.max_height = 800.0;
        state.map.settings.water.fresnel_power = 6.5;
        state.map.recipe_meta.author = Some("test_author".into());
        state.map.features.push(bar_project::recipe::PlacedFeature {
            feature_type: "arborreal".into(),
            x: 100.0,
            y: 0.0,
            z: 200.0,
            angle: 0.0,
            taken_damage: 0,
        });
        let snap = Snapshot {
            state: state.clone(),
            description: "with-map".into(),
            painted_assets: HashMap::new(),
        };
        let cloned = snap.clone();
        assert_eq!(cloned.state.map.width, 513);
        assert_eq!(cloned.state.map.height, 1025);
        assert!((cloned.state.map.min_height - (-50.0)).abs() < 1e-6);
        assert!((cloned.state.map.max_height - 800.0).abs() < 1e-6);
        assert!((cloned.state.map.settings.water.fresnel_power - 6.5).abs() < 1e-6);
        assert_eq!(
            cloned.state.map.recipe_meta.author.as_deref(),
            Some("test_author")
        );
        assert_eq!(cloned.state.map.features.len(), 1);
        assert_eq!(cloned.state.map.features[0].feature_type, "arborreal");
    }

    #[test]
    fn snapshot_carries_groups() {
        let mut state = EditorState::empty();
        state.groups.insert(
            7,
            crate::state::GroupRuntime {
                label: "test".to_string(),
                member_ids: std::collections::HashSet::new(),
                color_idx: 3,
                collapsed: false,
                is_subgraph: false,
                subgraph_inputs: Vec::new(),
                subgraph_outputs: Vec::new(),
                macro_params: Vec::new(),
            },
        );
        state.next_group_id = 8;
        let snap = Snapshot {
            state: state.clone(),
            description: "with-group".into(),
            painted_assets: HashMap::new(),
        };
        let cloned = snap.clone();
        assert_eq!(cloned.state.groups.len(), 1);
        assert_eq!(cloned.state.next_group_id, 8);
        assert_eq!(cloned.state.groups.get(&7).unwrap().label, "test");
    }
}

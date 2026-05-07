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

use crate::state::EditorState;

/// A captured editor state plus a human-readable label. The label is
/// shown in undo-history UIs and helps debugging.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub state: EditorState,
    pub description: String,
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
        };
        let cloned = snap.clone();
        assert_eq!(cloned.state.groups.len(), 1);
        assert_eq!(cloned.state.next_group_id, 8);
        assert_eq!(cloned.state.groups.get(&7).unwrap().label, "test");
    }
}

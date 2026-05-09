//! Canvas selection state owned by `BarEditorApp`.
//!
//! Tracks what's currently selected on the canvas: a primary node
//! (whose properties show in the side panel), a multi-selection of
//! nodes (for bulk move / delete), a selected group, a selected
//! connection (wire), and any group queued for deletion via the
//! confirm dialog.
//!
//! Selections are mutually exclusive between node-set vs. group vs.
//! connection: clicking on any one clears the others.

use bar_graph::{NodeId, PortId};

/// Grouped canvas selection state. See module docs.
#[derive(Default, Debug, Clone)]
pub struct SelectionState {
    /// Primary selected node -- the one whose properties show in
    /// the floating panel. `node ⊆ nodes` is always true: when a
    /// primary is set it's also a member of `nodes`.
    pub node: Option<NodeId>,
    /// Multi-selection set. Ctrl+click toggles membership; plain
    /// click clears and sets a single primary.
    pub nodes: std::collections::HashSet<NodeId>,
    /// Selected group, if any. Mutually exclusive with `node`/`nodes`
    /// and `connection`.
    pub group: Option<u64>,
    /// Selected connection (source port -> destination port), if any.
    /// Mutually exclusive with the others.
    pub connection: Option<(PortId, PortId)>,
    /// Group id queued for deletion (waiting for the user's confirm
    /// dialog response).
    pub pending_group_delete: Option<u64>,
}

use crate::app::BarEditorApp;

impl BarEditorApp {
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

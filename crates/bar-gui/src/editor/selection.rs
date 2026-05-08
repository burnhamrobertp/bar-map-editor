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

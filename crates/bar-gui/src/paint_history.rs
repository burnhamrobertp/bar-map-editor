//! Side table holding the asset bytes referenced by paint-undo snapshots.
//!
//! Why this exists: the snapshot system in `undo.rs` clones the graph
//! state for every mutation. Inlining painted asset bytes (heightmaps,
//! texture buffers -- megabytes each) into every snapshot would blow up
//! memory fast. Instead, snapshots carry compact pointers (content
//! hashes) into this store, and identical bytes dedupe to a single
//! `Arc<Vec<u8>>` shared across snapshots.
//!
//! The store grows monotonically during a session; we don't GC entries
//! whose snapshots have been evicted from the history stack (the
//! resulting "leak" is bounded by the lifetime of the editor process).
//! Worth revisiting if multi-hour painting sessions show memory growth
//! in practice.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Content-addressable byte store. Each blob is interned by a 64-bit
/// content hash; equal bytes always share the same `Arc`.
#[derive(Default)]
pub struct PaintHistoryStore {
    blobs: HashMap<u64, Arc<Vec<u8>>>,
}

impl PaintHistoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Hash the bytes; if not already interned, store them; return the
    /// hash for the caller to embed in a snapshot.
    pub fn register(&mut self, bytes: Vec<u8>) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut h);
        let id = h.finish();
        self.blobs.entry(id).or_insert_with(|| Arc::new(bytes));
        id
    }

    /// Look up the bytes for a previously-registered hash. Returns
    /// `None` if the entry has been GC'd (currently never, but kept in
    /// the API so callers handle the "missing" case correctly).
    pub fn get(&self, id: u64) -> Option<Arc<Vec<u8>>> {
        self.blobs.get(&id).cloned()
    }
}

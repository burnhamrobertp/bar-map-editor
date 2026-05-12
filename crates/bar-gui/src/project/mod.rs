//! Project lifecycle and persistence.

pub(crate) mod autosave;
pub(crate) mod lifecycle;
pub(crate) mod path;
pub(crate) mod persistence;
pub(crate) mod state;

pub(crate) use state::ProjectState;

//! Project lifecycle and persistence.
//!
//! Stage 1 lands `path` (pure path helpers used by save/load asset
//! packing). Stage 2+ lands `state`, `lifecycle`, `persistence`,
//! `autosave`, `sculpt_sidecar` modules per the architecture refactor
//! plan.

pub(crate) mod lifecycle;
pub(crate) mod path;

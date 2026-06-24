//! Channel ops: split a Color into per-channel Heightmaps and merge them back.

pub mod channel_merge;
pub mod channel_split;

use crate::nodes::def::NodeDef;

pub static NODES: &[&'static NodeDef] = &[&channel_split::DEF, &channel_merge::DEF];

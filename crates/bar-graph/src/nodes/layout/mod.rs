//! Layout: procedural primitive/spline compositor (one source node).

pub mod layout;

use crate::nodes::def::NodeDef;

pub static NODES: &[&'static NodeDef] = &[&layout::DEF];

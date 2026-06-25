//! Layout: procedural primitive/spline compositor (one source node).

// The Layout node's file is named after its node type (one-file-per-node /
// family-dir convention), which collides with the family module name. Intentional.
#[allow(clippy::module_inception)]
pub mod layout;

use crate::nodes::def::NodeDef;

pub static NODES: &[&NodeDef] = &[&layout::DEF];

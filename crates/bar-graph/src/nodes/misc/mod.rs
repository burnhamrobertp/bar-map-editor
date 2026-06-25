//! Miscellaneous nodes: an identity passthrough (Checkpoint), an N-way input
//! selector (Switch), and coastal erosion (CoastErosion).

pub mod checkpoint;
pub mod coast_erosion;
pub mod switch;

use crate::nodes::def::NodeDef;

pub static NODES: &[&NodeDef] = &[&checkpoint::DEF, &switch::DEF, &coast_erosion::DEF];

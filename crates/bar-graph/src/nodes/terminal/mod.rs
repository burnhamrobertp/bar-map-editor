//! Terminal node: the graph sink the export pipeline reads. A singleton
//! `FinalComposition` that composites managed paint layers over its inputs
//! and exposes the result on same-named ports for the bundler to consume.

pub mod final_composition;

use crate::nodes::def::NodeDef;

pub static NODES: &[&'static NodeDef] = &[&final_composition::DEF];

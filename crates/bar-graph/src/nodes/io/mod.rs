//! Subgraph boundary + file-source nodes: SubgraphInput / SubgraphOutput
//! (subgraph-only passthrough markers) and PassThrough / FileReference
//! (asset-bearing sources).

pub mod file_reference;
pub mod pass_through;
pub mod subgraph_input;
pub mod subgraph_output;

use crate::nodes::def::NodeDef;

pub static NODES: &[&NodeDef] = &[
    &subgraph_input::DEF,
    &subgraph_output::DEF,
    &pass_through::DEF,
    &file_reference::DEF,
];

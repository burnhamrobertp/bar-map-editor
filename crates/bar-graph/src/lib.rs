//! # bar-graph
//!
//! Node graph evaluation engine for BAR map editor.
//! Manages the DAG of processing nodes, handles incremental evaluation,
//! and schedules compute operations.

pub mod defaults;
pub mod engine;
pub mod eval;
pub mod node;
pub mod param_spec;
pub mod port;

pub use defaults::{
    biome_defaults, character_defaults, default_params, param_choices, param_is_color,
    param_side_effects, BiomeDefaults, CharacterDefaults,
};
pub use engine::GraphEngine;
pub use eval::{
    evaluate_graph, get_bundler_node_heightmap, get_bundler_node_texture, get_grassmap_output,
    get_heightmap_output, get_metalmap_output, get_node_output_color_named,
    get_node_output_heightmap, get_node_output_heightmap_named, get_normalmap_output,
    get_preview_heightmap, get_texture_output, get_typemap_output, EvalError, NodeExecutor,
    NodeOutputs,
};
pub use node::{Node, NodeId, NodeType, ParamValue};
pub use param_spec::{param_specs, validate_node_params, ParamError, ParamKind, ParamSpec};
pub use port::{FileRef, Port, PortCardinality, PortId, PortKind, PortPlacement, PortValue};

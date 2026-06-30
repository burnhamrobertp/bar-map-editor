//! Executors for the subgraph boundary + file-source nodes.

use std::collections::HashMap;

use bar_graph::{EvalError, NodeType, PortValue};

use super::{ExecCtx, ExecFn};

pub mod file_reference;
pub mod pass_through;
pub mod subgraph_input;
pub mod subgraph_output;

/// Both subgraph IO nodes are pure passthrough -- the value crossing the
/// boundary on the `value` input becomes the `value` output for the inner /
/// outer graph to read. Identical math for input and output; only the rendered
/// side of the subgraph boundary differs.
pub(crate) fn passthrough_value(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let mut outputs: HashMap<String, PortValue> = HashMap::new();
    if let Some(v) = ctx.inputs.get("value") {
        outputs.insert("value".to_string(), v.clone());
    }

    Ok(outputs)
}

pub fn register(m: &mut HashMap<NodeType, ExecFn>) {
    m.insert(NodeType::SubgraphInput, subgraph_input::exec);
    m.insert(NodeType::SubgraphOutput, subgraph_output::exec);
    m.insert(NodeType::PassThrough, pass_through::exec);
    m.insert(NodeType::FileReference, file_reference::exec);
}

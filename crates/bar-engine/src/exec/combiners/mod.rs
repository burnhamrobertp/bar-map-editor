//! Executors for the two-input combiners.

use std::collections::HashMap;

use bar_graph::{EvalError, NodeType, PortValue};

use super::{ExecCtx, ExecFn};
use crate::exec::shared::{apply_modulation, get_input_heightmap, get_optional_heightmap};
use shared::combine_heightmaps;

pub mod add;
pub mod blend;
pub mod mask_apply;
pub mod mask_select;
pub mod max;
pub mod min;
pub mod multiply;
pub mod shared;
pub mod subtract;

/// Shared body for the arithmetic combiners: combine a,b per-pixel then gate
/// by the optional mask back toward `a`.
pub(crate) fn run_binop(
    ctx: &ExecCtx,
    op: impl Fn(f32, f32) -> f32,
) -> Result<HashMap<String, PortValue>, EvalError> {
    let a = get_input_heightmap(ctx.inputs, "a")?;
    let b = get_input_heightmap(ctx.inputs, "b")?;
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let hm = combine_heightmaps(&a, &b, op);
    let hm = apply_modulation(&a, hm, None, mask.as_ref());
    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

pub fn register(m: &mut HashMap<NodeType, ExecFn>) {
    m.insert(NodeType::Blend, blend::exec);
    m.insert(NodeType::Add, add::exec);
    m.insert(NodeType::Subtract, subtract::exec);
    m.insert(NodeType::Multiply, multiply::exec);
    m.insert(NodeType::Max, max::exec);
    m.insert(NodeType::Min, min::exec);
    m.insert(NodeType::MaskSelect, mask_select::exec);
    m.insert(NodeType::MaskApply, mask_apply::exec);
}

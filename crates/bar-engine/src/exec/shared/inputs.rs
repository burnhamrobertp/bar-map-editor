//! Port -> value extractors shared across node families.

use std::collections::HashMap;

use bar_data::{ColorBuffer, Heightmap};
use bar_graph::{EvalError, PortValue};

pub(crate) fn get_input_heightmap(
    inputs: &HashMap<String, PortValue>,
    name: &str,
) -> Result<Heightmap, EvalError> {
    match inputs.get(name) {
        Some(PortValue::Heightmap(hm)) => Ok(hm.clone()),
        Some(PortValue::Mask(hm)) => Ok(hm.clone()),
        _ => Err(EvalError::MissingInput {
            port: name.to_string(),
        }),
    }
}

/// Optional heightmap input -- None if the port is unconnected or mistyped.
pub(crate) fn get_optional_heightmap(
    inputs: &HashMap<String, PortValue>,
    name: &str,
) -> Option<Heightmap> {
    match inputs.get(name) {
        Some(PortValue::Heightmap(hm)) | Some(PortValue::Mask(hm)) => Some(hm.clone()),
        _ => None,
    }
}

pub(crate) fn get_input_color(
    inputs: &HashMap<String, PortValue>,
    name: &str,
) -> Result<ColorBuffer, EvalError> {
    match inputs.get(name) {
        Some(PortValue::Color(cb)) => Ok(cb.clone()),
        _ => Err(EvalError::MissingInput {
            port: name.to_string(),
        }),
    }
}

/// Optional scalar input -- None if the port is unconnected or mistyped.
pub(crate) fn get_input_scalar(inputs: &HashMap<String, PortValue>, name: &str) -> Option<f32> {
    match inputs.get(name) {
        Some(PortValue::Scalar(s)) => Some(*s),
        _ => None,
    }
}

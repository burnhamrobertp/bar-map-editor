//! Typed param getters shared across node families.

use std::collections::HashMap;

use bar_graph::ParamValue;

pub(crate) fn get_float(params: &HashMap<String, ParamValue>, key: &str, default: f32) -> f32 {
    match params.get(key) {
        Some(ParamValue::Float(v)) => *v,
        _ => default,
    }
}

pub(crate) fn get_uint(params: &HashMap<String, ParamValue>, key: &str, default: u32) -> u32 {
    match params.get(key) {
        Some(ParamValue::UInt(v)) => *v,
        _ => default,
    }
}

pub(crate) fn get_string<'a>(
    params: &'a HashMap<String, ParamValue>,
    key: &str,
    default: &'a str,
) -> &'a str {
    match params.get(key) {
        Some(ParamValue::String(s)) => s.as_str(),
        _ => default,
    }
}

pub(crate) fn get_bool(params: &HashMap<String, ParamValue>, key: &str, default: bool) -> bool {
    match params.get(key) {
        Some(ParamValue::Bool(v)) => *v,
        _ => default,
    }
}

use std::collections::HashMap;

use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{CustomPanel, NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef};
use crate::port::PortKind;

/// Output interpretation of the composited coverage field; surfaced as a
/// dropdown.
const MODES: &[&str] = &["ridge", "valley", "mask"];
/// Symmetry multiplier: duplicates every item across reflection / rotation
/// axes so BAR-style symmetric maps need only one authored copy.
const SYMMETRY: &[&str] = &[
    "none",
    "mirror_x",
    "mirror_y",
    "mirror_xy",
    "rotate_180",
    "rotate_90",
];
const ITEM_TYPES: &[&str] = &["ellipse", "rectangle", "line", "spline"];

/// Number of per-item slots the dynamic params populate. `item_count` caps how
/// many the executor reads.
const SLOTS: usize = 8;

static INPUTS: &[PortDef] = &[PortDef::one("mask", "Mask", PortKind::Mask)];
static OUTPUTS: &[PortDef] = &[PortDef::one("output", "Heightmap", PortKind::Heightmap)];

static PARAMS: &[ParamDef] = &[
    ParamDef {
        key: "item_count",
        default: || ParamValue::UInt(1),
        ui: ParamUi::UIntRange { min: 1, max: 8 },
    },
    ParamDef {
        key: "mode",
        default: || ParamValue::String("ridge".to_string()),
        ui: ParamUi::Choices(MODES),
    },
    ParamDef {
        key: "symmetry",
        default: || ParamValue::String("none".to_string()),
        ui: ParamUi::Choices(SYMMETRY),
    },
];

/// Per-slot params. Each slot carries the primitive fields (type / x / y / rx /
/// ry / angle) plus the spline fields (points / closed / fill / width) so a
/// slot can switch kinds without losing data; only the fields its `type_i`
/// selects are read by the executor.
fn dynamic_params() -> HashMap<String, ParamValue> {
    let mut m = HashMap::new();
    for i in 0..SLOTS {
        let height = if i == 0 { 0.5 } else { 0.0 };
        m.insert(
            format!("type_{i}"),
            ParamValue::String("ellipse".to_string()),
        );
        m.insert(format!("x_{i}"), ParamValue::Float(0.5));
        m.insert(format!("y_{i}"), ParamValue::Float(0.5));
        m.insert(format!("rx_{i}"), ParamValue::Float(0.2));
        m.insert(format!("ry_{i}"), ParamValue::Float(0.2));
        m.insert(format!("angle_{i}"), ParamValue::Float(0.0));
        m.insert(format!("height_{i}"), ParamValue::Float(height));
        m.insert(format!("falloff_{i}"), ParamValue::Float(0.5));
        m.insert(format!("points_{i}"), ParamValue::Spline(Vec::new()));
        m.insert(format!("closed_{i}"), ParamValue::Bool(false));
        m.insert(format!("fill_{i}"), ParamValue::Bool(false));
        m.insert(format!("width_{i}"), ParamValue::Float(0.05));
    }
    m
}

fn dynamic_param_ui(key: &str) -> Option<ParamUi> {
    if key.starts_with("type_") {
        return Some(ParamUi::Choices(ITEM_TYPES));
    }
    if key.starts_with("x_")
        || key.starts_with("y_")
        || key.starts_with("rx_")
        || key.starts_with("ry_")
        || key.starts_with("height_")
        || key.starts_with("falloff_")
    {
        return Some(ParamUi::FloatRange { min: 0.0, max: 1.0 });
    }
    if key.starts_with("angle_") {
        return Some(ParamUi::FloatRange {
            min: 0.0,
            max: 360.0,
        });
    }
    if key.starts_with("width_") {
        return Some(ParamUi::FloatRange {
            min: 0.001,
            max: 0.5,
        });
    }
    if key.starts_with("closed_") || key.starts_with("fill_") {
        return Some(ParamUi::Bool);
    }
    if key.starts_with("points_") {
        return Some(ParamUi::Spline);
    }

    None
}

pub static DEF: NodeDef = NodeDef {
    node_type: NodeType::Layout,
    label: "Layout",
    category: NodeCategory::Generator,
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    caps: NodeCaps::NONE,
    dynamic_params: Some(dynamic_params),
    dynamic_param_ui: Some(dynamic_param_ui),
    param_side_effects: None,
    post_build: None,
    scalar_bindable: &[],
    custom_panel: Some(CustomPanel::Layout),
};

use crate::node::{Node, NodeType, ParamValue};
use crate::nodes::def::{
    CustomPanel, NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi, PortDef,
};
use crate::port::PortKind;

// Default 2 inputs; `post_build` / `resize_switch_ports` grows them to
// `input_count` after construction.
static INPUTS: &[PortDef] = &[
    PortDef::one("input_0", "Input 0", PortKind::Heightmap),
    PortDef::one("input_1", "Input 1", PortKind::Heightmap),
];
static OUTPUTS: &[PortDef] = &[PortDef::one("output", "Output", PortKind::Heightmap)];

static PARAMS: &[ParamDef] = &[
    ParamDef { key: "input_count", default: || ParamValue::UInt(2), ui: ParamUi::UIntRange { min: 2, max: 8 } },
    ParamDef { key: "selected", default: || ParamValue::UInt(0), ui: ParamUi::UIntRange { min: 0, max: 7 } },
];

fn resize_ports(node: &mut Node) {
    let n = match node.params.get("input_count") {
        Some(ParamValue::UInt(c)) => *c,
        _ => 2,
    };
    node.resize_switch_ports(n);
}

pub static DEF: NodeDef = NodeDef {
    node_type: NodeType::Switch,
    label: "Switch",
    category: NodeCategory::Combiner,
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    caps: NodeCaps::NONE,
    dynamic_params: None,
    dynamic_param_ui: None,
    param_side_effects: None,
    post_build: Some(resize_ports),
    scalar_bindable: &[],
    custom_panel: Some(CustomPanel::Switch),
};

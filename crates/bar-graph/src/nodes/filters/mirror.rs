use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};

static MODES: &[&str] = &[
    "mirror_x",
    "mirror_y",
    "mirror_xy",
    "rotate_180",
    "rotate_90_4way",
    "average_x",
    "average_y",
    "average_xy",
    "average_180",
    "average_90_4way",
];
static PARAMS: &[ParamDef] = &[ParamDef {
    key: "mode",
    default: || ParamValue::String("mirror_x".to_string()),
    ui: ParamUi::Choices(MODES),
}];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::Mirror,
    "Mirror",
    NodeCategory::Filter,
    super::shared::INPUT_MASK_IN,
    super::shared::OUTPUT,
    PARAMS,
    NodeCaps::NONE,
);

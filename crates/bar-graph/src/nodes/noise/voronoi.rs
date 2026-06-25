use crate::node::{NodeType, ParamValue};
use crate::nodes::def::{NodeCaps, NodeCategory, NodeDef, ParamDef, ParamUi};
use crate::port::PortKind;
use crate::nodes::def::PortDef;

static OUTPUT: &[PortDef] = &[PortDef::one("output", "Heightmap", PortKind::Heightmap)];
static PARAMS: &[ParamDef] = &[
    ParamDef { key: "frequency", default: || ParamValue::Float(8.0), ui: ParamUi::FloatFree },
    ParamDef { key: "seed", default: || ParamValue::UInt(0), ui: ParamUi::UIntFree },
    ParamDef { key: "mode", default: || ParamValue::String("f1".to_string()), ui: ParamUi::Choices(&["f1", "f2", "f2_f1", "cell"]) },
];

pub static DEF: NodeDef = NodeDef::basic(
    NodeType::Voronoi,
    "Voronoi",
    NodeCategory::Generator,
    super::shared::CONTROL_IN,
    OUTPUT,
    PARAMS,
    NodeCaps::source(),
);

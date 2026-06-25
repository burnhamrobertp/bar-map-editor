use crate::node::NodeType;
use crate::nodes::def::NodeDef;

pub static DEF: NodeDef = super::shared::fbm_def(NodeType::WorleyNoise, "Worley Noise");

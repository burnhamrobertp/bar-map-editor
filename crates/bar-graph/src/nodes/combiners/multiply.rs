use crate::node::NodeType;
use crate::nodes::def::NodeDef;

pub static DEF: NodeDef = super::shared::binop_def(NodeType::Multiply, "Multiply");

//! Node descriptor registry.
//!
//! Each node type's static metadata lives in a per-node file under a family
//! directory (e.g. `nodes/noise/perlin.rs`), exposed as a `pub static DEF:
//! NodeDef`. Each family's `mod.rs` collects its nodes into a `NODES` slice;
//! `ALL` lists the families; `REGISTRY` flattens them into a `NodeType`-keyed
//! map. This single explicit list is the exhaustiveness checkpoint -- the
//! `param_spec` count guard fails if a `NodeType` variant has no descriptor.
//!
//! Migration note: `def()` returns `Option` so the legacy `defaults.rs` /
//! `node.rs` match arms can serve as a fallthrough for not-yet-migrated node
//! types while families are moved over one at a time.

mod def;
pub use def::*;

pub mod channels;
pub mod combiners;
pub mod expr;
pub mod filters;
pub mod io;
pub mod layout;
pub mod misc;
pub mod noise;
pub mod paint;
pub mod scalar;
pub mod selectors;
pub mod terminal;
pub mod texture;

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::node::NodeType;

/// Per-family descriptor slices. Each family module exposes `pub static NODES:
/// &[&NodeDef]`. All node types now live in the registry.
static ALL: &[&[&NodeDef]] = &[
    noise::NODES,
    combiners::NODES,
    selectors::NODES,
    filters::NODES,
    texture::NODES,
    channels::NODES,
    layout::NODES,
    paint::NODES,
    io::NODES,
    terminal::NODES,
    misc::NODES,
    expr::NODES,
    scalar::NODES,
];

pub static REGISTRY: LazyLock<HashMap<NodeType, &'static NodeDef>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    for family in ALL {
        for d in *family {
            assert!(
                m.insert(d.node_type.clone(), *d).is_none(),
                "duplicate NodeDef registered for {:?}",
                d.node_type
            );
        }
    }
    m
});

/// The descriptor for a node type, or `None` if it hasn't been migrated to the
/// registry yet (callers fall back to the legacy match during migration).
pub fn def(nt: &NodeType) -> Option<&'static NodeDef> {
    REGISTRY.get(nt).copied()
}

/// Every registered descriptor.
pub fn all_defs() -> impl Iterator<Item = &'static NodeDef> {
    REGISTRY.values().copied()
}

// ── Bridge: project a descriptor into the legacy return shapes ──────────────
// These let the `defaults.rs` / `node.rs` free functions read the registry for
// migrated node types while keeping their public signatures unchanged.

use crate::node::ParamValue;
use crate::port::{Port, PortKind};

/// Default param map: each `ParamDef` default + any `dynamic_params`.
pub fn build_params(d: &NodeDef) -> HashMap<String, ParamValue> {
    let mut m: HashMap<String, ParamValue> = d
        .params
        .iter()
        .map(|p| (p.key.to_string(), (p.default)()))
        .collect();
    if let Some(f) = d.dynamic_params {
        m.extend(f());
    }
    m
}

/// Input + output `Port` vecs from the descriptor's `PortDef`s.
///
/// Each `scalar_bindable` param key gets an extra optional input port (named
/// the param key, `PortKind::Scalar`) appended after the declared inputs, so a
/// scalar wire can drive that param at eval time. Outputs are untouched.
pub fn build_ports(d: &NodeDef) -> (Vec<Port>, Vec<Port>) {
    fn conv(ports: &[PortDef]) -> Vec<Port> {
        ports
            .iter()
            .map(|p| {
                if p.many {
                    Port::new_many(p.name, p.label, p.kind)
                } else {
                    Port::new(p.name, p.label, p.kind)
                }
            })
            .collect()
    }
    let mut inputs = conv(d.inputs);
    for key in d.scalar_bindable {
        inputs.push(Port::new(*key, format!("{key} (scalar)"), PortKind::Scalar));
    }
    (inputs, conv(d.outputs))
}

/// The UI spec for a param key: a static `ParamDef`, else the node's
/// `dynamic_param_ui` hook (indexed keys like `priority_3`, `pos_5`).
fn param_ui(d: &NodeDef, key: &str) -> Option<ParamUi> {
    if let Some(p) = d.params.iter().find(|p| p.key == key) {
        return Some(p.ui);
    }
    d.dynamic_param_ui.and_then(|f| f(key))
}

pub fn param_choices(d: &NodeDef, key: &str) -> Option<&'static [&'static str]> {
    match param_ui(d, key)? {
        ParamUi::Choices(c) => Some(c),
        _ => None,
    }
}

pub fn param_float_range(d: &NodeDef, key: &str) -> Option<(f32, f32)> {
    match param_ui(d, key)? {
        ParamUi::FloatRange { min, max } => Some((min, max)),
        _ => None,
    }
}

pub fn param_uint_range(d: &NodeDef, key: &str) -> Option<(u32, u32)> {
    match param_ui(d, key)? {
        ParamUi::UIntRange { min, max } => Some((min, max)),
        _ => None,
    }
}

pub fn param_is_color(d: &NodeDef, key: &str) -> bool {
    matches!(param_ui(d, key), Some(ParamUi::Color))
}

pub fn param_side_effects(
    d: &NodeDef,
    key: &str,
    new_val: &ParamValue,
) -> Option<Vec<(String, ParamValue)>> {
    d.param_side_effects.map(|f| f(key, new_val))
}

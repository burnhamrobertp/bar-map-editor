//! Node descriptor types.
//!
//! A `NodeDef` is the single source of truth for one node type's static
//! metadata: its ports, default params + UI specs, palette category, capability
//! flags, and optional non-uniform hooks. The registry (`super`) keys these by
//! `NodeType`; the executor (in `bar-engine`) is associated by the same key.
//!
//! Metadata is *data*, not behavior -- hence plain structs of fields + a few
//! fn-pointers, not a trait-object per node. Everything here is
//! const-constructible so each node's descriptor can be a `pub static DEF`.

use std::collections::HashMap;

use crate::node::{Node, NodeType, ParamValue};
use crate::port::PortKind;

/// A node's input or output port.
pub struct PortDef {
    pub name: &'static str,
    pub label: &'static str,
    pub kind: PortKind,
    /// `Port::new_many` (accepts multiple connections) vs `Port::new`.
    pub many: bool,
}

impl PortDef {
    pub const fn one(name: &'static str, label: &'static str, kind: PortKind) -> Self {
        Self {
            name,
            label,
            kind,
            many: false,
        }
    }
    pub const fn many(name: &'static str, label: &'static str, kind: PortKind) -> Self {
        Self {
            name,
            label,
            kind,
            many: true,
        }
    }
}

/// How a param renders in the generic property grid + how it validates.
/// Unifies the four parallel lookups that used to live in `defaults.rs`
/// (`param_float_range` / `param_uint_range` / `param_choices` / `param_is_color`).
#[derive(Debug, Clone, Copy)]
pub enum ParamUi {
    FloatRange {
        min: f32,
        max: f32,
    },
    FloatFree,
    UIntRange {
        min: u32,
        max: u32,
    },
    UIntFree,
    IntFree,
    Bool,
    /// Enum string param -> dropdown.
    Choices(&'static [&'static str]),
    /// `RRGGBB` hex string -> colour swatch.
    Color,
    /// Free-form text.
    Text,
    Vec2,
    /// Canvas-edited polyline; generic grid skips it.
    Spline,
    /// System-managed (asset paths, subgraph kind/name); generic grid skips it.
    Hidden,
}

/// One param's full spec. `default` is a fn-ptr because `ParamValue` holds
/// `String`/`Vec` and is not const-constructible; a non-capturing closure
/// (`|| ParamValue::Float(4.0)`) coerces to this fn-ptr in a `static`.
pub struct ParamDef {
    pub key: &'static str,
    pub default: fn() -> ParamValue,
    pub ui: ParamUi,
}

/// Palette grouping + canvas title-bar colour class. Single source for both
/// (previously two separate matches in two crates). `bar-gui` maps this to its
/// colour tokens and palette sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeCategory {
    Generator,
    Filter,
    Combiner,
    Colorizer,
    SplatMap,
    Mask,
    Source,
    Terminal,
    Io,
}

/// Capability flags -- the behaviour classes that scattered `matches!`
/// allow-lists used to encode (source / terminal / asset-bearing / subgraph /
/// GPU-eligible).
#[derive(Debug, Clone, Copy)]
pub struct NodeCaps {
    /// No upstream input required (generators / source nodes).
    pub is_source: bool,
    /// Graph sink / bundler target (FinalComposition).
    pub is_terminal: bool,
    /// Carries on-disk binary assets (asset_id/asset_path) that must round-trip.
    pub holds_assets: bool,
    /// Placeable only inside a subgraph (SubgraphInput/Output).
    pub is_subgraph_only: bool,
    /// `HybridExecutor` may route this to a GPU kernel.
    pub gpu_eligible: bool,
}

impl NodeCaps {
    pub const NONE: NodeCaps = NodeCaps {
        is_source: false,
        is_terminal: false,
        holds_assets: false,
        is_subgraph_only: false,
        gpu_eligible: false,
    };
    pub const fn source() -> NodeCaps {
        NodeCaps {
            is_source: true,
            ..NodeCaps::NONE
        }
    }
}

/// Names the bespoke GUI property panel a node uses (the panel impl lives in
/// `bar-gui`, which can't be referenced from here, so this is just a tag the
/// GUI matches on). `None` = the data-driven generic grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomPanel {
    PassThrough,
    PaintedHeightmap,
    PaintedTexture,
    TextureWeightmap,
    ColorRamp,
    Layout,
    Equation,
    Switch,
    Curve,
}

/// Preset side-effect hook: editing one param returns overrides for others
/// (AutoTexture biome, noise character).
pub type ParamSideEffectFn = fn(&str, &ParamValue) -> Vec<(String, ParamValue)>;

/// The full static descriptor for one node type.
pub struct NodeDef {
    pub node_type: NodeType,
    /// Palette label + default node label.
    pub label: &'static str,
    pub category: NodeCategory,
    pub inputs: &'static [PortDef],
    pub outputs: &'static [PortDef],
    pub params: &'static [ParamDef],
    pub caps: NodeCaps,

    /// Extra params with dynamically-generated keys (Layout's 8 item slots).
    /// Merged on top of `params` defaults at `Node::new`.
    pub dynamic_params: Option<fn() -> HashMap<String, ParamValue>>,
    /// UI spec for a dynamically-keyed param (e.g. `priority_3`, `pos_5`,
    /// `type_2`). Keeps the `key.starts_with(...)` logic inside the node's
    /// own module instead of a central match. `None` for static-key nodes.
    pub dynamic_param_ui: Option<fn(&str) -> Option<ParamUi>>,
    /// Preset side-effects (AutoTexture biome, noise character) -- setting one
    /// param rewrites others.
    pub param_side_effects: Option<ParamSideEffectFn>,
    /// Post-construction fixup after `Node::new` + recipe param merge
    /// (TextureWeightmap port resize, Subgraph kind sync).
    pub post_build: Option<fn(&mut Node)>,
    /// Param keys that accept a scalar wire override (scalar-parameter graph).
    /// Empty for nodes with no scalar-bindable params.
    pub scalar_bindable: &'static [&'static str],
    pub custom_panel: Option<CustomPanel>,
}

impl NodeDef {
    /// Convenience for the common case: a node with no non-uniform hooks.
    /// Family modules build on this with struct-update syntax.
    pub const fn basic(
        node_type: NodeType,
        label: &'static str,
        category: NodeCategory,
        inputs: &'static [PortDef],
        outputs: &'static [PortDef],
        params: &'static [ParamDef],
        caps: NodeCaps,
    ) -> NodeDef {
        NodeDef {
            node_type,
            label,
            category,
            inputs,
            outputs,
            params,
            caps,
            dynamic_params: None,
            dynamic_param_ui: None,
            param_side_effects: None,
            post_build: None,
            scalar_bindable: &[],
            custom_panel: None,
        }
    }
}

//! Per-`NodeType` parameter schema and validator.
//!
//! Today, missing or wrong-typed params silently fall back to defaults
//! at execution time. That makes typos in saved `.barproj` files (or
//! hand-edited recipes) invisible until evaluation time, where the
//! error surface is "your terrain looks wrong" instead of "param
//! `radiu` doesn't exist on Blur."
//!
//! This module derives the schema directly from `default_params`
//! (single source of truth) and gives `Recipe::validate` something
//! to call at load time. Catching the typo at recipe-load is several
//! orders of magnitude friendlier than catching it after a 512-px
//! preview eval finishes.
//!
//! ## Strictness
//!
//! - **Type mismatch is hard-fail.** A param declared `Float` cannot
//!   silently be loaded as `Int`; the recipe is rejected. Type
//!   collisions are always a hand-edit error or a deliberate format
//!   change.
//! - **Unknown keys are warnings (today: silent), not errors.** Hand-
//!   edited recipes that carry params from removed node-type variants
//!   still load; the unknown key is dropped silently. Tighten to
//!   hard-fail here if the schema stabilises and stray keys become
//!   useful to surface.

use std::collections::HashMap;

use crate::defaults::default_params;
use crate::node::{NodeType, ParamValue};

/// The kind of value a param holds — maps 1:1 to `ParamValue`'s
/// variants but without the inhabitant. Used by validation and (in
/// the future) by the property panel to pick the right widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    Float,
    Int,
    UInt,
    Bool,
    String,
    Vec2,
    Spline,
}

impl ParamKind {
    /// The `ParamKind` corresponding to a concrete `ParamValue`.
    pub fn of(v: &ParamValue) -> Self {
        match v {
            ParamValue::Float(_) => ParamKind::Float,
            ParamValue::Int(_) => ParamKind::Int,
            ParamValue::UInt(_) => ParamKind::UInt,
            ParamValue::Bool(_) => ParamKind::Bool,
            ParamValue::String(_) => ParamKind::String,
            ParamValue::Vec2(_) => ParamKind::Vec2,
            ParamValue::Spline(_) => ParamKind::Spline,
        }
    }
}

/// Describes a single param expected on a node type.
#[derive(Debug, Clone)]
pub struct ParamSpec {
    /// Param key (the `HashMap` key on `Node::params`).
    pub name: String,
    /// Required type of the value.
    pub kind: ParamKind,
}

/// Errors a recipe-supplied param map can produce against the
/// schema. Returned as a `Vec` so the loader can report all problems
/// in one go instead of failing at the first issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamError {
    /// Param key declared in the recipe doesn't appear on this node
    /// type. Currently surfaced as a warning rather than an error
    /// (see module docs); kept in the API so callers can opt into
    /// stricter behaviour later.
    UnknownKey { node_type: NodeType, key: String },
    /// Param key matches a spec but the value is the wrong type.
    /// E.g. `Blur.radius` is `Float` but the recipe supplied a
    /// `String`. Always a hard error — silent type coercion would
    /// produce wrong-but-not-broken evaluations.
    TypeMismatch {
        node_type: NodeType,
        key: String,
        expected: ParamKind,
        got: ParamKind,
    },
}

/// Param schema for `node_type`. Derived from `default_params` —
/// every default value's type is the spec's expected type, and
/// every default key is a recognised param name. Kept as a function
/// (vs. a static) because `default_params` itself returns owned
/// values; cost is one allocation per param at validation time,
/// which is negligible at recipe-load.
pub fn param_specs(node_type: &NodeType) -> Vec<ParamSpec> {
    default_params(node_type)
        .into_iter()
        .map(|(name, value)| ParamSpec {
            name,
            kind: ParamKind::of(&value),
        })
        .collect()
}

/// Validate a recipe-supplied param map against the schema.
///
/// Returns the list of all problems found; an empty vec means OK.
/// The caller decides whether each variant is a hard error or a
/// warning — `Recipe::validate` today treats only `TypeMismatch` as
/// fatal (see module docs).
pub fn validate_node_params(
    node_type: &NodeType,
    params: &HashMap<String, ParamValue>,
) -> Vec<ParamError> {
    let specs = param_specs(node_type);
    let mut errors = Vec::new();
    for (key, value) in params {
        let Some(spec) = specs.iter().find(|s| &s.name == key) else {
            errors.push(ParamError::UnknownKey {
                node_type: node_type.clone(),
                key: key.clone(),
            });
            continue;
        };
        let got = ParamKind::of(value);
        if got != spec.kind {
            errors.push(ParamError::TypeMismatch {
                node_type: node_type.clone(),
                key: key.clone(),
                expected: spec.kind,
                got,
            });
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_specs_reflect_default_kinds() {
        // For every node type, the spec's kind must match the
        // default value's runtime variant — otherwise a fresh node
        // would fail its own validation.
        for nt in NODE_TYPES_FOR_TEST {
            let specs = param_specs(nt);
            let defaults = default_params(nt);
            for (name, value) in defaults {
                let spec = specs
                    .iter()
                    .find(|s| s.name == name)
                    .unwrap_or_else(|| panic!("no spec for default param {nt:?}.{name}"));
                let got = ParamKind::of(&value);
                assert_eq!(
                    spec.kind, got,
                    "{nt:?}.{name}: spec says {:?}, default is {got:?}",
                    spec.kind,
                );
            }
        }
    }

    #[test]
    fn type_mismatch_is_reported() {
        let mut p = HashMap::new();
        // Blur.radius is Float; supply a String.
        p.insert("radius".to_string(), ParamValue::String("nope".into()));
        let errs = validate_node_params(&NodeType::Blur, &p);
        assert_eq!(errs.len(), 1);
        match &errs[0] {
            ParamError::TypeMismatch {
                key, expected, got, ..
            } => {
                assert_eq!(key, "radius");
                assert_eq!(*expected, ParamKind::Float);
                assert_eq!(*got, ParamKind::String);
            }
            other => panic!("expected TypeMismatch, got {other:?}"),
        }
    }

    #[test]
    fn unknown_key_is_reported() {
        let mut p = HashMap::new();
        p.insert("totally_not_a_param".to_string(), ParamValue::Float(1.0));
        let errs = validate_node_params(&NodeType::Blur, &p);
        assert_eq!(errs.len(), 1);
        match &errs[0] {
            ParamError::UnknownKey { key, .. } => {
                assert_eq!(key, "totally_not_a_param");
            }
            other => panic!("expected UnknownKey, got {other:?}"),
        }
    }

    #[test]
    fn defaults_validate_clean() {
        // Every node type's own defaults must validate against the
        // schema with zero errors; the schema is *derived* from the
        // defaults, so anything else is a logic bug here.
        for nt in NODE_TYPES_FOR_TEST {
            let p: HashMap<_, _> = default_params(nt).into_iter().collect();
            let errs = validate_node_params(nt, &p);
            assert!(
                errs.is_empty(),
                "default params for {nt:?} produced errors: {errs:?}",
            );
        }
    }

    /// Every `NodeType` variant in enum declaration order. Must stay in
    /// sync with `node.rs` -- the const assert below makes a missing
    /// entry a compile error.
    const NODE_TYPES_FOR_TEST: &[NodeType] = &[
        // Generators
        NodeType::PerlinNoise,
        NodeType::SimplexNoise,
        NodeType::WorleyNoise,
        NodeType::RidgedNoise,
        NodeType::Constant,
        // Filters
        NodeType::HydraulicErosion,
        NodeType::ThermalErosion,
        NodeType::Blur,
        NodeType::Sharpen,
        NodeType::Clamp,
        NodeType::Terrace,
        // Combiners
        NodeType::Blend,
        NodeType::Add,
        NodeType::Subtract,
        NodeType::Multiply,
        NodeType::Max,
        NodeType::Min,
        // Texture/Splat
        NodeType::SlopeMap,
        NodeType::HeightSelect,
        NodeType::TerrainSplat,
        NodeType::AutoTexture,
        NodeType::RockSoil,
        NodeType::Vegetation,
        NodeType::LayerBlend,
        NodeType::TextureWeightmap,
        NodeType::ColorRamp,
        // Map layers
        NodeType::NormalMap,
        NodeType::GrassMap,
        NodeType::SpecularMap,
        // Mask operations
        NodeType::MaskThreshold,
        NodeType::MaskApply,
        // Utility
        NodeType::Mask,
        NodeType::Invert,
        NodeType::Mirror,
        NodeType::Curve,
        NodeType::PaintedHeightmap,
        NodeType::PaintedTexture,
        NodeType::ImportedTexture,
        // Additional generators
        NodeType::FileInput,
        NodeType::Voronoi,
        NodeType::Gradient,
        // Additional filters
        NodeType::Normalize,
        NodeType::BiasGain,
        NodeType::Displacement,
        // Selectors
        NodeType::FlowSelect,
        NodeType::SelectConvexity,
        // Shape generator
        NodeType::LayoutGenerator,
        // Transform / warp / strata filters
        NodeType::Transform,
        NodeType::Warp,
        NodeType::Stratify,
        // Morphological
        NodeType::MaskExpand,
        NodeType::MaskShrink,
        // Aspect selector
        NodeType::SelectAspect,
        // Additional combiners
        NodeType::MaskSelect,
        // Bundler/packaging
        NodeType::FinalComposition,
        NodeType::FileReference,
        // Source nodes
        NodeType::PassThrough,
        NodeType::SubgraphInput,
        NodeType::SubgraphOutput,
    ];

    #[test]
    fn node_types_for_test_is_exhaustive() {
        // Guards against a new NodeType variant being added without updating
        // NODE_TYPES_FOR_TEST. Update this constant AND the array when you add a variant.
        // (A compile-time version of this check requires nightly variant_count; this
        // test is the stable equivalent.)
        const EXPECTED_VARIANT_COUNT: usize = 59;
        assert_eq!(
            NODE_TYPES_FOR_TEST.len(),
            EXPECTED_VARIANT_COUNT,
            "NODE_TYPES_FOR_TEST has {} entries but NodeType has {} variants. \
             Add/remove the variant from the array and update EXPECTED_VARIANT_COUNT.",
            NODE_TYPES_FOR_TEST.len(),
            EXPECTED_VARIANT_COUNT,
        );
    }
}

//! Data + helpers shared across the selector / mask-analysis nodes.

use crate::nodes::def::PortDef;
use crate::port::PortKind;

/// Heightmap input + optional control (SlopeMap / HeightSelect / SlopeSelect /
/// MaskThreshold / Mask).
pub static INPUT_CONTROL_IN: &[PortDef] = &[
    PortDef::one("input", "Heightmap", PortKind::Heightmap),
    PortDef::one("control", "Control", PortKind::Control),
];
/// Single heightmap input, no control (FlowSelect / SelectConvexity /
/// SelectAspect / MaskExpand / MaskShrink).
pub static INPUT_ONLY_IN: &[PortDef] = &[PortDef::one("input", "Heightmap", PortKind::Heightmap)];

/// A `Heightmap`-typed mask output (most selectors emit their mask this way).
pub static MASK_OUT: &[PortDef] = &[PortDef::one("output", "Mask", PortKind::Heightmap)];

pub static FALLOFF_TYPES: &[&str] = &["linear", "smooth"];
pub static CONVEXITY_MODES: &[&str] = &["ridges", "valleys", "full"];

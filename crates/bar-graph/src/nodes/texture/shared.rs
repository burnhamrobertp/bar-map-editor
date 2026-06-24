//! Data + helpers shared across the texture / map-layer nodes.

use crate::nodes::def::PortDef;
use crate::port::PortKind;

/// A `Color`-typed texture output (the colorizers + NormalMap emit this).
pub static TEXTURE_OUT: &[PortDef] = &[PortDef::one("output", "Texture", PortKind::Color)];

/// Heightmap + slope inputs gated by a mask (RockSoil / Vegetation).
pub static INPUT_SLOPE_MASK_IN: &[PortDef] = &[
    PortDef::one("input", "Heightmap", PortKind::Heightmap),
    PortDef::one("slope", "Slope Map", PortKind::Heightmap),
    PortDef::one("mask", "Mask", PortKind::Mask),
];

/// Heightmap + slope inputs with control + mask (SpecularMap).
pub static INPUT_SLOPE_CONTROL_MASK_IN: &[PortDef] = &[
    PortDef::one("input", "Heightmap", PortKind::Heightmap),
    PortDef::one("slope", "Slope Map", PortKind::Heightmap),
    PortDef::one("control", "Control", PortKind::Control),
    PortDef::one("mask", "Mask", PortKind::Mask),
];

/// Heightmap input gated by a mask (ColorRamp / NormalMap).
pub static INPUT_MASK_IN: &[PortDef] = &[
    PortDef::one("input", "Heightmap", PortKind::Heightmap),
    PortDef::one("mask", "Mask", PortKind::Mask),
];

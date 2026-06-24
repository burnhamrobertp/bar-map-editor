//! Port sets shared across the filter nodes.

use crate::nodes::def::PortDef;
use crate::port::PortKind;

/// Input + Control + Mask (Blur / ThermalErosion / Clamp / Terrace / Sharpen /
/// Curve / BiasGain).
pub static INPUT_CONTROL_MASK_IN: &[PortDef] = &[
    PortDef::one("input", "Input", PortKind::Heightmap),
    PortDef::one("control", "Control", PortKind::Control),
    PortDef::one("mask", "Mask", PortKind::Mask),
];

/// Input + Mask (Invert / Mirror / Normalize / Transform / Stratify).
pub static INPUT_MASK_IN: &[PortDef] = &[
    PortDef::one("input", "Input", PortKind::Heightmap),
    PortDef::one("mask", "Mask", PortKind::Mask),
];

pub static OUTPUT: &[PortDef] = &[PortDef::one("output", "Output", PortKind::Heightmap)];

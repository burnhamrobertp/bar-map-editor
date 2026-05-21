//! sRGB <-> linear conversion (IEC 61966-2-1).
//!
//! ## Why this exists (UI only)
//!
//! BME's renderer is end-to-end gamma-incorrect by design: it mirrors
//! BAR's pipeline, which uploads all textures with non-sRGB internal
//! formats, runs every shader multiplication in sRGB-perceptual space,
//! and writes to non-sRGB framebuffers. The display device gamma-
//! decodes the resulting byte values. The whole pipeline is "wrong"
//! by modern standards but visually consistent end-to-end, and BME
//! has to match it so map authors see what their map will look like
//! in-engine. No sRGB conversion happens anywhere in the render path.
//!
//! These helpers exist purely for **UI colour pickers**. egui's
//! `color_edit_button_rgb` interprets its input as linear RGB and
//! sRGB-encodes the swatch for screen display. Mapinfo colour values
//! are sRGB-perceptual, so without conversion the swatch shows the
//! wrong colour for what the user typed. The picker callback in
//! `bar-gui::panels::mapinfo_editor::color_rgb` decodes for the
//! swatch, then re-encodes the user-edited value before writing back
//! to the recipe -- the recipe always holds sRGB-perceptual values.
//!
//! Do NOT call these helpers anywhere in the shader-uniform upload
//! path or texture-format selection. Those must stay in BAR's
//! perceptual-everywhere space to match the engine's visible
//! appearance. Tests below pin the maths.

/// sRGB-encoded value in `[0, 1]` -> linear-space value in `[0, 1]`.
/// Piecewise function from the sRGB spec: linear branch below `0.04045`,
/// 2.4-power branch above.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear value -> sRGB-encoded value. Inverse of [`srgb_to_linear`].
pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Per-channel sRGB->linear for an RGB triple.
pub fn srgb_to_linear_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [
        srgb_to_linear(rgb[0]),
        srgb_to_linear(rgb[1]),
        srgb_to_linear(rgb[2]),
    ]
}

/// Per-channel linear->sRGB for an RGB triple.
pub fn linear_to_srgb_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [
        linear_to_srgb(rgb[0]),
        linear_to_srgb(rgb[1]),
        linear_to_srgb(rgb[2]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn endpoints_round_trip_exactly() {
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert!(approx_eq(srgb_to_linear(1.0), 1.0, 1e-5));
        assert_eq!(linear_to_srgb(0.0), 0.0);
        assert!(approx_eq(linear_to_srgb(1.0), 1.0, 1e-5));
    }

    #[test]
    fn srgb_to_linear_reference_mid_grey() {
        // Standard reference: sRGB 0.5 -> linear ~0.214.
        assert!(approx_eq(srgb_to_linear(0.5), 0.2140, 1e-3));
    }

    #[test]
    fn linear_to_srgb_reference_mid_grey() {
        // Inverse: linear ~0.214 -> sRGB 0.5.
        assert!(approx_eq(linear_to_srgb(0.2140), 0.5, 1e-3));
    }

    #[test]
    fn round_trip_srgb_to_linear_to_srgb() {
        for i in 0..=255 {
            let v = i as f32 / 255.0;
            let back = linear_to_srgb(srgb_to_linear(v));
            assert!(approx_eq(back, v, 1e-4), "i={i} v={v} back={back}");
        }
    }

    #[test]
    fn round_trip_linear_to_srgb_to_linear() {
        for i in 0..=255 {
            let v = i as f32 / 255.0;
            let back = srgb_to_linear(linear_to_srgb(v));
            assert!(approx_eq(back, v, 1e-4), "i={i} v={v} back={back}");
        }
    }

    #[test]
    fn onyx_fog_color_byte_matches_author_intent() {
        // Onyx Cauldron's mapinfo: fog_color = [0.11, 0.13, 0.15].
        // Author intent (per BAR's accidental double-gamma pipeline):
        // these appear on-screen as bytes [28, 33, 38]. After we
        // sRGB-decode at the uniform boundary and let the
        // sRGB-encoding framebuffer re-encode, we should land at the
        // same bytes.
        let perceptual = [0.11_f32, 0.13_f32, 0.15_f32];
        let expected_bytes = [28_u8, 33_u8, 38_u8];
        let linear = srgb_to_linear_rgb(perceptual);
        let bytes_after_encode = [
            (linear_to_srgb(linear[0]) * 255.0).round() as u8,
            (linear_to_srgb(linear[1]) * 255.0).round() as u8,
            (linear_to_srgb(linear[2]) * 255.0).round() as u8,
        ];
        assert_eq!(bytes_after_encode, expected_bytes);
    }

    #[test]
    fn srgb_to_linear_dark_values_use_linear_branch() {
        // Below threshold 0.04045: srgb_to_linear is just `c / 12.92`.
        let v = 0.02_f32;
        assert!(approx_eq(srgb_to_linear(v), v / 12.92, 1e-6));
    }

    #[test]
    fn srgb_to_linear_continuous_at_threshold() {
        // Piecewise function should be continuous at 0.04045.
        let below = srgb_to_linear(0.04044);
        let above = srgb_to_linear(0.04046);
        assert!(approx_eq(below, above, 1e-4));
    }

    #[test]
    fn linear_to_srgb_continuous_at_threshold() {
        let below = linear_to_srgb(0.0031307);
        let above = linear_to_srgb(0.0031309);
        assert!(approx_eq(below, above, 1e-4));
    }

    #[test]
    fn rgb_helper_applies_per_channel() {
        let rgb = [0.11, 0.5, 0.0];
        let linear = srgb_to_linear_rgb(rgb);
        assert_eq!(linear[0], srgb_to_linear(0.11));
        assert_eq!(linear[1], srgb_to_linear(0.5));
        assert_eq!(linear[2], srgb_to_linear(0.0));
    }

    #[test]
    fn rgb_round_trip_per_channel() {
        let rgb = [0.11_f32, 0.42_f32, 0.87_f32];
        let back = linear_to_srgb_rgb(srgb_to_linear_rgb(rgb));
        for c in 0..3 {
            assert!(approx_eq(rgb[c], back[c], 1e-4));
        }
    }
}

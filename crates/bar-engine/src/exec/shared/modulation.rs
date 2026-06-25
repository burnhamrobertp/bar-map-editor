//! WM-compatible Control/Density/Mask modulation, plus the 1-v invert.
//!
//! All three port types (Control, Density, Mask) arrive as PortValue::Heightmap
//! or PortValue::Mask -- the graph routes by port name, not kind. These helpers
//! are the single implementation point for WM-compatible modulation semantics.
//!
//! Every helper assumes its inputs are at the eval-graph resolution. The graph
//! pipeline normalizes generators / FileInput to (width, height), so callers
//! should never feed mismatched buffers; the debug_assert_eq! guards catch any
//! regression that would otherwise panic inside Heightmap::frbar_data.

use bar_data::Heightmap;

/// Multiply every pixel by an optional scale field, in place.
/// Returns `effect` unchanged when `field` is None.
pub(crate) fn scale_by_field(mut effect: Heightmap, field: Option<&Heightmap>) -> Heightmap {
    let Some(f) = field else {
        return effect;
    };
    debug_assert_eq!(effect.width(), f.width(), "scale_by_field: width mismatch");
    debug_assert_eq!(
        effect.height(),
        f.height(),
        "scale_by_field: height mismatch"
    );
    for (e, &s) in effect.data_mut().iter_mut().zip(f.data()) {
        *e *= s.clamp(0.0, 1.0);
    }
    effect
}

/// Apply optional control and mask to a filter node (one with a passthrough `input`).
///
/// WM semantics:
///   - Control modulates effect strength: `lerp(input, effect, control)`
///   - Mask gates where the effect applies:  `lerp(input, effect, mask)`
///   - Both together multiply the weights in a single pass
///
/// Mutates `effect` in place; returns it untouched when both ports are unconnected.
pub(crate) fn apply_modulation(
    input: &Heightmap,
    mut effect: Heightmap,
    control: Option<&Heightmap>,
    mask: Option<&Heightmap>,
) -> Heightmap {
    if control.is_none() && mask.is_none() {
        return effect;
    }
    debug_assert_eq!(
        input.width(),
        effect.width(),
        "apply_modulation: input/effect width"
    );
    debug_assert_eq!(
        input.height(),
        effect.height(),
        "apply_modulation: input/effect height"
    );
    if let Some(c) = control {
        debug_assert_eq!(input.width(), c.width(), "apply_modulation: control width");
        debug_assert_eq!(
            input.height(),
            c.height(),
            "apply_modulation: control height"
        );
    }
    if let Some(m) = mask {
        debug_assert_eq!(input.width(), m.width(), "apply_modulation: mask width");
        debug_assert_eq!(input.height(), m.height(), "apply_modulation: mask height");
    }
    let in_d = input.data();
    let ef_d = effect.data_mut();
    match (control, mask) {
        (Some(c), Some(m)) => {
            let cd = c.data();
            let md = m.data();
            for i in 0..in_d.len() {
                let t = (cd[i].clamp(0.0, 1.0) * md[i].clamp(0.0, 1.0)).clamp(0.0, 1.0);
                ef_d[i] = in_d[i] + (ef_d[i] - in_d[i]) * t;
            }
        }
        (Some(w), None) | (None, Some(w)) => {
            let wd = w.data();
            for i in 0..in_d.len() {
                let t = wd[i].clamp(0.0, 1.0);
                ef_d[i] = in_d[i] + (ef_d[i] - in_d[i]) * t;
            }
        }
        (None, None) => unreachable!(),
    }
    effect
}

pub(crate) fn apply_invert(input: &Heightmap) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let data: Vec<f32> = input.data().iter().map(|&v| 1.0 - v).collect();
    Heightmap::frbar_data(w, h, data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn const_hm(w: u32, h: u32, v: f32) -> Heightmap {
        Heightmap::frbar_data(w, h, vec![v; (w as usize) * (h as usize)]).unwrap()
    }

    #[test]
    fn scale_by_field_none_is_identity() {
        let effect = const_hm(4, 4, 0.7);
        let out = scale_by_field(effect.clone(), None);
        assert_eq!(out.data(), effect.data());
    }

    #[test]
    fn scale_by_field_clamps_and_multiplies_per_pixel() {
        // Field values outside [0, 1] are clamped before multiply, so the
        // result never exceeds the effect.
        let effect = const_hm(2, 2, 0.6);
        let mut field = const_hm(2, 2, 0.0);
        field.data_mut().copy_from_slice(&[0.0, 0.5, 1.0, 2.0]);
        let out = scale_by_field(effect, Some(&field));
        assert!((out.data()[0] - 0.0).abs() < 1e-6);
        assert!((out.data()[1] - 0.30).abs() < 1e-6);
        assert!((out.data()[2] - 0.60).abs() < 1e-6);
        assert!((out.data()[3] - 0.60).abs() < 1e-6); // 2.0 -> clamp(1.0)
    }

    #[test]
    fn apply_modulation_no_inputs_returns_effect_untouched() {
        let input = const_hm(2, 2, 0.0);
        let effect = const_hm(2, 2, 0.9);
        let out = apply_modulation(&input, effect.clone(), None, None);
        assert_eq!(out.data(), effect.data());
    }

    #[test]
    fn apply_modulation_mask_zero_falls_back_to_input() {
        // Where mask is 0, the output must equal `input`. Where mask is 1,
        // the output must equal `effect`. Halfway lerps to the midpoint.
        let input = const_hm(2, 2, 0.0);
        let effect = const_hm(2, 2, 1.0);
        let mut mask = const_hm(2, 2, 0.0);
        mask.data_mut().copy_from_slice(&[0.0, 0.5, 1.0, 1.0]);
        let out = apply_modulation(&input, effect, None, Some(&mask));
        assert!((out.data()[0] - 0.0).abs() < 1e-6);
        assert!((out.data()[1] - 0.5).abs() < 1e-6);
        assert!((out.data()[2] - 1.0).abs() < 1e-6);
        assert!((out.data()[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn apply_modulation_control_and_mask_multiply() {
        // Both fields collapse to a single weight = clamp(c)*clamp(m).
        let input = const_hm(1, 1, 0.0);
        let effect = const_hm(1, 1, 1.0);
        let ctrl = const_hm(1, 1, 0.5);
        let mask = const_hm(1, 1, 0.4);
        let out = apply_modulation(&input, effect, Some(&ctrl), Some(&mask));
        // 0 + (1 - 0) * (0.5 * 0.4) = 0.2
        assert!((out.data()[0] - 0.2).abs() < 1e-6);
    }
}

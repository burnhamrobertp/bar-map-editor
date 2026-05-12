//! Pure brush dab application math. No `BarEditorApp` dependency, no
//! egui, no I/O -- just heightmap / colorbuffer / value stamping at a
//! pixel coordinate. Unit-tested below.
//!
//! Used by `BarEditorApp::apply_*_brush_at_heightmap` orchestration in
//! `app.rs`. The orchestration handles undo, dirtying, and routing
//! between the four brush targets (heightmap, color, metal, type);
//! these functions just write the pixels.

use bar_data::{ColorBuffer, Heightmap};

use crate::app::{BrushState, BrushTool};

/// Apply one heightmap brush dab centred at `(cx, cy)` (pixel
/// coordinates in the heightmap). Tool, radius, strength, falloff,
/// and flatten target come from `brush`. Heightmap values stay
/// clamped to `[0, 1]`.
pub(crate) fn apply_brush_dab(hm: &mut Heightmap, cx: f32, cy: f32, brush: &BrushState) {
    let w = hm.width() as i32;
    let h = hm.height() as i32;
    let radius = brush.radius_px.max(1.0);
    let r_i = radius.ceil() as i32;
    let cx_i = cx.round() as i32;
    let cy_i = cy.round() as i32;
    let x0 = (cx_i - r_i).max(0);
    let y0 = (cy_i - r_i).max(0);
    let x1 = (cx_i + r_i).min(w - 1);
    let y1 = (cy_i + r_i).min(h - 1);
    if x1 < x0 || y1 < y0 {
        return;
    }

    // For Smooth we need to read pixels we may overwrite; snapshot the
    // affected region first. The snapshot is only consulted by the
    // Smooth branch below -- other tools touch the live heightmap
    // directly.
    let snapshot: Option<Vec<f32>> = if brush.tool == BrushTool::Smooth {
        let mut v = Vec::with_capacity(((x1 - x0 + 1) * (y1 - y0 + 1)) as usize);
        for sy in y0..=y1 {
            for sx in x0..=x1 {
                v.push(hm.get(sx as u32, sy as u32).unwrap_or(0.0));
            }
        }
        Some(v)
    } else {
        None
    };
    let snap_w = (x1 - x0 + 1) as usize;

    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            if d > radius {
                continue;
            }
            // Falloff: 1.0 at center -> 0.0 at the radius. Falloff
            // exponent shapes the curve (1.0 = linear, 2.0 = squared).
            let t = (1.0 - d / radius).clamp(0.0, 1.0);
            let weight = t.powf(brush.falloff);

            let cur = hm.get(x as u32, y as u32).unwrap_or(0.0);
            let new_val = match brush.tool {
                BrushTool::Raise => cur + brush.strength * weight,
                BrushTool::Lower => cur - brush.strength * weight,
                BrushTool::Smooth => {
                    // Average the 3x3 neighbourhood from the snapshot,
                    // then lerp toward it. Mix is clamped so a hot
                    // strength setting can't overshoot the average and
                    // oscillate.
                    let snap = snapshot.as_ref().expect("Smooth mode pre-snapshots");
                    let mut sum = 0.0_f32;
                    let mut n = 0_f32;
                    for oy in -1..=1 {
                        for ox in -1..=1 {
                            let nx = x + ox;
                            let ny = y + oy;
                            if nx >= x0 && nx <= x1 && ny >= y0 && ny <= y1 {
                                let lx = (nx - x0) as usize;
                                let ly = (ny - y0) as usize;
                                sum += snap[ly * snap_w + lx];
                                n += 1.0;
                            }
                        }
                    }
                    let avg = if n > 0.0 { sum / n } else { cur };
                    let mix = (brush.strength * weight * 8.0).clamp(0.0, 1.0);
                    cur + (avg - cur) * mix
                }
                BrushTool::Flatten => {
                    let target = brush.flatten_target.unwrap_or(cur);
                    let mix = (brush.strength * weight * 4.0).clamp(0.0, 1.0);
                    cur + (target - cur) * mix
                }
            };
            let _ = hm.set(x as u32, y as u32, new_val.clamp(0.0, 1.0));
        }
    }
}

/// Stamp a circular brush of `color` into a live `ColorBuffer` cache
/// at normalised UV `(u, v)` with normalised radius `ru` (relative
/// to the buffer's longer side). Mirrors the executor's
/// `apply_color_dabs` math so the live preview matches the eventual
/// graph re-eval result.
pub(crate) fn stamp_color_dab_in_buffer(
    cb: &mut ColorBuffer,
    u: f32,
    v: f32,
    ru: f32,
    rgb: [u8; 3],
) {
    let w = cb.width() as f32;
    let h = cb.height() as f32;
    let map_dim = w.max(h);
    let cx = (u * w).round() as i32;
    let cy = (v * h).round() as i32;
    let radius_px = (ru * map_dim).max(1.0);
    let r_i = radius_px.ceil() as i32;
    let r2 = radius_px * radius_px;
    let x0 = (cx - r_i).max(0);
    let y0 = (cy - r_i).max(0);
    let x1 = (cx + r_i).min(cb.width() as i32 - 1);
    let y1 = (cy + r_i).min(cb.height() as i32 - 1);
    let rgba = [
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
        1.0,
    ];
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = (x - cx) as f32;
            let dy = (y - cy) as f32;
            if dx * dx + dy * dy > r2 {
                continue;
            }
            cb.set(x as u32, y as u32, rgba);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_hm(w: u32, h: u32, val: f32) -> Heightmap {
        let mut hm = Heightmap::new(w, h).unwrap();
        for y in 0..h {
            for x in 0..w {
                hm.set(x, y, val).unwrap();
            }
        }
        hm
    }

    fn brush(tool: BrushTool) -> BrushState {
        BrushState {
            tool,
            radius_px: 4.0,
            strength: 0.1,
            falloff: 1.0,
            flatten_target: None,
            color_rgb: [0x8B, 0x73, 0x55],
            paint_value: 1.0,
        }
    }

    #[test]
    fn raise_brush_increases_center_pixel() {
        let mut hm = flat_hm(16, 16, 0.5);
        let b = brush(BrushTool::Raise);
        apply_brush_dab(&mut hm, 8.0, 8.0, &b);
        let center = hm.get(8, 8).unwrap();
        assert!(center > 0.5, "expected center > 0.5, got {center}");
        // Outside the radius, untouched.
        let far = hm.get(0, 0).unwrap();
        assert!(
            (far - 0.5).abs() < 1e-6,
            "far pixel should be unchanged: {far}"
        );
    }

    #[test]
    fn lower_brush_decreases_center_pixel() {
        let mut hm = flat_hm(16, 16, 0.5);
        apply_brush_dab(&mut hm, 8.0, 8.0, &brush(BrushTool::Lower));
        assert!(hm.get(8, 8).unwrap() < 0.5);
    }

    #[test]
    fn flatten_brush_pulls_toward_target() {
        let mut hm = flat_hm(16, 16, 0.2);
        // Spike of height 0.9 at the centre.
        hm.set(8, 8, 0.9).unwrap();
        let mut b = brush(BrushTool::Flatten);
        b.flatten_target = Some(0.2);
        b.strength = 0.5;
        // Apply many dabs until convergence.
        for _ in 0..40 {
            apply_brush_dab(&mut hm, 8.0, 8.0, &b);
        }
        let v = hm.get(8, 8).unwrap();
        assert!(
            (v - 0.2).abs() < 0.05,
            "flatten should pull centre to ~0.2, got {v}"
        );
    }

    #[test]
    fn smooth_brush_reduces_local_variance() {
        let mut hm = flat_hm(16, 16, 0.5);
        // Single spike.
        hm.set(8, 8, 1.0).unwrap();
        let b = BrushState {
            tool: BrushTool::Smooth,
            radius_px: 3.0,
            strength: 0.5,
            falloff: 1.0,
            flatten_target: None,
            color_rgb: [0x8B, 0x73, 0x55],
            paint_value: 1.0,
        };
        // Several passes.
        for _ in 0..10 {
            apply_brush_dab(&mut hm, 8.0, 8.0, &b);
        }
        let center = hm.get(8, 8).unwrap();
        assert!(
            center < 1.0 && center > 0.5,
            "smooth should pull spike toward neighbourhood mean, got {center}"
        );
    }

    #[test]
    fn raise_clamps_to_one() {
        let mut hm = flat_hm(8, 8, 1.0);
        let b = BrushState {
            tool: BrushTool::Raise,
            radius_px: 2.0,
            strength: 0.5,
            falloff: 1.0,
            flatten_target: None,
            color_rgb: [0x8B, 0x73, 0x55],
            paint_value: 1.0,
        };
        apply_brush_dab(&mut hm, 4.0, 4.0, &b);
        assert!(hm.get(4, 4).unwrap() <= 1.0);
    }

    #[test]
    fn brush_dab_outside_bounds_is_a_noop() {
        let mut hm = flat_hm(8, 8, 0.5);
        // Centre well outside the heightmap.
        apply_brush_dab(&mut hm, 100.0, 100.0, &brush(BrushTool::Raise));
        for y in 0..8 {
            for x in 0..8 {
                assert!(
                    (hm.get(x, y).unwrap() - 0.5).abs() < 1e-6,
                    "no pixel should change: ({x},{y})"
                );
            }
        }
    }
}

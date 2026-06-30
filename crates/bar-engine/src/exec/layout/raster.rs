//! Rasterisers and spline helpers for the Layout node.

use std::collections::HashMap;
use std::f32::consts::PI;

use bar_graph::ParamValue;

use crate::exec::shared::get_float;

/// Expand a single shape placement `(cx, cy, angle_deg)` into all the
/// instances implied by the Layout node's `symmetry` mode. Coords
/// are in normalised [0..1, 0..1] space; the reflection axes pass
/// through (0.5, 0.5) and rotations pivot about the same centre.
///
/// Angle handling: a mirror flips the apparent rotation direction, so
/// the mirrored copy's angle is negated. Rotations add the rotation
/// step to the angle so the shape's silhouette rotates with its
/// position.
pub(crate) fn expand_symmetric_placements(
    cx: f32,
    cy: f32,
    angle_deg: f32,
    mode: &str,
) -> Vec<(f32, f32, f32)> {
    match mode {
        "mirror_x" => vec![(cx, cy, angle_deg), (1.0 - cx, cy, -angle_deg)],
        "mirror_y" => vec![(cx, cy, angle_deg), (cx, 1.0 - cy, -angle_deg)],
        "mirror_xy" => vec![
            (cx, cy, angle_deg),
            (1.0 - cx, cy, -angle_deg),
            (cx, 1.0 - cy, -angle_deg),
            (1.0 - cx, 1.0 - cy, angle_deg),
        ],
        "rotate_180" => vec![(cx, cy, angle_deg), (1.0 - cx, 1.0 - cy, angle_deg + 180.0)],
        "rotate_90" => {
            // Rotate (cx, cy) about (0.5, 0.5) by 0 / 90 / 180 / 270.
            // (px, py) = (0.5 + (cx - 0.5) * cos - (cy - 0.5) * sin,
            //            0.5 + (cx - 0.5) * sin + (cy - 0.5) * cos)
            let dx = cx - 0.5;
            let dy = cy - 0.5;
            vec![
                (cx, cy, angle_deg),
                (0.5 - dy, 0.5 + dx, angle_deg + 90.0),
                (1.0 - cx, 1.0 - cy, angle_deg + 180.0),
                (0.5 + dy, 0.5 - dx, angle_deg + 270.0),
            ]
        }
        _ => vec![(cx, cy, angle_deg)],
    }
}

/// Composite one primitive item (ellipse / rectangle / ridge) into the
/// coverage `field` by per-pixel max, expanding it across the
/// `symmetry` orbit first.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_primitive_item(
    field: &mut [f32],
    shape_type: &str,
    params: &HashMap<String, ParamValue>,
    i: usize,
    height_i: f32,
    falloff: f32,
    symmetry: &str,
    width: u32,
    height: u32,
) {
    let base_cx = get_float(params, &format!("x_{i}"), 0.5);
    let base_cy = get_float(params, &format!("y_{i}"), 0.5);
    let rx = get_float(params, &format!("rx_{i}"), 0.2).max(1e-4);
    let ry = get_float(params, &format!("ry_{i}"), 0.2).max(1e-4);
    let base_angle = get_float(params, &format!("angle_{i}"), 0.0);
    let falloff = falloff.clamp(0.001, 1.0);

    for (cx, cy, angle_deg) in expand_symmetric_placements(base_cx, base_cy, base_angle, symmetry) {
        let angle_rad = angle_deg * PI / 180.0;
        let cos_a = angle_rad.cos();
        let sin_a = angle_rad.sin();
        // Restrict to the shape's footprint: coverage (d < 1) lies within
        // `rx`/`ry` of the centre in any rotation, plus the line cap. `rx + ry`
        // safely over-bounds all shape types; far pixels are zero. Output-
        // identical, but skips scanning the whole grid for a small shape.
        let sw = (width - 1).max(1) as f32;
        let sh = (height - 1).max(1) as f32;
        let rad = rx + ry + 1.0 / sw.max(sh);
        let cxp = cx * sw;
        let cyp = cy * sh;
        let x0 = (cxp - rad * sw).floor().max(0.0) as u32;
        let x1 = ((cxp + rad * sw).ceil().min((width - 1) as f32)).max(0.0) as u32;
        let y0 = (cyp - rad * sh).floor().max(0.0) as u32;
        let y1 = ((cyp + rad * sh).ceil().min((height - 1) as f32)).max(0.0) as u32;
        for py in y0..=y1 {
            for px in x0..=x1 {
                let ux = px as f32 / (width - 1).max(1) as f32 - cx;
                let uy = py as f32 / (height - 1).max(1) as f32 - cy;
                let lx = (ux * cos_a + uy * sin_a) / rx;
                let ly = (-ux * sin_a + uy * cos_a) / ry;
                let d = match shape_type {
                    "rectangle" => lx.abs().max(ly.abs()),
                    "line" => {
                        // Distance from the pixel to the line SEGMENT
                        // running between local (-1, 0) and (1, 0).
                        // Inside the segment's projection: perpendicular
                        // distance (the line's body). Outside it:
                        // distance to the nearer endpoint, giving a
                        // rounded cap of radius `ry` in world space.
                        if (-1.0..=1.0).contains(&lx) {
                            ly.abs()
                        } else {
                            let sign = if lx > 0.0 { 1.0 } else { -1.0 };
                            let cap_x = (lx - sign) * rx / ry;
                            (cap_x * cap_x + ly * ly).sqrt()
                        }
                    }
                    _ => (lx * lx + ly * ly).sqrt(),
                };
                if d >= 1.0 {
                    continue;
                }
                let t = 1.0 - d;
                let smoothed = if t >= falloff {
                    1.0
                } else {
                    let s = t / falloff;
                    s * s * (3.0 - 2.0 * s)
                };
                let v = smoothed * height_i;
                let idx = py as usize * width as usize + px as usize;
                if v > field[idx] {
                    field[idx] = v;
                }
            }
        }
    }
}

/// Even-odd point-in-polygon test against a sampled (closed) curve.
/// `samples` and the query are both in normalised [0, 1] space.
fn point_in_polygon(samples: &[[f32; 2]], px: u32, py: u32, width: u32, height: u32) -> bool {
    let x = px as f32 / (width - 1).max(1) as f32;
    let y = py as f32 / (height - 1).max(1) as f32;
    let n = samples.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for k in 0..n {
        let (xi, yi) = (samples[k][0], samples[k][1]);
        let (xj, yj) = (samples[j][0], samples[j][1]);
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = k;
    }
    inside
}

/// Composite one spline item into the coverage `field`. Open splines
/// raise a band along the curve; closed splines with `fill` set raise
/// their whole interior. Symmetry duplicates the control points across
/// the orbit, rasterising one virtual spline per orbit position.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_spline_item(
    field: &mut [f32],
    params: &HashMap<String, ParamValue>,
    i: usize,
    height_i: f32,
    falloff: f32,
    symmetry: &str,
    width: u32,
    height: u32,
) {
    let points = get_spline(params, &format!("points_{i}"));
    if points.len() < 2 {
        return;
    }
    let width_norm = get_float(params, &format!("width_{i}"), 0.05).clamp(0.001, 0.5);
    let closed = matches!(
        params.get(&format!("closed_{i}")),
        Some(ParamValue::Bool(true))
    );
    let fill = matches!(
        params.get(&format!("fill_{i}")),
        Some(ParamValue::Bool(true))
    );

    let orbits: Vec<Vec<[f32; 2]>> = if symmetry == "none" {
        vec![points.to_vec()]
    } else {
        let expansions: Vec<Vec<(f32, f32, f32)>> = points
            .iter()
            .map(|p| expand_symmetric_placements(p[0], p[1], 0.0, symmetry))
            .collect();
        let orbit_size = expansions.first().map(|e| e.len()).unwrap_or(1);
        (0..orbit_size)
            .map(|orbit_idx| {
                expansions
                    .iter()
                    .map(|exp| {
                        let (x, y, _) = exp[orbit_idx];
                        [x, y]
                    })
                    .collect()
            })
            .collect()
    };

    let aspect_ref = width.min(height) as f32;
    let width_px = width_norm * aspect_ref;
    let inner_px = width_px * (1.0 - falloff.clamp(0.0, 1.0));
    let sw = (width - 1).max(1) as f32;
    let sh = (height - 1).max(1) as f32;
    // Feathered band weight from distance-to-curve (in pixels).
    let band = |d: f32| -> f32 {
        if d <= inner_px {
            1.0
        } else if d >= width_px {
            0.0
        } else {
            let t = (width_px - d) / (width_px - inner_px).max(1e-6);
            t * t * (3.0 - 2.0 * t)
        }
    };

    for orbit in &orbits {
        // 16 samples/segment is smooth enough: the band uses exact
        // point-to-segment distance, so the polyline only has to approximate
        // the curve's shape, not its distance field. Halving the segment count
        // halves the scatter cost.
        let samples = sample_catmull_rom(orbit, 16, closed || fill);
        if samples.len() < 2 {
            continue;
        }

        if fill {
            // Closed interior fill needs a per-pixel interior test, so scan the
            // polygon's bounding box (expanded by the feather band) and combine
            // the distance-feathered edge with the point-in-polygon interior.
            let (mut bx0, mut by0, mut bx1, mut by1) = (
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            );
            for s in &samples {
                bx0 = bx0.min(s[0] * sw);
                bx1 = bx1.max(s[0] * sw);
                by0 = by0.min(s[1] * sh);
                by1 = by1.max(s[1] * sh);
            }
            let m = width_px + 1.0;
            let x0 = (bx0 - m).floor().max(0.0) as u32;
            let x1 = ((bx1 + m).ceil().min((width - 1) as f32)).max(0.0) as u32;
            let y0 = (by0 - m).floor().max(0.0) as u32;
            let y1 = ((by1 + m).ceil().min((height - 1) as f32)).max(0.0) as u32;
            for py in y0..=y1 {
                for px in x0..=x1 {
                    let mut min_d2 = f32::INFINITY;
                    for s in &samples {
                        let dx = px as f32 - s[0] * sw;
                        let dy = py as f32 - s[1] * sh;
                        min_d2 = min_d2.min(dx * dx + dy * dy);
                    }
                    let mut weight = band(min_d2.sqrt());
                    if point_in_polygon(&samples, px, py, width, height) {
                        weight = 1.0;
                    }
                    let v = weight * height_i;
                    let idx = py as usize * width as usize + px as usize;
                    if v > field[idx] {
                        field[idx] = v;
                    }
                }
            }
        } else {
            // Open (or closed-but-unfilled) band: stamp each polyline segment's
            // neighbourhood with the point-to-segment distance band, instead of
            // testing every pixel against every sample. Cost is O(band area),
            // not O(pixels * samples) -- the difference between a few ms and
            // ~230 ms for a long river at preview resolution.
            let n = samples.len();
            let seg_count = if closed { n } else { n - 1 };
            for k in 0..seg_count {
                let a = samples[k];
                let b = samples[(k + 1) % n];
                let (ax, ay) = (a[0] * sw, a[1] * sh);
                let (bx, by) = (b[0] * sw, b[1] * sh);
                let m = width_px + 1.0;
                let x0 = (ax.min(bx) - m).floor().max(0.0) as u32;
                let x1 = ((ax.max(bx) + m).ceil().min((width - 1) as f32)).max(0.0) as u32;
                let y0 = (ay.min(by) - m).floor().max(0.0) as u32;
                let y1 = ((ay.max(by) + m).ceil().min((height - 1) as f32)).max(0.0) as u32;
                for py in y0..=y1 {
                    for px in x0..=x1 {
                        let d = dist_point_to_segment(px as f32, py as f32, ax, ay, bx, by);
                        let weight = band(d);
                        if weight <= 0.0 {
                            continue;
                        }
                        let v = weight * height_i;
                        let idx = py as usize * width as usize + px as usize;
                        if v > field[idx] {
                            field[idx] = v;
                        }
                    }
                }
            }
        }
    }
}

/// Shortest distance from point `(px, py)` to the line segment `a`-`b`,
/// all in pixel space.
fn dist_point_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let ab2 = abx * abx + aby * aby;
    let t = if ab2 > 1e-12 {
        (((px - ax) * abx + (py - ay) * aby) / ab2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let cx = ax + t * abx;
    let cy = ay + t * aby;
    let dx = px - cx;
    let dy = py - cy;
    (dx * dx + dy * dy).sqrt()
}

/// Read the `ParamValue::Spline` at `key`, returning an empty slice
/// when the param is missing or has the wrong variant. The spline
/// rasteriser uses this so it can short-circuit empty splines cleanly.
fn get_spline<'a>(params: &'a HashMap<String, ParamValue>, key: &str) -> &'a [[f32; 2]] {
    match params.get(key) {
        Some(ParamValue::Spline(pts)) => pts,
        _ => &[],
    }
}

/// One segment of a centripetal Catmull-Rom curve. Given the four
/// surrounding control points and `t` in `[0, 1]`, returns the curve
/// position. P1 and P2 are the segment's endpoints; P0 and P3 are
/// the neighbours that bias the tangents.
fn catmull_rom_segment(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], t: f32) -> [f32; 2] {
    let t2 = t * t;
    let t3 = t2 * t;
    let cx = 0.5
        * ((2.0 * p1[0])
            + (-p0[0] + p2[0]) * t
            + (2.0 * p0[0] - 5.0 * p1[0] + 4.0 * p2[0] - p3[0]) * t2
            + (-p0[0] + 3.0 * p1[0] - 3.0 * p2[0] + p3[0]) * t3);
    let cy = 0.5
        * ((2.0 * p1[1])
            + (-p0[1] + p2[1]) * t
            + (2.0 * p0[1] - 5.0 * p1[1] + 4.0 * p2[1] - p3[1]) * t2
            + (-p0[1] + 3.0 * p1[1] - 3.0 * p2[1] + p3[1]) * t3);
    [cx, cy]
}

/// Sample a Catmull-Rom curve through `points` at `samples_per_segment`
/// evenly-spaced `t` values per segment. Endpoint tangents are
/// reflected (open spline) or wrap around (closed spline). Output is
/// in the same normalised coord space as `points`.
fn sample_catmull_rom(
    points: &[[f32; 2]],
    samples_per_segment: usize,
    closed: bool,
) -> Vec<[f32; 2]> {
    let n = points.len();
    if n < 2 {
        return points.to_vec();
    }
    let mut samples = Vec::with_capacity(n * samples_per_segment);
    let seg_count = if closed { n } else { n - 1 };
    for i in 0..seg_count {
        let i_prev = if closed {
            (i + n - 1) % n
        } else if i == 0 {
            // Open spline: reflect P1 through P0 to get a virtual P-1.
            // Encoded by passing a synthesised point computed below.
            usize::MAX
        } else {
            i - 1
        };
        let i_next = if closed {
            (i + 2) % n
        } else {
            (i + 2).min(n - 1)
        };
        let p0 = if i_prev == usize::MAX {
            // 2*P0 - P1 -- reflection through the endpoint
            [
                2.0 * points[i][0] - points[i + 1][0],
                2.0 * points[i][1] - points[i + 1][1],
            ]
        } else {
            points[i_prev]
        };
        let p1 = points[i];
        let p2 = points[if closed { (i + 1) % n } else { i + 1 }];
        // Open spline last segment: reflect through P_(n-1) to get P_n.
        let p3 = if !closed && i + 2 >= n {
            [2.0 * p2[0] - p1[0], 2.0 * p2[1] - p1[1]]
        } else {
            points[i_next]
        };
        for s in 0..samples_per_segment {
            let t = s as f32 / samples_per_segment as f32;
            samples.push(catmull_rom_segment(p0, p1, p2, p3, t));
        }
    }
    // Include the final endpoint so distance queries near the tip don't
    // miss a sample.
    if !closed {
        samples.push(points[n - 1]);
    }
    samples
}

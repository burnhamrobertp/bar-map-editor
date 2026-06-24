//! Paint-asset decoding helpers shared within the family.

use bar_data::{ColorBuffer, Heightmap};

/// Sampling mode for `painted_grayscale_to_heightmap`. Smooth
/// (bilinear) is correct for continuous data like heightmap-delta
/// paint layers; Nearest preserves quantised data like the
/// engine's metalmap / typemap, where each u8 value is an integer
/// reading (metal density, terrain-type id) and averaging
/// neighbouring values is semantically meaningless. Bilinear-blurring
/// a sparse metal map dilutes single-pixel spots into faint blobs
/// that the engine's spot-finder later mis-aggregates (or filters
/// out entirely via the `maxValue = 15` gate in `gui_metalspots`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GrayscaleSampling {
    Bilinear,
    Nearest,
}

/// Read a `PaintedHeightmap` asset, dispatching on its pixel format.
/// Brush-painted assets are still 8-bit (the brush dab range is captured
/// at u8 granularity); SD7-imported asset are f32 to preserve the full
/// SMF height precision (16-bit native; u8 storage was visibly terraced).
pub(crate) fn read_painted_heightmap_asset(
    asset_path: &str,
    fallback_w: u32,
    fallback_h: u32,
    out_w: u32,
    out_h: u32,
    sampling: GrayscaleSampling,
) -> Heightmap {
    if asset_path.is_empty() {
        return painted_grayscale_to_heightmap(
            Vec::new(),
            fallback_w,
            fallback_h,
            out_w,
            out_h,
            sampling,
        );
    }
    match bar_project::read_asset_file(std::path::Path::new(asset_path)) {
        Ok((header, data)) => {
            let src_w = header.width.max(1);
            let src_h = header.height.max(1);
            match header.kind {
                bar_project::AssetKind::GrayscaleU8 => {
                    painted_grayscale_to_heightmap(data, src_w, src_h, out_w, out_h, sampling)
                }
                bar_project::AssetKind::GrayscaleF32 => {
                    painted_f32_to_heightmap(&data, src_w, src_h, out_w, out_h)
                }
                other => {
                    tracing::warn!(
                        asset_path,
                        ?other,
                        "PaintedHeightmap asset has non-grayscale kind; falling back to zero heightmap",
                    );
                    painted_grayscale_to_heightmap(
                        Vec::new(),
                        fallback_w,
                        fallback_h,
                        out_w,
                        out_h,
                        sampling,
                    )
                }
            }
        }
        Err(e) => {
            tracing::warn!(asset_path, error = %e, "Failed to read PaintedHeightmap asset");
            painted_grayscale_to_heightmap(
                Vec::new(),
                fallback_w,
                fallback_h,
                out_w,
                out_h,
                sampling,
            )
        }
    }
}

/// Bilinearly resample a `src_w x src_h` f32 heightmap (stored as
/// little-endian f32 bytes) into a `out_w x out_h` `Heightmap`. Sample
/// values are clamped to `[0, 1]` to match the contract of the rest of
/// the heightmap pipeline.
fn painted_f32_to_heightmap(
    bytes: &[u8],
    src_w: u32,
    src_h: u32,
    out_w: u32,
    out_h: u32,
) -> Heightmap {
    let expected = (src_w as usize)
        .saturating_mul(src_h as usize)
        .saturating_mul(4);
    if bytes.len() != expected || src_w == 0 || src_h == 0 {
        // Wrong size -- produce a flat zero heightmap so downstream nodes
        // still have something to operate on.
        return Heightmap::frbar_data(
            out_w,
            out_h,
            vec![0.0f32; (out_w as usize) * (out_h as usize)],
        )
        .unwrap();
    }
    let src: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();

    let mut data = vec![0.0f32; (out_w as usize) * (out_h as usize)];
    for oy in 0..out_h {
        for ox in 0..out_w {
            let sx = ox as f32 * (src_w as f32 - 1.0) / (out_w as f32 - 1.0).max(1.0);
            let sy = oy as f32 * (src_h as f32 - 1.0) / (out_h as f32 - 1.0).max(1.0);
            let x0 = sx as u32;
            let y0 = sy as u32;
            let x1 = (x0 + 1).min(src_w - 1);
            let y1 = (y0 + 1).min(src_h - 1);
            let fx = sx - sx.floor();
            let fy = sy - sy.floor();
            let v00 = src[(y0 as usize) * (src_w as usize) + x0 as usize];
            let v10 = src[(y0 as usize) * (src_w as usize) + x1 as usize];
            let v01 = src[(y1 as usize) * (src_w as usize) + x0 as usize];
            let v11 = src[(y1 as usize) * (src_w as usize) + x1 as usize];
            let v = v00 * (1.0 - fx) * (1.0 - fy)
                + v10 * fx * (1.0 - fy)
                + v01 * (1.0 - fx) * fy
                + v11 * fx * fy;
            data[(oy as usize) * (out_w as usize) + ox as usize] = v.clamp(0.0, 1.0);
        }
    }
    Heightmap::frbar_data(out_w, out_h, data).unwrap()
}

/// Bilinearly scale a painted greyscale image at `src_w x src_h`
/// up/down to the output dims and normalise `[0,255] -> [0.0, 1.0]`.
pub(crate) fn painted_grayscale_to_heightmap(
    pixels: Vec<u8>,
    src_w: u32,
    src_h: u32,
    out_w: u32,
    out_h: u32,
    sampling: GrayscaleSampling,
) -> Heightmap {
    // Fill with zeros if no painted data
    let pixels = if pixels.len() == (src_w as usize) * (src_h as usize) {
        pixels
    } else {
        vec![0u8; (src_w as usize) * (src_h as usize)]
    };

    let mut data = vec![0.0f32; (out_w as usize) * (out_h as usize)];
    for oy in 0..out_h {
        for ox in 0..out_w {
            let sx = ox as f32 * (src_w as f32 - 1.0) / (out_w as f32 - 1.0).max(1.0);
            let sy = oy as f32 * (src_h as f32 - 1.0) / (out_h as f32 - 1.0).max(1.0);
            let v = match sampling {
                GrayscaleSampling::Bilinear => {
                    let x0 = sx as u32;
                    let y0 = sy as u32;
                    let x1 = (x0 + 1).min(src_w - 1);
                    let y1 = (y0 + 1).min(src_h - 1);
                    let fx = sx - sx.floor();
                    let fy = sy - sy.floor();
                    let v00 = pixels[(y0 as usize) * (src_w as usize) + x0 as usize] as f32 / 255.0;
                    let v10 = pixels[(y0 as usize) * (src_w as usize) + x1 as usize] as f32 / 255.0;
                    let v01 = pixels[(y1 as usize) * (src_w as usize) + x0 as usize] as f32 / 255.0;
                    let v11 = pixels[(y1 as usize) * (src_w as usize) + x1 as usize] as f32 / 255.0;
                    v00 * (1.0 - fx) * (1.0 - fy)
                        + v10 * fx * (1.0 - fy)
                        + v01 * (1.0 - fx) * fy
                        + v11 * fx * fy
                }
                GrayscaleSampling::Nearest => {
                    // Round, not floor, so cells at the half-pixel
                    // boundary land on the nearer source pixel rather
                    // than systematically biasing toward the lower
                    // index (which on a 64 -> 1536 upsample would
                    // leave the rightmost column of every 24-px block
                    // un-mapped).
                    let sx_round = (sx + 0.5) as u32;
                    let sy_round = (sy + 0.5) as u32;
                    let sx_c = sx_round.min(src_w.saturating_sub(1));
                    let sy_c = sy_round.min(src_h.saturating_sub(1));
                    pixels[(sy_c as usize) * (src_w as usize) + sx_c as usize] as f32 / 255.0
                }
            };
            data[(oy as usize) * (out_w as usize) + ox as usize] = v;
        }
    }
    Heightmap::frbar_data(out_w, out_h, data).unwrap()
}

/// Bilinearly scale a painted RGB image (3 bytes per pixel at
/// `src_res x src_res`) up/down to the output dims, returning a
/// `ColorBuffer` (RGBA with alpha = 1.0).
pub(crate) fn painted_rgb_to_color_buffer(
    pixels: Vec<u8>,
    src_w: u32,
    src_h: u32,
    out_w: u32,
    out_h: u32,
) -> ColorBuffer {
    let src_w = src_w as usize;
    let src_h = src_h as usize;
    let expected = src_w * src_h * 3;

    // Fall back to opaque mid-grey if no painted data -- same shape
    // as PaintedHeightmap's "no data -> zeros" fallback.
    let pixels = if pixels.len() == expected {
        pixels
    } else {
        vec![128u8; expected]
    };

    let mut buf = ColorBuffer::new(out_w, out_h).unwrap();
    let sample =
        |x: usize, y: usize, c: usize| -> f32 { pixels[(y * src_w + x) * 3 + c] as f32 / 255.0 };
    for oy in 0..out_h {
        for ox in 0..out_w {
            let sx = ox as f32 * (src_w as f32 - 1.0) / (out_w as f32 - 1.0).max(1.0);
            let sy = oy as f32 * (src_h as f32 - 1.0) / (out_h as f32 - 1.0).max(1.0);
            let x0 = sx as usize;
            let y0 = sy as usize;
            let x1 = (x0 + 1).min(src_w - 1);
            let y1 = (y0 + 1).min(src_h - 1);
            let fx = sx - sx.floor();
            let fy = sy - sy.floor();

            let mut rgb = [0.0_f32; 3];
            for (c, slot) in rgb.iter_mut().enumerate() {
                let v00 = sample(x0, y0, c);
                let v10 = sample(x1, y0, c);
                let v01 = sample(x0, y1, c);
                let v11 = sample(x1, y1, c);
                *slot = v00 * (1.0 - fx) * (1.0 - fy)
                    + v10 * fx * (1.0 - fy)
                    + v01 * (1.0 - fx) * fy
                    + v11 * fx * fy;
            }
            buf.set(ox, oy, [rgb[0], rgb[1], rgb[2], 1.0]);
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_neighbour_preserves_single_metal_spot_through_upsample() {
        // Metalmap regression: a single-pixel metal spot at value 200
        // in a 4x4 source must survive an upsample to 12x12 with peak
        // value intact. With the previous bilinear path the same
        // upsample dilutes the peak to ~140; the engine's spot
        // finder then sees a smeared cluster instead of the original
        // discrete spot.
        let mut pixels = vec![0u8; 16];
        pixels[5] = 200; // (1, 1) in a 4x4 grid
        let hm = painted_grayscale_to_heightmap(
            pixels.clone(),
            4,
            4,
            12,
            12,
            GrayscaleSampling::Nearest,
        );
        // Peak should be the original value normalised.
        let peak = hm.data().iter().cloned().fold(0.0f32, f32::max);
        let expected = 200.0 / 255.0;
        assert!(
            (peak - expected).abs() < 1e-4,
            "nearest peak should be {expected}, got {peak}",
        );
        // Bilinear sanity-check: same upsample with bilinear gives a
        // strictly lower peak (the spot is averaged with zero
        // neighbours), confirming the round-trip degradation the
        // engine's spot-finder was running into.
        let blurred =
            painted_grayscale_to_heightmap(pixels, 4, 4, 12, 12, GrayscaleSampling::Bilinear);
        let bilinear_peak = blurred.data().iter().cloned().fold(0.0f32, f32::max);
        assert!(
            bilinear_peak < expected * 0.95,
            "bilinear should dilute the peak below 95% of source (got {bilinear_peak} vs expected < {})",
            expected * 0.95,
        );
    }
}

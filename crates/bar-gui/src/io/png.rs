//! PNG load/save helpers for `bar_data::Heightmap` and
//! `bar_data::ColorBuffer`. Used by the project-save sculpt sidecar
//! and by the inspector "Save heightmap as PNG" path.
//!
//! Two flavours of heightmap PNG:
//! - `save_heightmap_as_png16` / `load_heightmap_from_png16`: maps
//!   f32 [0, 1] to u16 [0, 65535] linearly. Used for absolute height
//!   values (the source heightmap, sculpted result).
//! - `save_heightmap_as_png16_biased` /
//!   `load_heightmap_from_png16_biased`: maps f32 [-1, 1] to u16
//!   [0, 65535] with 0 -> 32768. Used for signed deltas (the sculpt
//!   overlay layer).

use bar_data::{ColorBuffer, Heightmap};
use eframe::egui;

/// Read a 16-bit grayscale PNG into a Heightmap. Inverse of
/// `save_heightmap_as_png16`. Used to restore sculpt overlays at
/// project load time.
pub(crate) fn load_heightmap_from_png16(path: &std::path::Path) -> Result<Heightmap, String> {
    let img = image::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let gray = img.to_luma16();
    let (w, h) = gray.dimensions();
    let data: Vec<f32> = gray.pixels().map(|p| p.0[0] as f32 / 65535.0).collect();
    Heightmap::frbar_data(w, h, data).map_err(|e| e.to_string())
}

/// Write a heightmap as a 16-bit grayscale PNG. The heightmap stores
/// f32 in [0, 1]; we map that to the full u16 range so the round-trip
/// through FileInput preserves precision. Errors come from disk
/// failure or image-encoding issues -- surface them as user-facing
/// status messages instead of unwrapping.
pub(crate) fn save_heightmap_as_png16(
    hm: &Heightmap,
    path: &std::path::Path,
) -> Result<(), String> {
    let w = hm.width();
    let h = hm.height();
    let mut buf: Vec<u16> = Vec::with_capacity((w as usize) * (h as usize));
    for v in hm.data() {
        buf.push((v.clamp(0.0, 1.0) * 65535.0) as u16);
    }
    // image::save_buffer expects the bytes in native (little-endian on
    // x86_64) byte order -- image's L16 codec handles the PNG-spec
    // big-endian conversion internally.
    let mut bytes: Vec<u8> = Vec::with_capacity(buf.len() * 2);
    for v in &buf {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    image::save_buffer(path, &bytes, w, h, image::ExtendedColorType::L16)
        .map_err(|e| format!("PNG save failed: {e}"))
}

/// Write a signed height-delta as a biased 16-bit grayscale PNG.
/// delta=0 maps to pixel value 32768; range [-1,+1] maps to [0, 65535].
pub(crate) fn save_heightmap_as_png16_biased(
    hm: &Heightmap,
    path: &std::path::Path,
) -> Result<(), String> {
    let w = hm.width();
    let h = hm.height();
    let mut bytes: Vec<u8> = Vec::with_capacity((w as usize) * (h as usize) * 2);
    for &v in hm.data() {
        let pixel = ((v.clamp(-1.0, 1.0) + 1.0) * 0.5 * 65535.0) as u16;
        bytes.extend_from_slice(&pixel.to_le_bytes());
    }
    image::save_buffer(path, &bytes, w, h, image::ExtendedColorType::L16)
        .map_err(|e| format!("PNG save failed: {e}"))
}

/// Load a biased 16-bit grayscale PNG back to a signed height-delta.
pub(crate) fn load_heightmap_from_png16_biased(
    path: &std::path::Path,
) -> Result<Heightmap, String> {
    let img = image::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let gray = img.to_luma16();
    let (w, h) = gray.dimensions();
    let data: Vec<f32> = gray
        .pixels()
        .map(|p| (p.0[0] as f32 / 65535.0) * 2.0 - 1.0)
        .collect();
    Heightmap::frbar_data(w, h, data).map_err(|e| e.to_string())
}

/// Write a `ColorBuffer` as an RGBA PNG.
pub(crate) fn save_color_buffer_as_png(
    cb: &ColorBuffer,
    path: &std::path::Path,
) -> Result<(), String> {
    let w = cb.width();
    let h = cb.height();
    let mut bytes: Vec<u8> = Vec::with_capacity((w as usize) * (h as usize) * 4);
    for chunk in cb.data().chunks_exact(4) {
        bytes.push((chunk[0].clamp(0.0, 1.0) * 255.0) as u8);
        bytes.push((chunk[1].clamp(0.0, 1.0) * 255.0) as u8);
        bytes.push((chunk[2].clamp(0.0, 1.0) * 255.0) as u8);
        bytes.push((chunk[3].clamp(0.0, 1.0) * 255.0) as u8);
    }
    image::save_buffer(path, &bytes, w, h, image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("PNG save failed: {e}"))
}

/// Load a `ColorBuffer` from an RGBA PNG.
pub(crate) fn load_color_buffer_from_png(path: &std::path::Path) -> Result<ColorBuffer, String> {
    let img = image::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let data: Vec<f32> = rgba
        .pixels()
        .flat_map(|p| {
            [
                p.0[0] as f32 / 255.0,
                p.0[1] as f32 / 255.0,
                p.0[2] as f32 / 255.0,
                p.0[3] as f32 / 255.0,
            ]
        })
        .collect();
    ColorBuffer::frbar_data(w, h, data).map_err(|e| e.to_string())
}

/// Render a heightmap into an egui `ColorImage` for the 2D inspector.
/// Underwater pixels (n < waterline_norm) are tinted blue with depth
/// darkening; above-water pixels go from dark grey (low) to a warm
/// near-white (high) so the user can read terrain shape at a glance.
pub(crate) fn heightmap_to_color_image(hm: &Heightmap, min_h: f32, max_h: f32) -> egui::ColorImage {
    let w = hm.width() as usize;
    let h = hm.height() as usize;
    let span = (max_h - min_h).max(1.0);
    let waterline_norm = if min_h < 0.0 {
        (-min_h / span).clamp(0.0, 1.0)
    } else {
        -1.0
    };
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let n = hm.get(x as u32, y as u32).unwrap_or(0.0).clamp(0.0, 1.0);
            let pixel = if waterline_norm >= 0.0 && n < waterline_norm {
                let depth = (waterline_norm - n) / waterline_norm.max(0.001);
                let dim = (1.0 - depth * 0.6).clamp(0.3, 1.0);
                egui::Color32::from_rgb((40.0 * dim) as u8, (90.0 * dim) as u8, (160.0 * dim) as u8)
            } else {
                let above = if waterline_norm >= 0.0 {
                    (n - waterline_norm) / (1.0 - waterline_norm).max(0.001)
                } else {
                    n
                };
                let v = (above * 220.0 + 35.0) as u8;
                let warm = (above * 25.0) as u8;
                egui::Color32::from_rgb(v.saturating_add(warm), v, v.saturating_sub(warm / 2))
            };
            pixels.push(pixel);
        }
    }
    egui::ColorImage {
        size: [w, h],
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_save_roundtrip_preserves_values() {
        let mut hm = Heightmap::new(4, 4).expect("new");
        for y in 0..4 {
            for x in 0..4 {
                let v = (x + y * 4) as f32 / 15.0;
                let _ = hm.set(x, y, v);
            }
        }
        let dir = tempdir_or_skip();
        let path = dir.join("test.png");
        save_heightmap_as_png16(&hm, &path).expect("save");
        let loaded = load_heightmap_from_png16(&path).expect("load");
        assert_eq!(loaded.width(), 4);
        assert_eq!(loaded.height(), 4);
        for y in 0..4 {
            for x in 0..4 {
                let want = hm.get(x, y).unwrap();
                let got = loaded.get(x, y).unwrap();
                // u16 quantisation gives ~1.5e-5 max error, so round-trip
                // values within that threshold.
                assert!((want - got).abs() < 1.0e-4, "{want} vs {got}");
            }
        }
    }

    #[test]
    fn png_load_roundtrip_matches_save() {
        let mut hm = Heightmap::new(2, 2).expect("new");
        let _ = hm.set(0, 0, 0.0);
        let _ = hm.set(1, 0, 0.5);
        let _ = hm.set(0, 1, 0.75);
        let _ = hm.set(1, 1, 1.0);
        let dir = tempdir_or_skip();
        let path = dir.join("rt.png");
        save_heightmap_as_png16(&hm, &path).expect("save should succeed");
        let loaded = load_heightmap_from_png16(&path).expect("load");
        assert_eq!(loaded.get(0, 0).unwrap(), 0.0);
        assert!((loaded.get(1, 0).unwrap() - 0.5).abs() < 1.0e-4);
        assert!((loaded.get(0, 1).unwrap() - 0.75).abs() < 1.0e-4);
        assert_eq!(loaded.get(1, 1).unwrap(), 1.0);
    }

    fn tempdir_or_skip() -> std::path::PathBuf {
        // Prefer the OS tempdir; fall back to target/ if unavailable.
        let base = std::env::temp_dir();
        let dir = base.join(format!("bar-gui-png-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}

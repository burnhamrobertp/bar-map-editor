//! Image-decode + asset-write helpers shared between the wizard and
//! the Finish handler.
//!
//! Each helper decodes one picked file (via the `image` crate), pulls
//! it into the asset-storage format the matching graph node expects,
//! and writes the asset file under the supplied temp dir. The Finish
//! handler in `project::lifecycle` then mints the actual graph node
//! and wires it to the FinalComposition.

use bar_project::{write_asset_file, AssetHeader, AssetId, AssetKind};

/// Outcome of staging one input file for a graph node: the asset id,
/// the on-disk asset path (caller injects this into the node's
/// `asset_path` param), and the rectangular dimensions of the asset.
/// Non-square images keep their full source aspect ratio -- no min(w,h)
/// crop -- so 32x64-square (W=32, H=64) map authoring works end to end.
pub struct StagedAsset {
    pub asset_id: AssetId,
    pub asset_path: std::path::PathBuf,
    pub width: u32,
    pub height: u32,
}

const HM_CAP: u32 = 8192;
const OTHER_CAP: u32 = 512;
const TEX_CAP: u32 = 2048;

pub fn stage_heightmap(
    src: &std::path::Path,
    assets_dir: &std::path::Path,
) -> std::io::Result<StagedAsset> {
    let img = image::open(src).map_err(io_other)?;
    let (w, h) = (img.width(), img.height());
    let (tw, th) = scale_to_cap(w, h, HM_CAP);
    // Promote to 16-bit luma when the source supports it so the f32
    // conversion preserves full SMF precision; 8-bit sources still go
    // through but at u8 granularity.
    let bytes = if let Some(buf) = img.as_luma16() {
        resample_luma16_to_f32_bytes(buf, tw, th)
    } else {
        resample_luma8_to_f32_bytes(&img.to_luma8(), tw, th)
    };
    write_asset(assets_dir, AssetKind::GrayscaleF32, tw, th, &bytes)
}

pub fn stage_grayscale_u8(
    src: &std::path::Path,
    assets_dir: &std::path::Path,
) -> std::io::Result<StagedAsset> {
    let img = image::open(src).map_err(io_other)?;
    let (w, h) = (img.width(), img.height());
    let (tw, th) = scale_to_cap(w, h, OTHER_CAP);
    let g = img.to_luma8();
    let bytes = resample_luma_to_u8_bytes(&g, tw, th);
    write_asset(assets_dir, AssetKind::GrayscaleU8, tw, th, &bytes)
}

pub fn stage_texture(
    src: &std::path::Path,
    assets_dir: &std::path::Path,
) -> std::io::Result<StagedAsset> {
    let img = image::open(src).map_err(io_other)?;
    let (w, h) = (img.width(), img.height());
    let (tw, th) = scale_to_cap(w, h, TEX_CAP);
    let rgb = img.to_rgb8();
    let bytes = if (w, h) == (tw, th) {
        rgb.into_raw()
    } else {
        image::imageops::resize(&rgb, tw, th, image::imageops::FilterType::Triangle).into_raw()
    };
    write_asset(assets_dir, AssetKind::RgbU8, tw, th, &bytes)
}

/// Scale `(w, h)` so the larger axis sits at `cap`, preserving aspect
/// ratio; if the source is already within `cap` returns it unchanged.
/// Mirrors the helper in `bar-engine::extract` so .sd7 import and the
/// wizard land at matching target sizes.
fn scale_to_cap(w: u32, h: u32, cap: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (1, 1);
    }
    let larger = w.max(h);
    if larger <= cap {
        return (w, h);
    }
    let scale = cap as f64 / larger as f64;
    let tw = ((w as f64 * scale).round() as u32).max(1);
    let th = ((h as f64 * scale).round() as u32).max(1);
    (tw, th)
}

fn write_asset(
    assets_dir: &std::path::Path,
    kind: AssetKind,
    width: u32,
    height: u32,
    bytes: &[u8],
) -> std::io::Result<StagedAsset> {
    std::fs::create_dir_all(assets_dir)?;
    let asset_id = AssetId::new();
    let asset_path = assets_dir.join(format!("{}.bin", asset_id.0));
    write_asset_file(
        &asset_path,
        AssetHeader {
            kind,
            width,
            height,
        },
        bytes,
    )
    .map_err(io_other)?;
    Ok(StagedAsset {
        asset_id,
        asset_path,
        width,
        height,
    })
}

fn resample_luma8_to_f32_bytes(
    src: &image::ImageBuffer<image::Luma<u8>, Vec<u8>>,
    tw: u32,
    th: u32,
) -> Vec<u8> {
    let (sw, sh) = src.dimensions();
    let mut out = Vec::with_capacity((tw as usize) * (th as usize) * 4);
    for oy in 0..th {
        let sy = (oy as u64 * sh as u64 / th as u64) as u32;
        for ox in 0..tw {
            let sx = (ox as u64 * sw as u64 / tw as u64) as u32;
            let v = src.get_pixel(sx.min(sw - 1), sy.min(sh - 1))[0] as f32 / 255.0;
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

fn resample_luma16_to_f32_bytes(
    src: &image::ImageBuffer<image::Luma<u16>, Vec<u16>>,
    tw: u32,
    th: u32,
) -> Vec<u8> {
    let (sw, sh) = src.dimensions();
    let mut out = Vec::with_capacity((tw as usize) * (th as usize) * 4);
    for oy in 0..th {
        let sy = (oy as u64 * sh as u64 / th as u64) as u32;
        for ox in 0..tw {
            let sx = (ox as u64 * sw as u64 / tw as u64) as u32;
            let v = src.get_pixel(sx.min(sw - 1), sy.min(sh - 1))[0] as f32 / 65535.0;
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

fn resample_luma_to_u8_bytes(
    src: &image::ImageBuffer<image::Luma<u8>, Vec<u8>>,
    tw: u32,
    th: u32,
) -> Vec<u8> {
    let (sw, sh) = src.dimensions();
    if (sw, sh) == (tw, th) {
        return src.as_raw().clone();
    }
    let mut out = Vec::with_capacity((tw as usize) * (th as usize));
    for oy in 0..th {
        let sy = (oy as u64 * sh as u64 / th as u64) as u32;
        for ox in 0..tw {
            let sx = (ox as u64 * sw as u64 / tw as u64) as u32;
            out.push(src.get_pixel(sx.min(sw - 1), sy.min(sh - 1))[0]);
        }
    }
    out
}

fn io_other(e: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

//! Extract .sd7 archives into a work directory and scan their contents.
//!
//! Work directories live under an app-controlled cache directory rather than
//! next to the source archive, so opening a read-only or shared `.sd7` does
//! not litter the user's filesystem. Each archive maps to a stable subdirectory
//! keyed by a hash of its absolute path, so re-opening the same archive
//! preserves any in-place edits the user has made between sessions.

use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use bar_data::smt::TILE_SIZE;
use directories::ProjectDirs;

pub use bar_project::WorkDirScan;

/// Root of all extracted SD7 work directories. Falls back to the OS temp dir
/// if a per-user cache directory cannot be resolved.
pub fn work_dir_root() -> PathBuf {
    if let Some(dirs) = ProjectDirs::from("", "BarEditor", "BarEditor") {
        dirs.cache_dir().join("work")
    } else {
        std::env::temp_dir().join("BarEditor").join("work")
    }
}

/// Compute the per-archive work directory for a given source `.sd7`.
fn work_dir_for(archive: &Path) -> PathBuf {
    let stem = archive
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("map");
    let canonical = std::fs::canonicalize(archive).unwrap_or_else(|_| archive.to_path_buf());
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    let hash = hasher.finish();
    work_dir_root().join(format!("{}_{:016x}", stem, hash))
}

/// Extract an `.sd7` archive to a managed work directory and scan its contents.
///
/// The work directory is placed under the user's app cache directory at
/// `<cache>/BarEditor/work/<stem>_<hash>/`, where `<hash>` is derived from
/// the archive's canonical path. This keeps re-opens stable while ensuring we
/// never write into the source archive's directory.
///
/// If the work directory already exists and is non-empty, extraction is skipped
/// so that any edits the user has made are preserved.
pub fn extract_sd7_to_work_dir(archive: &Path) -> Result<WorkDirScan> {
    let stem = archive
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("map");
    let map_name = stem.to_string();
    let work_dir = work_dir_for(archive);

    let should_extract = !work_dir.exists()
        || std::fs::read_dir(&work_dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(true);

    if should_extract {
        std::fs::create_dir_all(&work_dir)
            .with_context(|| format!("Failed to create work directory: {}", work_dir.display()))?;
        sevenz_rust::decompress_file(archive, &work_dir)
            .with_context(|| format!("Failed to extract '{}'", archive.display()))?;
    }

    scan_work_dir(work_dir, map_name)
}

/// Delete work directories under [`work_dir_root`] whose mtime is older than
/// `max_age`. Best-effort: errors on individual entries are logged and skipped
/// so a single permission issue cannot abort cleanup.
pub fn prune_old_work_dirs(max_age: Duration) {
    let root = work_dir_root();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    let cutoff = SystemTime::now()
        .checked_sub(max_age)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let too_old = entry
            .metadata()
            .and_then(|m| m.modified())
            .map(|mtime| mtime < cutoff)
            .unwrap_or(false);
        if too_old {
            if let Err(e) = std::fs::remove_dir_all(&path) {
                tracing::warn!(?path, error = %e, "Failed to prune stale work directory");
            }
        }
    }
}

fn scan_work_dir(work_dir: PathBuf, map_name: String) -> Result<WorkDirScan> {
    let mut smf_abs: Option<PathBuf> = None;
    let mut smf_rel: Option<PathBuf> = None;
    let mut smt_abs: Option<PathBuf> = None;
    let mut smt_rel: Option<PathBuf> = None;
    let mut passthrough_files: Vec<(PathBuf, PathBuf)> = Vec::new();

    scan_dir_recursive(
        &work_dir,
        &work_dir,
        &mut smf_abs,
        &mut smf_rel,
        &mut smt_abs,
        &mut smt_rel,
        &mut passthrough_files,
    )?;

    // Read SMF: extract header metadata plus heightmap/metalmap/typemap pixel data.
    let smf_data = smf_abs.as_ref().and_then(|abs| {
        let file = std::fs::File::open(abs).ok()?;
        bar_data::SmfMap::read(&mut std::io::BufReader::new(file)).ok()
    });

    let (tile_grid, map_dims, header_range) = smf_data
        .as_ref()
        .map(|smf| {
            let tg = smf.header.tile_grid_size();
            let (hw, hh) = smf.header.heightmap_size();
            let hr = (smf.header.min_height, smf.header.max_height);
            (Some(tg), Some((hw, hh)), Some(hr))
        })
        .unwrap_or((None, None, None));

    let mapinfo_override = std::fs::read_to_string(work_dir.join("mapinfo.lua"))
        .ok()
        .and_then(|s| crate::importer::parse_mapinfo_smf_heights(&s));
    let height_range = mapinfo_override.or(header_range);

    // Extract and embed heightmap, metalmap, typemap as hex-encoded u8 grids.
    // PaintedHeightmap supports up to 512; downsample to the largest power-of-2 <= 512.
    const MAX_RES: u32 = 512;

    let (heightmap_hex, heightmap_res) = smf_data
        .as_ref()
        .map(|smf| {
            let (w, h) = smf.header.heightmap_size();
            let target = largest_pow2_leq(w.min(h).min(MAX_RES));
            let pixels = downsample_f32_to_u8_square(smf.heightmap.data(), w, h, target);
            (hex_encode(&pixels), target)
        })
        .unwrap_or_default();

    let (metalmap_hex, metalmap_res) = smf_data
        .as_ref()
        .map(|smf| {
            let (w, h) = smf.header.metalmap_size();
            let target = largest_pow2_leq(w.min(h).min(MAX_RES));
            let pixels = downsample_u8_to_square(&smf.metalmap, w, h, target);
            (hex_encode(&pixels), target)
        })
        .unwrap_or_default();

    let (typemap_hex, typemap_res) = smf_data
        .as_ref()
        .map(|smf| {
            let (w, h) = smf.header.typemap_size();
            let target = largest_pow2_leq(w.min(h).min(MAX_RES));
            let pixels = downsample_u8_to_square(&smf.typemap, w, h, target);
            (hex_encode(&pixels), target)
        })
        .unwrap_or_default();

    // Assemble SMT texture into a 256x256 RGB hex blob for PaintedTexture.
    const TEX_RES: u32 = 256;
    let (texture_hex, texture_res) =
        if let (Some(smt_path), Some(smf)) = (smt_abs.as_ref(), smf_data.as_ref()) {
            let result: Option<(String, u32)> = (|| {
                let file = std::fs::File::open(smt_path).ok()?;
                let tiles = bar_data::smt::read_smt(&mut std::io::BufReader::new(file)).ok()?;
                let (tiles_x, tiles_y) = smf.header.tile_grid_size();
                if tiles_x == 0 || tiles_y == 0 {
                    return None;
                }
                let src_w = tiles_x * TILE_SIZE;
                let src_h = tiles_y * TILE_SIZE;
                let out_w = TEX_RES.min(src_w).max(1);
                let out_h = TEX_RES.min(src_h).max(1);
                let rgba = crate::executor::assemble_texture_preview(
                    &tiles,
                    &smf.tile_indices,
                    tiles_x,
                    tiles_y,
                    out_w,
                    out_h,
                );
                // Drop the alpha channel; PaintedTexture expects RGB (3 bytes/pixel).
                let rgb: Vec<u8> = rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
                Some((hex_encode(&rgb), TEX_RES))
            })();
            result.unwrap_or_default()
        } else {
            (String::new(), 0)
        };

    Ok(WorkDirScan {
        work_dir,
        map_name,
        smf_abs,
        smf_rel,
        smt_abs,
        smt_rel,
        tile_grid,
        map_dims,
        height_range,
        passthrough_files,
        heightmap_hex,
        heightmap_res,
        metalmap_hex,
        metalmap_res,
        typemap_hex,
        typemap_res,
        texture_hex,
        texture_res,
    })
}

/// Encode bytes as lowercase hex (2 chars per byte).
fn hex_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 2);
    for &b in data {
        let _ = write!(out, "{:02x}", b);
    }
    out
}

/// Largest power of 2 that is <= `n`. Returns 1 for n == 0.
fn largest_pow2_leq(n: u32) -> u32 {
    if n == 0 {
        return 1;
    }
    let mut p = 1u32;
    while p * 2 <= n {
        p *= 2;
    }
    p
}

/// Bilinear downsample of an f32 [0,1] `w x h` grid into a `res x res` u8 grid.
fn downsample_f32_to_u8_square(data: &[f32], w: u32, h: u32, res: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((res * res) as usize);
    for oy in 0..res {
        for ox in 0..res {
            let fx = (ox as f32 + 0.5) / res as f32 * w as f32 - 0.5;
            let fy = (oy as f32 + 0.5) / res as f32 * h as f32 - 0.5;
            let x0 = (fx as i32).clamp(0, w as i32 - 1) as u32;
            let y0 = (fy as i32).clamp(0, h as i32 - 1) as u32;
            let x1 = (x0 + 1).min(w - 1);
            let y1 = (y0 + 1).min(h - 1);
            let dx = (fx - fx.floor()).max(0.0);
            let dy = (fy - fy.floor()).max(0.0);
            let v00 = data[(y0 * w + x0) as usize];
            let v10 = data[(y0 * w + x1) as usize];
            let v01 = data[(y1 * w + x0) as usize];
            let v11 = data[(y1 * w + x1) as usize];
            let v = v00 * (1.0 - dx) * (1.0 - dy)
                + v10 * dx * (1.0 - dy)
                + v01 * (1.0 - dx) * dy
                + v11 * dx * dy;
            out.push((v.clamp(0.0, 1.0) * 255.0) as u8);
        }
    }
    out
}

/// Nearest-neighbor downsample of a u8 `w x h` grid into a `res x res` square.
fn downsample_u8_to_square(data: &[u8], w: u32, h: u32, res: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((res * res) as usize);
    for oy in 0..res {
        for ox in 0..res {
            let sx = (ox as u64 * w as u64 / res as u64).min(w as u64 - 1) as u32;
            let sy = (oy as u64 * h as u64 / res as u64).min(h as u64 - 1) as u32;
            out.push(data[(sy * w + sx) as usize]);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn scan_dir_recursive(
    root: &Path,
    dir: &Path,
    smf_abs: &mut Option<PathBuf>,
    smf_rel: &mut Option<PathBuf>,
    smt_abs: &mut Option<PathBuf>,
    smt_rel: &mut Option<PathBuf>,
    pass: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("Cannot read directory: {}", dir.display()))?
    {
        let entry = entry?;
        let abs = entry.path();
        if abs.is_dir() {
            scan_dir_recursive(root, &abs, smf_abs, smf_rel, smt_abs, smt_rel, pass)?;
        } else {
            let rel = abs.strip_prefix(root).unwrap_or(&abs).to_path_buf();
            let ext = abs
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();

            match ext.as_str() {
                "smf" if smf_abs.is_none() => {
                    *smf_abs = Some(abs);
                    *smf_rel = Some(rel);
                }
                "smt" if smt_abs.is_none() => {
                    *smt_abs = Some(abs);
                    *smt_rel = Some(rel);
                }
                _ => {
                    pass.push((abs, rel));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_encode_empty() {
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn hex_encode_zero_and_max_bytes() {
        assert_eq!(hex_encode(&[0x00]), "00");
        assert_eq!(hex_encode(&[0xff]), "ff");
    }

    #[test]
    fn hex_encode_multi_byte_lowercase() {
        assert_eq!(hex_encode(&[0x0a, 0xb0, 0xff]), "0ab0ff");
    }

    #[test]
    fn largest_pow2_leq_zero_returns_one() {
        assert_eq!(largest_pow2_leq(0), 1);
    }

    #[test]
    fn largest_pow2_leq_exact_powers() {
        assert_eq!(largest_pow2_leq(1), 1);
        assert_eq!(largest_pow2_leq(2), 2);
        assert_eq!(largest_pow2_leq(4), 4);
        assert_eq!(largest_pow2_leq(512), 512);
    }

    #[test]
    fn largest_pow2_leq_rounds_down() {
        assert_eq!(largest_pow2_leq(3), 2);
        assert_eq!(largest_pow2_leq(513), 512);
        assert_eq!(largest_pow2_leq(1000), 512);
    }

    #[test]
    fn downsample_f32_output_size() {
        let data = vec![0.5_f32; 4 * 4];
        let out = downsample_f32_to_u8_square(&data, 4, 4, 2);
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn downsample_f32_uniform_white_maps_to_255() {
        let data = vec![1.0_f32; 8 * 8];
        let out = downsample_f32_to_u8_square(&data, 8, 8, 4);
        assert!(out.iter().all(|&v| v == 255));
    }

    #[test]
    fn downsample_f32_uniform_black_maps_to_0() {
        let data = vec![0.0_f32; 4 * 4];
        let out = downsample_f32_to_u8_square(&data, 4, 4, 2);
        assert!(out.iter().all(|&v| v == 0));
    }

    #[test]
    fn downsample_u8_output_size() {
        let data = vec![128u8; 4 * 4];
        let out = downsample_u8_to_square(&data, 4, 4, 2);
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn downsample_u8_uniform_preserves_value() {
        let data = vec![42u8; 8 * 8];
        let out = downsample_u8_to_square(&data, 8, 8, 4);
        assert!(out.iter().all(|&v| v == 42));
    }
}

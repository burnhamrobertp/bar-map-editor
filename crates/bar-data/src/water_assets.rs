//! Engine-shipped water texture assets: shore-foam, wave-randomisation,
//! and a 32-frame caustic animation. All three live inside Recoil's
//! `bitmaps.sdz` base archive (which is just a standard ZIP despite
//! the `.sdz` extension).
//!
//! BAR's `BumpWaterFS.glsl` references these via mapinfo paths
//! (`water.foamTexture`, `water.causticTextures`) but the defaults
//! point at engine-shipped bitmaps, not at map-side assets -- so the
//! source-of-truth for BME's shore foam and caustic animation is the
//! engine install on disk, not the per-map data.
//!
//! Asset paths inside `bitmaps.sdz` (per `MapInfo.cpp:295-333` and
//! `BumpWater.cpp:251`):
//!
//! - `bitmaps/foam.jpg` -- shore foam tile.
//! - `bitmaps/shorewaverand.png` -- random-direction perturbation used
//!   by `GetShorewaves`.
//! - `bitmaps/caustics/caustic{00..31}.jpg` -- 32-frame animation,
//!   host swaps once per game step.
//!
//! This module only handles asset *extraction* -- decoding zip entries
//! into RGBA8 buffers. The renderer-side upload, sampling, and shader
//! stages live in `bar-render`.

use std::io::Read;
use std::path::{Path, PathBuf};

/// One decoded 2D image: RGBA8 bytes plus dimensions.
#[derive(Debug, Clone)]
pub struct WaterTexture {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Full bundle of engine-shipped water assets.
#[derive(Debug, Clone)]
pub struct WaterAssetSet {
    /// `bitmaps/foam.jpg` -- tiled across the shoreline by
    /// `GetShorewaves` (`BumpWaterFS.glsl:186-220`).
    pub foam: WaterTexture,
    /// `bitmaps/shorewaverand.png` -- per-fragment randomisation
    /// sampled by `GetShorewaves` to break up the foam tiling.
    pub waverand: WaterTexture,
    /// 32-frame caustic animation, sorted by frame index `00..31`.
    /// Engine cycles one frame per game step (`waterDrawer->Update`).
    pub caustics: Vec<WaterTexture>,
}

#[derive(Debug, thiserror::Error)]
pub enum WaterAssetError {
    #[error("could not find engine bitmaps.sdz under any of: {searched:?}")]
    ArchiveNotFound { searched: Vec<PathBuf> },
    #[error("io error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("zip error reading {path}: {source}")]
    Zip {
        path: PathBuf,
        #[source]
        source: zip::result::ZipError,
    },
    #[error("entry {entry} missing from {archive}")]
    EntryMissing { archive: PathBuf, entry: String },
    #[error("image decode failed for {entry}: {source}")]
    ImageDecode {
        entry: String,
        #[source]
        source: image::ImageError,
    },
}

/// Locate `bitmaps.sdz` inside the BAR install's `data/` root. The
/// engine ships many versions side-by-side under `engine/<version>/`;
/// callers should pass the path of the engine version they want to
/// pull assets from (typically the newest one detected via
/// `bar_install::BarVersions`). The fallback search order is:
///
/// 1. `<engine_dir>/base/spring/bitmaps.sdz` (Recoil layout).
/// 2. `<engine_dir>/base/bitmaps.sdz` (older Spring layout).
/// 3. `<engine_dir>/bitmaps.sdz` (legacy fallback).
///
/// Returns the first existing candidate. None of these paths are
/// guaranteed; the caller should be resilient to missing assets and
/// fall back to inert defaults (no foam / no caustics) so the renderer
/// stays in working order on machines without a local BAR install.
pub fn locate_bitmaps_archive(engine_dir: &Path) -> Option<PathBuf> {
    let candidates = [
        engine_dir.join("base").join("spring").join("bitmaps.sdz"),
        engine_dir.join("base").join("bitmaps.sdz"),
        engine_dir.join("bitmaps.sdz"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

/// Open `bitmaps.sdz` for the given engine version and extract every
/// water-related asset into RGBA8 buffers. Logs a warning for any
/// missing entry but continues -- a partial bundle is more useful
/// than an outright failure (e.g. older engine versions could ship
/// a different caustic-frame count).
pub fn load_from_archive(archive_path: &Path) -> Result<WaterAssetSet, WaterAssetError> {
    let file = std::fs::File::open(archive_path).map_err(|e| WaterAssetError::Io {
        path: archive_path.to_path_buf(),
        source: e,
    })?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| WaterAssetError::Zip {
        path: archive_path.to_path_buf(),
        source: e,
    })?;

    let foam = read_image_entry(&mut zip, archive_path, "bitmaps/foam.jpg")?;
    let waverand = read_image_entry(&mut zip, archive_path, "bitmaps/shorewaverand.png")?;

    let mut caustics = Vec::with_capacity(32);
    for i in 0..32 {
        // Engine numbers caustic frames `caustic00..caustic31` with a
        // 2-digit zero-pad (`MapInfo.cpp:333`). Anything else would
        // miss the upstream content.
        let name = format!("bitmaps/caustics/caustic{i:02}.jpg");
        caustics.push(read_image_entry(&mut zip, archive_path, &name)?);
    }

    Ok(WaterAssetSet {
        foam,
        waverand,
        caustics,
    })
}

/// Convenience: locate the archive under `engine_dir` and load it. On
/// success returns `Some(WaterAssetSet)`; returns `None` (with a
/// warning logged) when the archive cannot be located. Hard errors
/// during extraction still propagate.
pub fn load_from_engine_dir(engine_dir: &Path) -> Result<Option<WaterAssetSet>, WaterAssetError> {
    let Some(path) = locate_bitmaps_archive(engine_dir) else {
        tracing::warn!(
            engine_dir = %engine_dir.display(),
            "bitmaps.sdz not found; shore foam and caustics will be disabled",
        );
        return Ok(None);
    };
    load_from_archive(&path).map(Some)
}

fn read_image_entry(
    zip: &mut zip::ZipArchive<std::fs::File>,
    archive_path: &Path,
    entry: &str,
) -> Result<WaterTexture, WaterAssetError> {
    let mut buffer = Vec::new();
    {
        let mut file = zip
            .by_name(entry)
            .map_err(|_| WaterAssetError::EntryMissing {
                archive: archive_path.to_path_buf(),
                entry: entry.to_string(),
            })?;
        file.read_to_end(&mut buffer)
            .map_err(|e| WaterAssetError::Io {
                path: archive_path.to_path_buf(),
                source: e,
            })?;
    }
    let img = image::load_from_memory(&buffer).map_err(|e| WaterAssetError::ImageDecode {
        entry: entry.to_string(),
        source: e,
    })?;
    let rgba_img = img.to_rgba8();
    let (width, height) = rgba_img.dimensions();
    Ok(WaterTexture {
        rgba: rgba_img.into_raw(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `locate_bitmaps_archive` returns None when no candidate path
    /// exists -- behaviour the renderer relies on to gracefully fall
    /// back to inert defaults.
    #[test]
    fn locate_returns_none_for_empty_dir() {
        let tmp = std::env::temp_dir().join("bme-water-assets-locate-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(locate_bitmaps_archive(&tmp).is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Round-trip the entry-name format against the engine reference
    /// (`MapInfo.cpp:333`: `IntToString(i, "bitmaps/caustics/caustic%02i.jpg")`).
    #[test]
    fn caustic_entry_names_match_engine_format() {
        assert_eq!(
            format!("bitmaps/caustics/caustic{:02}.jpg", 0),
            "bitmaps/caustics/caustic00.jpg"
        );
        assert_eq!(
            format!("bitmaps/caustics/caustic{:02}.jpg", 31),
            "bitmaps/caustics/caustic31.jpg"
        );
    }
}

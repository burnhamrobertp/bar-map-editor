//! On-disk cache for feature-palette thumbnails.
//!
//! Once a thumbnail is rendered, we persist the RGBA pixels as a PNG
//! under the user's cache directory. Subsequent app launches read the
//! PNG back, decode it, and hand the pixels to egui via
//! `ctx.load_texture` -- no GPU render needed for cached features.
//!
//! Cache key is the lowercase feature type name. Invalidation is
//! manual: deleting the cache directory forces a fresh render on the
//! next launch. The cache key is intentionally simple; matching the
//! exact S3O content hash would require reading the S3O up front,
//! which defeats the point of caching.

use std::path::PathBuf;

use directories::ProjectDirs;

const SUBDIR: &str = "feature_thumbs";

/// Root directory for thumbnail PNGs. Returns `None` if the platform
/// doesn't expose a usable project cache root.
pub fn cache_dir() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "BarEditor", "BarEditor")?;
    Some(dirs.cache_dir().join(SUBDIR))
}

/// Path the PNG for `feature_type` lives at (whether it exists or not).
/// Caller is responsible for case-normalising `feature_type` to
/// lowercase before passing in.
pub fn path_for(feature_type: &str) -> Option<PathBuf> {
    Some(cache_dir()?.join(format!("{feature_type}.png")))
}

/// Decode a cached PNG for `feature_type` into RGBA8 pixels + width/
/// height. `None` when no cache file exists or decoding fails (in
/// either case the caller should render fresh and write the result).
pub fn read(feature_type: &str) -> Option<(Vec<u8>, u32, u32)> {
    let path = path_for(feature_type)?;
    let bytes = std::fs::read(&path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Some((img.into_raw(), w, h))
}

/// Persist a freshly rendered thumbnail. Creates the cache directory
/// on first use; quietly drops the write on error (the in-memory
/// `feature_thumb_cache` still has the texture, so the UI is fine --
/// we just won't have a persistent copy for next launch).
pub fn write(feature_type: &str, rgba: &[u8], w: u32, h: u32) {
    let Some(path) = path_for(feature_type) else {
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!(err = %e, dir = ?parent, "Feature thumb cache: mkdir failed");
            return;
        }
    }
    let img = match image::RgbaImage::from_raw(w, h, rgba.to_vec()) {
        Some(i) => i,
        None => {
            tracing::warn!("Feature thumb cache: invalid RGBA dims");
            return;
        }
    };
    if let Err(e) = img.save_with_format(&path, image::ImageFormat::Png) {
        tracing::warn!(err = %e, path = ?path, "Feature thumb cache: save failed");
    }
}

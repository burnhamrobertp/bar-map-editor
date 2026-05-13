//! Directory-as-package format for `.barproj` projects.
//!
//! A `.barproj` project is a directory containing:
//!   recipe.json   -- node graph + output config (no binary blobs)
//!   layout.json   -- editor visual state
//!   assets/       -- binary data files referenced by recipe.json
//!   passthrough/  -- verbatim files bundled into the exported .sd7
//!   autosave/     -- rolling recipe-only autosave snapshots
//!
//! Binary assets use a 24-byte header followed by raw bytes. The header
//! lets the reader know dimensions and pixel format without an external
//! codec dependency in this crate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use anyhow::{Context, Result};

// ── Asset identity ─────────────────────────────────────────────────────────

/// Stable identifier for a binary asset. UUID v4, serialised as a hyphenated
/// lowercase string. Assigned once at first write; never changes on
/// subsequent saves even if the node is renamed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetId(pub String);

impl AssetId {
    /// Generate a fresh random UUID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for AssetId {
    fn default() -> Self {
        Self::new()
    }
}

// ── Asset file format ──────────────────────────────────────────────────────

/// Pixel format tag stored in the asset file header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AssetKind {
    GrayscaleU8 = 0x01,
    GrayscaleF32 = 0x02,
    RgbU8 = 0x03,
}

impl AssetKind {
    fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Self::GrayscaleU8),
            0x02 => Some(Self::GrayscaleF32),
            0x03 => Some(Self::RgbU8),
            _ => None,
        }
    }

    /// Bytes per pixel for this format.
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            Self::GrayscaleU8 => 1,
            Self::GrayscaleF32 => 4,
            Self::RgbU8 => 3,
        }
    }
}

/// Dimensions + format of a binary asset.
#[derive(Debug, Clone, Copy)]
pub struct AssetHeader {
    pub kind: AssetKind,
    pub width: u32,
    pub height: u32,
}

const ASSET_MAGIC: &[u8; 8] = b"BARASSET";
const ASSET_VERSION: u8 = 1;
// Header layout: magic(8) + version(1) + kind(1) + width(4) + height(4) + reserved(6) = 24 bytes
const HEADER_SIZE: usize = 24;

/// Write a binary asset file: 24-byte header followed by raw pixel data.
pub fn write_asset_file(path: &Path, header: AssetHeader, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create {}", parent.display()))?;
    }
    let mut buf = Vec::with_capacity(HEADER_SIZE + data.len());
    buf.extend_from_slice(ASSET_MAGIC);
    buf.push(ASSET_VERSION);
    buf.push(header.kind as u8);
    buf.extend_from_slice(&header.width.to_le_bytes());
    buf.extend_from_slice(&header.height.to_le_bytes());
    buf.extend_from_slice(&[0u8; 6]); // reserved
    buf.extend_from_slice(data);
    std::fs::write(path, &buf).with_context(|| format!("Failed to write asset {}", path.display()))
}

/// Read a binary asset file. Returns the header and raw pixel bytes.
pub fn read_asset_file(path: &Path) -> Result<(AssetHeader, Vec<u8>)> {
    let raw =
        std::fs::read(path).with_context(|| format!("Cannot read asset {}", path.display()))?;
    anyhow::ensure!(
        raw.len() >= HEADER_SIZE,
        "Asset file too small: {} ({} bytes)",
        path.display(),
        raw.len()
    );
    anyhow::ensure!(
        &raw[..8] == ASSET_MAGIC,
        "Bad magic in asset file {}",
        path.display()
    );
    let kind = AssetKind::from_byte(raw[9])
        .with_context(|| format!("Unknown asset kind 0x{:02x} in {}", raw[9], path.display()))?;
    let width = u32::from_le_bytes(raw[10..14].try_into().unwrap());
    let height = u32::from_le_bytes(raw[14..18].try_into().unwrap());
    let data = raw[HEADER_SIZE..].to_vec();
    Ok((
        AssetHeader {
            kind,
            width,
            height,
        },
        data,
    ))
}

// ── Compile fingerprint ────────────────────────────────────────────────────

/// Metadata written to `compiled/fingerprint.json` after each successful
/// compile. Used to determine whether the compiled output is stale.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fingerprint {
    /// FNV-64 hash of `recipe.json` at compile time (hex string).
    pub recipe_hash: String,
    /// Map dimensions at compile time (map_x = output.width - 1).
    pub map_x: u32,
    pub map_y: u32,
    /// Tile grid dimensions for the compiled SMT (tiles_x = map_x/4, etc.).
    /// Zero when the fingerprint was written before this field was added.
    #[serde(default)]
    pub tiles_x: u32,
    #[serde(default)]
    pub tiles_y: u32,
    /// Size and mtime of each asset file that was live at compile time.
    pub assets: HashMap<String, AssetStat>,
}

/// File-level stats for one asset used in the staleness check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetStat {
    pub size: u64,
    pub mtime_secs: u64,
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// FNV-64 hash (no external dep).
fn fnv64(data: &[u8]) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01B3;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

// ── PackageDir ─────────────────────────────────────────────────────────────

/// A handle to an open `.barproj` directory package.
pub struct PackageDir {
    pub root: PathBuf,
}

impl PackageDir {
    /// Open an existing package directory. Validates that `recipe.json` exists.
    pub fn open(path: &Path) -> Result<Self> {
        anyhow::ensure!(path.is_dir(), "Not a directory: {}", path.display());
        anyhow::ensure!(
            path.join("recipe.json").exists(),
            "Missing recipe.json in {}",
            path.display()
        );
        Ok(Self {
            root: path.to_path_buf(),
        })
    }

    /// Create a new package directory at `path`, creating subdirectories.
    pub fn create(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path)
            .with_context(|| format!("Cannot create {}", path.display()))?;
        std::fs::create_dir_all(path.join("assets"))
            .with_context(|| format!("Cannot create assets dir in {}", path.display()))?;
        std::fs::create_dir_all(path.join("passthrough"))
            .with_context(|| format!("Cannot create passthrough dir in {}", path.display()))?;
        std::fs::create_dir_all(path.join("autosave"))
            .with_context(|| format!("Cannot create autosave dir in {}", path.display()))?;
        Ok(Self {
            root: path.to_path_buf(),
        })
    }

    pub fn recipe_path(&self) -> PathBuf {
        self.root.join("recipe.json")
    }

    pub fn layout_path(&self) -> PathBuf {
        self.root.join("layout.json")
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.root.join("assets")
    }

    pub fn passthrough_dir(&self) -> PathBuf {
        self.root.join("passthrough")
    }

    pub fn autosave_dir(&self) -> PathBuf {
        self.root.join("autosave")
    }

    /// Absolute path to the binary file for this asset.
    pub fn asset_path(&self, id: &AssetId) -> PathBuf {
        self.root.join("assets").join(format!("{}.bin", id.0))
    }

    pub fn write_asset(&self, id: &AssetId, header: AssetHeader, data: &[u8]) -> Result<()> {
        write_asset_file(&self.asset_path(id), header, data)
    }

    pub fn read_asset(&self, id: &AssetId) -> Result<(AssetHeader, Vec<u8>)> {
        read_asset_file(&self.asset_path(id))
    }

    pub fn asset_exists(&self, id: &AssetId) -> bool {
        self.asset_path(id).exists()
    }

    pub fn compiled_dir(&self) -> PathBuf {
        self.root.join("compiled")
    }

    /// Absolute path to the compiled SMT for a given recipe name.
    /// Sanitizes the name to be filesystem-safe (alphanumeric, `-`, `_` only).
    pub fn compiled_smt_path(&self, recipe_name: &str) -> PathBuf {
        self.compiled_dir()
            .join(format!("{}.smt", sanitize_filename(recipe_name)))
    }

    /// Absolute path to the compiled tile index file.
    pub fn compiled_tile_index_path(&self) -> PathBuf {
        self.compiled_dir().join("tile_index.bin")
    }

    /// True when `compiled/fingerprint.json` exists.
    pub fn is_compiled(&self) -> bool {
        self.compiled_dir().join("fingerprint.json").exists()
    }

    /// Read the fingerprint written by the last compile. Returns `None` on
    /// missing file or parse error.
    pub fn read_fingerprint(&self) -> Option<Fingerprint> {
        let s = std::fs::read_to_string(self.compiled_dir().join("fingerprint.json")).ok()?;
        serde_json::from_str(&s).ok()
    }

    /// Write `fingerprint.json` into the compiled dir.
    pub fn write_fingerprint(&self, fp: &Fingerprint) -> anyhow::Result<()> {
        let dir = self.compiled_dir();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Cannot create compiled dir {}", dir.display()))?;
        let s = serde_json::to_string_pretty(fp)?;
        std::fs::write(dir.join("fingerprint.json"), s)?;
        Ok(())
    }

    /// True when the compiled output is absent or does not match the current
    /// recipe + map dims. Asset stats are checked against the on-disk `assets/`
    /// directory.
    pub fn is_stale(&self, recipe_json: &str, map_x: u32, map_y: u32) -> bool {
        let Some(fp) = self.read_fingerprint() else {
            return true;
        };
        let recipe_hash = format!("{:016x}", fnv64(recipe_json.as_bytes()));
        if fp.recipe_hash != recipe_hash || fp.map_x != map_x || fp.map_y != map_y {
            return true;
        }
        let assets_dir = self.assets_dir();
        for (name, stat) in &fp.assets {
            match std::fs::metadata(assets_dir.join(name)) {
                Ok(m) => {
                    if m.len() != stat.size {
                        return true;
                    }
                    let mtime = m
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    if mtime != stat.mtime_secs {
                        return true;
                    }
                }
                Err(_) => return true,
            }
        }
        false
    }

    /// Save a timestamped autosave snapshot of `recipe_json` inside `autosave/`.
    pub fn save_autosave(&self, recipe_json: &str) -> Result<()> {
        let autosave_dir = self.autosave_dir();
        std::fs::create_dir_all(&autosave_dir)
            .with_context(|| format!("Cannot create autosave dir {}", autosave_dir.display()))?;
        let ts = chrono_timestamp();
        let path = autosave_dir.join(format!("recipe_{ts}.json"));
        std::fs::write(&path, recipe_json)
            .with_context(|| format!("Failed to write autosave {}", path.display()))
    }

    /// Remove oldest autosave files, keeping at most `keep` slots.
    pub fn prune_autosaves(&self, keep: usize) {
        let dir = self.autosave_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("recipe_"))
            .map(|e| e.path())
            .collect();
        // Sort ascending by name (timestamp prefix makes lexicographic = chronological).
        files.sort();
        if files.len() > keep {
            for old in &files[..files.len() - keep] {
                let _ = std::fs::remove_file(old);
            }
        }
    }
}

/// Current UTC timestamp in `YYYYMMDDTHHmmss` form — filesystem-safe on all platforms.
fn chrono_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Manual ISO basic timestamp from Unix seconds (no chrono dep).
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    // Days since epoch -> Gregorian date (Zeller-like, good enough for filenames).
    let (y, mo, d) = days_to_ymd(days);
    format!("{y:04}{mo:02}{d:02}T{h:02}{m:02}{s:02}")
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z % 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_roundtrip() {
        let dir = std::env::temp_dir().join("bar_package_test_asset");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.bin");
        let data = vec![10u8, 20, 30, 40, 50, 60];
        let header = AssetHeader {
            kind: AssetKind::GrayscaleU8,
            width: 2,
            height: 3,
        };
        write_asset_file(&path, header, &data).unwrap();
        let (h2, d2) = read_asset_file(&path).unwrap();
        assert_eq!(h2.width, 2);
        assert_eq!(h2.height, 3);
        assert!(matches!(h2.kind, AssetKind::GrayscaleU8));
        assert_eq!(d2, data);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn package_dir_create_and_open() {
        let dir = std::env::temp_dir().join("bar_package_test_dir");
        let pkg_path = dir.join("mymap.barproj");
        let _ = std::fs::remove_dir_all(&pkg_path);
        let pkg = PackageDir::create(&pkg_path).unwrap();
        // Create a recipe.json so open() validates
        std::fs::write(pkg.recipe_path(), "{}").unwrap();
        let pkg2 = PackageDir::open(&pkg_path).unwrap();
        assert_eq!(pkg2.root, pkg_path);
        std::fs::remove_dir_all(&dir).ok();
    }
}

//! Extract .sd7 archives into a work directory and scan their contents.
//!
//! Work directories live under an app-controlled cache directory rather than
//! next to the source archive, so opening a read-only or shared `.sd7` does
//! not litter the user's filesystem. Each archive maps to a stable subdirectory
//! keyed by a hash of its absolute path, so re-opening the same archive
//! preserves any in-place edits the user has made between sessions.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
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
        std::fs::create_dir_all(&work_dir).with_context(|| {
            format!("Failed to create work directory: {}", work_dir.display())
        })?;
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

    // Read tile grid dimensions, map pixel size, and height range from the
    // SMF header. Then check mapinfo.lua: Spring/BAR uses smf.minheight /
    // smf.maxheight from the lua file (when present) to override the binary
    // header's range. Many maps allocate generous header headroom but
    // specify the working range in lua, so failing to honour the override
    // produces flat previews that don't match in-game appearance.
    let (tile_grid, map_dims, header_range) = smf_abs.as_ref()
        .and_then(|abs| {
            let file = std::fs::File::open(abs).ok()?;
            let smf = bar_data::SmfMap::read(&mut std::io::BufReader::new(file)).ok()?;
            let tg = smf.header.tile_grid_size();
            let (hw, hh) = smf.header.heightmap_size();
            let hr = (smf.header.min_height, smf.header.max_height);
            Some((tg, (hw, hh), hr))
        })
        .map(|(tg, dims, hr)| (Some(tg), Some(dims), Some(hr)))
        .unwrap_or((None, None, None));

    let mapinfo_override = std::fs::read_to_string(work_dir.join("mapinfo.lua"))
        .ok()
        .and_then(|s| crate::importer::parse_mapinfo_smf_heights(&s));
    let height_range = mapinfo_override.or(header_range);

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
    })
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
            let rel = abs
                .strip_prefix(root)
                .unwrap_or(&abs)
                .to_path_buf();
            let ext = abs
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();

            match ext.as_str() {
                "smf" => {
                    if smf_abs.is_none() {
                        *smf_abs = Some(abs);
                        *smf_rel = Some(rel);
                    } else {
                        pass.push((abs, rel));
                    }
                }
                "smt" => {
                    if smt_abs.is_none() {
                        *smt_abs = Some(abs);
                        *smt_rel = Some(rel);
                    } else {
                        pass.push((abs, rel));
                    }
                }
                _ => {
                    pass.push((abs, rel));
                }
            }
        }
    }
    Ok(())
}

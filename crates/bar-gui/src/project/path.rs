//! Pure path-handling helpers used by project save/load:
//!
//! - `bar://...` project-relative URL scheme (so `.barproj` directories stay
//!   portable when copied to a new location).
//! - Asset packing: copy external files into the project's `assets/` or
//!   `passthrough/` subdirectory and rewrite param values to `bar://` URLs.
//! - PassThrough file-list parse / pack / resolve.
//!
//! Functions here are pure: they take params + paths, return new
//! params or perform local file I/O. They never touch
//! `BarEditorApp`. Tested implicitly via the project round-trip
//! tests in `crate::app` and `bar_project`.

use std::collections::HashMap;

use bar_graph::{GraphEngine, NodeType, ParamValue};

/// Marker for project-relative paths in saved `.barproj` files. Anything
/// starting with this prefix is resolved against the project's directory
/// at load time.
pub(crate) const PROJECT_RELATIVE_PREFIX: &str = "bar://";

/// Parse the `files` string stored in a PassThrough node's params.
pub(crate) fn parse_passthrough_files(s: &str) -> Vec<(String, String)> {
    s.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '|');
            let abs = parts.next()?.trim().to_string();
            let rel = parts.next()?.trim().to_string();
            if abs.is_empty() {
                None
            } else {
                Some((abs, rel))
            }
        })
        .collect()
}

/// True if `candidate` is inside `dir` (lexically -- both must be
/// absolute, or both relative; we only canonicalise the absolute case).
pub(crate) fn path_is_inside(candidate: &str, dir: &std::path::Path) -> bool {
    let p = std::path::Path::new(candidate);
    let canon_p = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let canon_d = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    canon_p.starts_with(canon_d)
}

/// Build a project-relative `bar://assets/<file_name>` URL.
pub(crate) fn asset_url(file_name: &str) -> String {
    format!("{PROJECT_RELATIVE_PREFIX}assets/{file_name}")
}

/// Build a project-relative `bar://passthrough/<bundle>` URL.
pub(crate) fn passthrough_url(bundle: &str) -> String {
    format!("{PROJECT_RELATIVE_PREFIX}passthrough/{bundle}")
}

/// Resolve a path that might be project-relative (`bar://...`) against the
/// project's directory. Returns absolute on-disk path. Pass-through for
/// already-absolute paths.
pub(crate) fn resolve_project_path(value: &str, project_dir: &std::path::Path) -> String {
    if let Some(rest) = value.strip_prefix(PROJECT_RELATIVE_PREFIX) {
        project_dir.join(rest).to_string_lossy().into_owned()
    } else {
        value.to_string()
    }
}

/// If the param holds an external file path, copy it into `assets_dir` and
/// rewrite the param to a `bar://assets/<file>` URL. No-op for missing keys,
/// empty strings, or paths already inside the project directory.
pub(crate) fn pack_path_param(
    params: &mut HashMap<String, ParamValue>,
    key: &str,
    project_dir: &std::path::Path,
    assets_dir: &std::path::Path,
    _bundle_subdir: &str,
) -> Result<(), String> {
    let Some(ParamValue::String(s)) = params.get(key).cloned() else {
        return Ok(());
    };
    if s.is_empty() || s.starts_with(PROJECT_RELATIVE_PREFIX) {
        return Ok(());
    }
    if path_is_inside(&s, project_dir) {
        return Ok(());
    }
    let src = std::path::PathBuf::from(&s);
    let file_name = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("Invalid file name in '{s}'"))?
        .to_string();
    std::fs::create_dir_all(assets_dir)
        .map_err(|e| format!("Cannot create assets dir {}: {e}", assets_dir.display()))?;
    let dest = assets_dir.join(&file_name);
    if !dest.exists() || !files_equal(&src, &dest) {
        std::fs::copy(&src, &dest).map_err(|e| {
            format!(
                "Failed to copy {} -> {}: {e}",
                src.display(),
                dest.display()
            )
        })?;
    }
    params.insert(key.to_string(), ParamValue::String(asset_url(&file_name)));
    Ok(())
}

/// Pack a PassThrough node's `files` param. Each line is `abs|bundle_path`;
/// we copy `abs` to `<passthrough_dir>/<bundle_path>` and rewrite the line to
/// `bar://passthrough/<bundle_path>|<bundle_path>`.
pub(crate) fn pack_passthrough_files(
    params: &mut HashMap<String, ParamValue>,
    project_dir: &std::path::Path,
    passthrough_dir: &std::path::Path,
) -> Result<(), String> {
    let Some(ParamValue::String(s)) = params.get("files").cloned() else {
        return Ok(());
    };
    let mut new_lines = Vec::new();
    for line in s.lines() {
        let mut parts = line.splitn(2, '|');
        let Some(abs) = parts.next() else {
            continue;
        };
        let abs = abs.trim();
        let bundle = parts.next().unwrap_or("").trim().to_string();
        if abs.is_empty() {
            continue;
        }
        if abs.starts_with(PROJECT_RELATIVE_PREFIX) || path_is_inside(abs, project_dir) {
            new_lines.push(format!("{abs}|{bundle}"));
            continue;
        }
        let dest = passthrough_dir.join(&bundle);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
        }
        let src = std::path::Path::new(abs);
        if !dest.exists() || !files_equal(src, &dest) {
            std::fs::copy(src, &dest).map_err(|e| {
                format!(
                    "Failed to copy {} -> {}: {e}",
                    src.display(),
                    dest.display()
                )
            })?;
        }
        new_lines.push(format!("{}|{bundle}", passthrough_url(&bundle)));
    }
    params.insert(
        "files".to_string(),
        ParamValue::String(new_lines.join("\n")),
    );
    Ok(())
}

/// Inverse of `pack_path_param`: rewrite a single param value from
/// `bar://...` to an absolute path anchored at `project_dir`.
pub(crate) fn resolve_path_param(
    params: &mut HashMap<String, ParamValue>,
    key: &str,
    project_dir: &std::path::Path,
) {
    if let Some(ParamValue::String(s)) = params.get(key).cloned() {
        let resolved = resolve_project_path(&s, project_dir);
        if resolved != s {
            params.insert(key.to_string(), ParamValue::String(resolved));
        }
    }
}

/// Inverse of `pack_passthrough_files`: rewrite any `bar://...` entries in
/// the `files` param's abs column to absolute paths.
pub(crate) fn resolve_passthrough_files(
    params: &mut HashMap<String, ParamValue>,
    project_dir: &std::path::Path,
) {
    let Some(ParamValue::String(s)) = params.get("files").cloned() else {
        return;
    };
    let mut changed = false;
    let mut out = Vec::new();
    for line in s.lines() {
        let mut parts = line.splitn(2, '|');
        let abs = parts.next().unwrap_or("").trim();
        let bundle = parts.next().unwrap_or("").trim();
        if abs.is_empty() {
            continue;
        }
        let resolved = resolve_project_path(abs, project_dir);
        if resolved != abs {
            changed = true;
        }
        out.push(format!("{resolved}|{bundle}"));
    }
    if changed {
        params.insert("files".to_string(), ParamValue::String(out.join("\n")));
    }
}

/// Pack a painted node's binary asset. If `asset_path` points outside
/// `assets_dir` (e.g. during "Save As" to a different location), copies the
/// `.bin` file into `assets_dir` and updates `asset_path` to the new location.
/// No-op when the asset is already inside the project or not yet set.
pub(crate) fn pack_painted_asset(
    params: &mut HashMap<String, ParamValue>,
    _project_dir: &std::path::Path,
    assets_dir: &std::path::Path,
) -> Result<(), String> {
    let asset_id = match params.get("asset_id") {
        Some(ParamValue::String(s)) if !s.is_empty() => s.clone(),
        _ => return Ok(()),
    };
    let asset_path = match params.get("asset_path") {
        Some(ParamValue::String(s)) if !s.is_empty() => s.clone(),
        _ => return Ok(()),
    };
    if path_is_inside(&asset_path, assets_dir) {
        return Ok(());
    }
    std::fs::create_dir_all(assets_dir)
        .map_err(|e| format!("Cannot create assets dir {}: {e}", assets_dir.display()))?;
    let src = std::path::Path::new(&asset_path);
    let dest = assets_dir.join(format!("{asset_id}.bin"));
    if !dest.exists() || !files_equal(src, &dest) {
        std::fs::copy(src, &dest).map_err(|e| {
            format!(
                "Failed to copy asset {} -> {}: {e}",
                src.display(),
                dest.display()
            )
        })?;
    }
    params.insert(
        "asset_path".to_string(),
        ParamValue::String(dest.to_string_lossy().into_owned()),
    );
    Ok(())
}

/// Copy a raw (non-BARASSET) asset from its current location to `assets_dir/<id>.<ext>`
/// and update the path param in-place. Skips the copy when the file is already
/// inside `assets_dir`.
pub(crate) fn pack_raw_asset(
    params: &mut HashMap<String, ParamValue>,
    assets_dir: &std::path::Path,
    id_param: &str,
    path_param: &str,
    ext: &str,
) -> Result<(), String> {
    let asset_id = match params.get(id_param) {
        Some(ParamValue::String(s)) if !s.is_empty() => s.clone(),
        _ => return Ok(()),
    };
    let asset_path = match params.get(path_param) {
        Some(ParamValue::String(s)) if !s.is_empty() => s.clone(),
        _ => return Ok(()),
    };
    if path_is_inside(&asset_path, assets_dir) {
        return Ok(());
    }
    std::fs::create_dir_all(assets_dir)
        .map_err(|e| format!("Cannot create assets dir {}: {e}", assets_dir.display()))?;
    let src = std::path::Path::new(&asset_path);
    let dest = assets_dir.join(format!("{asset_id}.{ext}"));
    if !dest.exists()
        || src.metadata().map(|m| m.len()).unwrap_or(0)
            != dest.metadata().map(|m| m.len()).unwrap_or(1)
    {
        std::fs::copy(src, &dest).map_err(|e| {
            format!(
                "Failed to copy asset {} -> {}: {e}",
                src.display(),
                dest.display()
            )
        })?;
    }
    params.insert(
        path_param.to_string(),
        ParamValue::String(dest.to_string_lossy().into_owned()),
    );
    Ok(())
}

/// Cheap "are these files identical" check by length first, then content.
/// Used to skip redundant copies on repeated saves to the same destination.
pub(crate) fn files_equal(a: &std::path::Path, b: &std::path::Path) -> bool {
    let (la, lb) = match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(ma), Ok(mb)) => (ma.len(), mb.len()),
        _ => return false,
    };
    if la != lb {
        return false;
    }
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

/// Walk every PassThrough node in the graph and return its (abs_path,
/// archive_path) entries flattened. Used by the Edit Map Info picker so the
/// user can pick from any text file currently in the bundle.
pub(crate) fn collect_all_passthrough_files(graph: &GraphEngine) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for node in graph.nodes().values() {
        if node.node_type != NodeType::PassThrough {
            continue;
        }
        if let Some(ParamValue::String(s)) = node.params.get("files") {
            out.extend(parse_passthrough_files(s));
        }
    }
    out
}

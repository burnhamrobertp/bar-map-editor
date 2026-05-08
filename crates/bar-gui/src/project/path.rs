//! Pure path-handling helpers used by project save/load:
//!
//! - `bar://...` project-relative URL scheme (so `.barproj` files stay
//!   portable when copied to a new directory).
//! - Asset packing: copy external files into `<stem>.assets/` and
//!   rewrite param values to `bar://` URLs.
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

/// Build the project-relative form of an asset's path under `<stem>.assets/`.
/// `bundle_subdir` is "maps" or "" -- it's the subfolder under .assets/ where
/// this kind of asset lives.
pub(crate) fn project_relative_for(
    bundle_subdir: &str,
    file_name: &str,
    project_stem: &str,
) -> String {
    let assets = format!("{project_stem}.assets");
    if bundle_subdir.is_empty() {
        format!("{PROJECT_RELATIVE_PREFIX}{assets}/{file_name}")
    } else {
        format!("{PROJECT_RELATIVE_PREFIX}{assets}/{bundle_subdir}/{file_name}")
    }
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

/// If the param holds an external file path, copy the file into
/// `<assets_dir>/<bundle_subdir>/` and rewrite the param to a project-relative
/// `bar://` URL. No-op for missing keys, empty strings, or paths already
/// inside the project directory.
pub(crate) fn pack_path_param(
    params: &mut HashMap<String, ParamValue>,
    key: &str,
    project_dir: &std::path::Path,
    assets_dir: &std::path::Path,
    bundle_subdir: &str,
) -> Result<(), String> {
    let Some(ParamValue::String(s)) = params.get(key).cloned() else {
        return Ok(());
    };
    if s.is_empty() || s.starts_with(PROJECT_RELATIVE_PREFIX) {
        return Ok(());
    }
    if path_is_inside(&s, project_dir) {
        return Ok(()); // already local
    }
    let src = std::path::PathBuf::from(&s);
    let file_name = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("Invalid file name in '{s}'"))?
        .to_string();
    let dest_dir = if bundle_subdir.is_empty() {
        assets_dir.to_path_buf()
    } else {
        assets_dir.join(bundle_subdir)
    };
    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("Cannot create assets dir {}: {e}", dest_dir.display()))?;
    let dest = dest_dir.join(&file_name);
    if !dest.exists() || !files_equal(&src, &dest) {
        std::fs::copy(&src, &dest).map_err(|e| {
            format!(
                "Failed to copy {} -> {}: {e}",
                src.display(),
                dest.display()
            )
        })?;
    }
    let stem = project_dir
        .file_name() // dir name doesn't help; we need project stem
        .and_then(|s| s.to_str())
        .unwrap_or("");
    // Derive project stem from the assets_dir name ("<stem>.assets").
    let project_stem = assets_dir
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_suffix(".assets"))
        .unwrap_or(stem)
        .to_string();
    let new_value = project_relative_for(bundle_subdir, &file_name, &project_stem);
    params.insert(key.to_string(), ParamValue::String(new_value));
    Ok(())
}

/// Pack a PassThrough node's `files` param. Each line is `abs|bundle_path`;
/// we copy `abs` to `<assets_dir>/<bundle_path>` and rewrite the line to
/// `bar://<stem>.assets/<bundle_path>|<bundle_path>`.
pub(crate) fn pack_passthrough_files(
    params: &mut HashMap<String, ParamValue>,
    project_dir: &std::path::Path,
    assets_dir: &std::path::Path,
) -> Result<(), String> {
    let Some(ParamValue::String(s)) = params.get("files").cloned() else {
        return Ok(());
    };
    let project_stem = assets_dir
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_suffix(".assets"))
        .unwrap_or("")
        .to_string();
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
        let dest = assets_dir.join(&bundle);
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
        let new_abs = format!("{PROJECT_RELATIVE_PREFIX}{project_stem}.assets/{bundle}");
        new_lines.push(format!("{new_abs}|{bundle}"));
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

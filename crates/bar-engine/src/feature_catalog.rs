// SPDX-License-Identifier: GPL-2.0-or-later
//! BAR game archive feature catalog.
//!
//! Reads `features/*.lua` from a BAR game archive (.sdz = ZIP, .sdd =
//! directory on disk) or from the Spring rapid pool alongside a .sdd install.
//! The rapid pool stores game content downloaded by the BAR launcher in
//! `data/pool/<xx>/<yyyyyy>.gz` (gzip-compressed), indexed by SDP manifests
//! in `data/packages/*.sdp` (also gzip-compressed binary records).

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// Metadata for one BAR feature type, extracted from Lua definitions.
#[derive(Debug, Clone)]
pub struct FeatureDef {
    pub name: String,
    pub object: String,
    pub footprint_x: u32,
    pub footprint_z: u32,
}

/// All feature definitions found in a BAR game archive.
/// Keys are lowercase feature type names matching the string in placed-feature
/// records (e.g. "arborreal_short_02").
#[derive(Debug, Clone, Default)]
pub struct FeatureCatalog {
    pub features: HashMap<String, FeatureDef>,
}

impl FeatureCatalog {
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// Return true if `feature_type` matches a known catalog entry.
    /// Comparison is case-insensitive.
    pub fn is_known(&self, feature_type: &str) -> bool {
        self.features.contains_key(&feature_type.to_lowercase())
    }

    /// Merge another catalog into this one. Entries in `other` do not
    /// overwrite existing keys so the game-level catalog wins on conflict.
    pub fn merge(&mut self, other: FeatureCatalog) {
        for (k, v) in other.features {
            self.features.entry(k).or_insert(v);
        }
    }

    /// Load feature definitions from a plain directory (not an archive).
    /// Scans `dir/features/*.lua` and `dir/gamedata/featuredata.lua`.
    pub fn from_dir(dir: &Path) -> Self {
        let mut catalog = Self::default();
        for path in find_feature_lua_in_dir(dir) {
            if let Ok(content) = std::fs::read_to_string(&path) {
                parse_feature_lua(&content, &mut catalog.features);
            }
        }
        catalog
    }

    /// Load a catalog from a BAR game archive.
    ///
    /// - `.sdz` / `.sd7` treated as ZIP archives (BAR's sdz is ZIP-format)
    /// - `.sdd` treated as a plain directory
    ///
    /// Returns an empty catalog on any error so callers need not handle Err.
    pub fn from_archive(archive: &Path) -> Self {
        let ext = archive
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        let mut catalog = Self::default();

        match ext.as_str() {
            "sdz" | "sd7" => {
                scan_zip_for_features(archive, &mut catalog.features);
            }
            "sdd" => {
                // Scan the stub directory itself (usually very sparse).
                let paths = find_feature_lua_in_dir(archive);
                for path in paths {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        parse_feature_lua(&content, &mut catalog.features);
                    }
                }
                // Scan sibling .sdz / .sd7 archives in the same directory --
                // BAR.sdd is a launcher stub; the real game content lives in a
                // byar_*.sdz or similar archive next to it.
                for sibling in sibling_zip_archives(archive) {
                    scan_zip_for_features(&sibling, &mut catalog.features);
                }
                // Also load from the rapid pool (override / map-specific Lua).
                if let Some(data_dir) = archive.parent().and_then(|p| p.parent()) {
                    load_from_rapid_pool(data_dir, &mut catalog.features);
                }
            }
            _ => {}
        }

        catalog
    }
}

/// Read one named file from an `.sdz`/`.sd7` (ZIP) or `.sdd` (directory) game archive.
/// Returns `None` if the file is not found or cannot be read.
pub fn read_file_from_archive(archive: &std::path::Path, internal_path: &str) -> Option<Vec<u8>> {
    let ext = archive
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "sdz" | "sd7" => {
            let file = std::fs::File::open(archive).ok()?;
            let mut zip = zip::ZipArchive::new(file).ok()?;
            // Case-insensitive search (BAR archives use inconsistent case).
            let names: Vec<String> = (0..zip.len())
                .filter_map(|i| zip.by_index(i).ok().map(|e| e.name().to_string()))
                .collect();
            let matched = names
                .iter()
                .find(|n| n.eq_ignore_ascii_case(internal_path))?
                .clone();
            let mut entry = zip.by_name(&matched).ok()?;
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut buf).ok()?;
            Some(buf)
        }
        "sdd" => {
            // Directory archive: just join the path.
            let path = archive.join(internal_path.replace('/', std::path::MAIN_SEPARATOR_STR));
            std::fs::read(&path).ok()
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Rapid pool reader
// ---------------------------------------------------------------------------

/// Parse all SDP manifests in `data_dir/packages/` and load any
/// `features/*.lua` files they reference from `data_dir/pool/`.
fn load_from_rapid_pool(data_dir: &Path, features: &mut HashMap<String, FeatureDef>) {
    use flate2::read::GzDecoder;

    let packages_dir = data_dir.join("packages");
    let pool_dir = data_dir.join("pool");
    if !packages_dir.is_dir() || !pool_dir.is_dir() {
        return;
    }

    let Ok(pkg_entries) = std::fs::read_dir(&packages_dir) else {
        return;
    };

    // Collect (path, md5_hex) for each features/*.lua across all SDPs.
    // Multiple SDPs may reference the same file (same MD5 = same content).
    let mut seen_md5: std::collections::HashSet<String> = std::collections::HashSet::new();

    for entry in pkg_entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("sdp") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            tracing::warn!(sdp = %path.display(), "failed to read SDP file");
            continue;
        };
        let mut decoder = GzDecoder::new(bytes.as_slice());
        let mut data = Vec::new();
        if decoder.read_to_end(&mut data).is_err() {
            tracing::warn!(sdp = %path.display(), "failed to decompress SDP file");
            continue;
        }
        let mut feature_lua_count = 0usize;
        let mut pos = 0usize;
        while pos < data.len() {
            let name_len = data[pos] as usize;
            pos += 1;
            if name_len == 0 || pos + name_len + 16 + 4 + 4 > data.len() {
                break;
            }
            let name = std::str::from_utf8(&data[pos..pos + name_len])
                .unwrap_or("")
                .to_string();
            pos += name_len;
            let md5 = &data[pos..pos + 16];
            pos += 16 + 4 + 4; // skip md5, crc32, size

            if !is_feature_lua(&name) {
                continue;
            }
            feature_lua_count += 1;
            let md5_hex = md5.iter().map(|b| format!("{b:02x}")).collect::<String>();
            if !seen_md5.insert(md5_hex.clone()) {
                tracing::debug!(name, "feature lua already loaded (duplicate md5), skipping");
                continue;
            }

            // Pool path: pool/<first2>/<rest30>.gz
            let pool_file = pool_dir
                .join(&md5_hex[..2])
                .join(format!("{}.gz", &md5_hex[2..]));
            let Ok(pool_bytes) = std::fs::read(&pool_file) else {
                tracing::warn!(name, md5 = %md5_hex, "pool file missing for feature lua");
                continue;
            };
            let mut lua_decoder = GzDecoder::new(pool_bytes.as_slice());
            let mut content = String::new();
            let before = features.len();
            if lua_decoder.read_to_string(&mut content).is_ok() {
                if name.to_lowercase() == "features/enginetrees_override.lua" {
                    parse_engine_trees_override(&content, features);
                } else {
                    parse_feature_lua(&content, features);
                }
                tracing::debug!(
                    name,
                    added = features.len() - before,
                    total = features.len(),
                    "loaded feature lua"
                );
            } else {
                tracing::warn!(name, "failed to decode feature lua as UTF-8");
            }
        }
        tracing::debug!(
            sdp = %path.file_name().unwrap_or_default().to_string_lossy(),
            feature_lua_count,
            "SDP scanned"
        );
    }

    tracing::info!("Rapid pool: loaded {} feature defs", features.len());
}

/// Return all `.sdz` / `.sd7` archives in the same directory as `archive`
/// (excluding `archive` itself). Used to find the real game content archive
/// when the user has a `.sdd` launcher stub selected.
pub fn sibling_zip_archives(archive: &Path) -> Vec<std::path::PathBuf> {
    let Some(parent) = archive.parent() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let ext = p
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.to_lowercase())
                .unwrap_or_default();
            if (ext == "sdz" || ext == "sd7") && p != archive {
                Some(p)
            } else {
                None
            }
        })
        .collect()
}

/// Scan a `.sdz` / `.sd7` ZIP archive and parse all feature Lua files into `features`.
pub fn scan_zip_for_features(archive: &Path, features: &mut HashMap<String, FeatureDef>) {
    let Ok(file) = std::fs::File::open(archive) else {
        return;
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return;
    };
    let lua_paths: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|e| e.name().to_string()))
        .filter(|n| is_feature_lua(n))
        .collect();
    let before = features.len();
    for path in lua_paths {
        if let Ok(mut entry) = zip.by_name(&path) {
            let mut content = String::new();
            if entry.read_to_string(&mut content).is_ok() {
                if path.to_lowercase() == "features/enginetrees_override.lua" {
                    parse_engine_trees_override(&content, features);
                } else {
                    parse_feature_lua(&content, features);
                }
            }
        }
    }
    tracing::debug!(
        archive = %archive.file_name().unwrap_or_default().to_string_lossy(),
        added = features.len() - before,
        total = features.len(),
        "scanned zip for feature defs"
    );
}

/// Read one file from `data_dir/pool/` using the SDP manifest index.
/// Scans all `.sdp` files in `data_dir/packages/` to locate the MD5 for `path`,
/// then decompresses and returns the raw bytes from the pool.
pub fn read_file_from_rapid_pool(data_dir: &Path, path: &str) -> Option<Vec<u8>> {
    use flate2::read::GzDecoder;
    use std::io::Read;

    let packages_dir = data_dir.join("packages");
    let pool_dir = data_dir.join("pool");
    if !packages_dir.is_dir() || !pool_dir.is_dir() {
        return None;
    }

    let path_lower = path.to_lowercase();
    let mut md5_hex: Option<String> = None;

    for entry in std::fs::read_dir(&packages_dir).ok()?.flatten() {
        if entry.path().extension().and_then(|e| e.to_str()) != Some("sdp") {
            continue;
        }
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        let mut decoder = GzDecoder::new(bytes.as_slice());
        let mut data = Vec::new();
        if decoder.read_to_end(&mut data).is_err() {
            continue;
        }
        let mut pos = 0usize;
        while pos < data.len() {
            let name_len = data[pos] as usize;
            pos += 1;
            if name_len == 0 || pos + name_len + 16 + 4 + 4 > data.len() {
                break;
            }
            let name = std::str::from_utf8(&data[pos..pos + name_len])
                .unwrap_or("")
                .to_lowercase();
            let md5 = &data[pos + name_len..pos + name_len + 16];
            pos += name_len + 16 + 4 + 4;
            if name == path_lower {
                md5_hex = Some(md5.iter().map(|b| format!("{b:02x}")).collect());
                break;
            }
        }
        if md5_hex.is_some() {
            break;
        }
    }

    let md5_hex = md5_hex?;
    let pool_file = pool_dir
        .join(&md5_hex[..2])
        .join(format!("{}.gz", &md5_hex[2..]));
    tracing::debug!(path, md5 = %md5_hex, pool = %pool_file.display(), "rapid pool lookup");
    let pool_bytes = std::fs::read(&pool_file).ok()?;
    let mut decoder = GzDecoder::new(pool_bytes.as_slice());
    let mut out = Vec::new();
    if let Err(e) = decoder.read_to_end(&mut out) {
        tracing::warn!(path, md5 = %md5_hex, err = %e, "gzip decompress failed in rapid pool");
        return None;
    }
    tracing::debug!(path, md5 = %md5_hex, decompressed_bytes = out.len(), "rapid pool decompressed");
    Some(out)
}

/// Parse `features/enginetrees_override.lua` which programmatically generates
/// treetype0..N definitions mapped to rotating S3O models.
///
/// The file defines a local `objects` array of `.s3o` filenames, then loops
/// `for i = 0, N do` assigning `treeDefs["treetype" .. i] = { object = ..., ... }`.
/// We extract the object list and loop bound without executing Lua.
fn parse_engine_trees_override(content: &str, features: &mut HashMap<String, FeatureDef>) {
    let mut objects: Vec<String> = Vec::new();
    let mut in_objects_array = false;
    let mut loop_max: u32 = 255;

    for line in content.lines() {
        let stripped = strip_comment(line).trim();

        if stripped.contains("local objects") && stripped.contains('{') {
            in_objects_array = true;
        }

        if in_objects_array {
            let mut rest = stripped;
            while let Some(start) = rest.find('"') {
                rest = &rest[start + 1..];
                if let Some(end) = rest.find('"') {
                    let s = &rest[..end];
                    if s.ends_with(".s3o") {
                        objects.push(s.to_string());
                    }
                    rest = &rest[end + 1..];
                } else {
                    break;
                }
            }
            // End of the objects table
            if stripped.contains('}') && !objects.is_empty() {
                in_objects_array = false;
            }
        }

        // "for i = 0, 255 do" or similar
        if let Some(after_for) = stripped.strip_prefix("for i = 0,") {
            let after = after_for.trim();
            if let Some(n_str) = after.split_whitespace().next() {
                if let Ok(n) = n_str.trim_end_matches(',').parse::<u32>() {
                    loop_max = n;
                }
            }
        }
    }

    // Fallback if parsing the objects array failed
    if objects.is_empty() {
        objects = vec![
            "fir_tree_smallest.s3o".to_string(),
            "fir_tree_small.s3o".to_string(),
            "fir_tree_medium.s3o".to_string(),
            "fir_tree_large.s3o".to_string(),
        ];
    }

    // Lua arrays are 1-indexed; (i % #objects) + 1 maps i to 1-based index.
    let n = objects.len();
    for i in 0..=loop_max {
        let name = format!("treetype{i}");
        let object = objects[(i as usize) % n].clone();
        features.entry(name.clone()).or_insert(FeatureDef {
            name,
            object,
            footprint_x: 1,
            footprint_z: 1,
        });
    }
}

fn is_feature_lua(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "gamedata/featuredefs.lua"
        || lower == "gamedata/featuredefs_post.lua"
        || (lower.starts_with("features/") && lower.ends_with(".lua"))
}

fn find_feature_lua_in_dir(root: &Path) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    let featuredata = root.join("gamedata").join("featuredefs.lua");
    if featuredata.exists() {
        result.push(featuredata);
    }
    let features_dir = root.join("features");
    if features_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&features_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("lua"))
                    == Some(true)
                {
                    result.push(path);
                }
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Lua state-machine parser
// ---------------------------------------------------------------------------

/// Count `{` and `}` on a line, skipping those inside string literals.
fn count_braces(s: &str) -> (i32, i32) {
    let mut open = 0i32;
    let mut close = 0i32;
    let mut in_str = false;
    let mut str_ch = '"';
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str {
            if c == '\\' {
                chars.next(); // skip escaped char
            } else if c == str_ch {
                in_str = false;
            }
        } else if c == '"' || c == '\'' {
            in_str = true;
            str_ch = c;
        } else if c == '{' {
            open += 1;
        } else if c == '}' {
            close += 1;
        }
    }
    (open, close)
}

/// Strip a Lua `--` line comment.  Does not handle block comments (`--[[`).
fn strip_comment(line: &str) -> &str {
    // Simple: find "--" not inside a string. Good enough for feature files.
    let mut in_str = false;
    let mut str_ch = '"';
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if c == '\\' {
                i += 2;
                continue;
            } else if c == str_ch {
                in_str = false;
            }
        } else if c == '"' || c == '\'' {
            in_str = true;
            str_ch = c;
        } else if i + 1 < bytes.len() && bytes[i] == b'-' && bytes[i + 1] == b'-' {
            return &line[..i];
        }
        i += 1;
    }
    line
}

/// Extract a feature name from a line of the form `name = {`.
/// Handles:
/// - bare identifiers: `armorplate = {`
/// - bracket keys: `['Tree Type 1'] = {` or `["Rock"] = {`
/// - table-indexed keys: `featureDefs["rock_large"] = {`
fn extract_feature_name(line: &str) -> Option<String> {
    let line = line.trim();
    let eq_pos = line.find('=')?;
    let key = line[..eq_pos].trim();
    let rest = line[eq_pos + 1..].trim();
    if !rest.starts_with('{') {
        return None;
    }
    // Bare identifier: `name = {`
    if key.chars().all(|c| c.is_alphanumeric() || c == '_') && !key.is_empty() {
        return Some(key.to_lowercase());
    }
    // Bracket key: `["name"] = {` or `['name'] = {`
    if key.starts_with('[') && key.ends_with(']') {
        let inner = key[1..key.len() - 1].trim();
        if (inner.starts_with('\'') && inner.ends_with('\''))
            || (inner.starts_with('"') && inner.ends_with('"'))
        {
            return Some(inner[1..inner.len() - 1].to_lowercase());
        }
    }
    // Table indexing: `tableName["name"] = {` or `tableName['name'] = {`
    if let Some(open) = key.rfind('[') {
        if key[open..].find(']').map(|c| open + c) == Some(key.len() - 1) {
            let inner = key[open + 1..key.len() - 1].trim();
            if (inner.starts_with('"') && inner.ends_with('"'))
                || (inner.starts_with('\'') && inner.ends_with('\''))
            {
                return Some(inner[1..inner.len() - 1].to_lowercase());
            }
        }
    }
    None
}

fn extract_string_field<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    let line = line.trim();
    // Match "field = value" (case-insensitive field name)
    if !line[..line.len().min(field.len())].eq_ignore_ascii_case(field) {
        return None;
    }
    let after_field = line[field.len()..].trim_start();
    let after_eq = after_field.strip_prefix('=')?;
    let value = after_eq.trim().trim_end_matches(',').trim();
    if (value.starts_with('\'') && value.ends_with('\''))
        || (value.starts_with('"') && value.ends_with('"'))
    {
        Some(&value[1..value.len() - 1])
    } else if !value.is_empty() && value != "nil" && value != "false" && value != "true" {
        Some(value)
    } else {
        None
    }
}

fn extract_u32_field(line: &str, field: &str) -> Option<u32> {
    let line = line.trim();
    if !line[..line.len().min(field.len())].eq_ignore_ascii_case(field) {
        return None;
    }
    let after_field = line[field.len()..].trim_start();
    let after_eq = after_field.strip_prefix('=')?;
    let value = after_eq.trim().trim_end_matches(',').trim();
    value.parse::<u32>().ok()
}

#[derive(Default)]
struct PartialDef {
    object: String,
    footprint_x: u32,
    footprint_z: u32,
}

/// Parse one Lua feature definition file into the `features` map.
///
/// mlua evaluates the file the same way Recoil does, so it's the source of
/// truth. The static text parser is only used as a fallback when mlua cannot
/// execute the file (e.g. it references engine-only globals like `Spring.*`
/// or `VFS.Include` that we do not expose). Earlier the order was reversed
/// and the static parser would extract nested keys (e.g. `customparams` inside
/// a `local Base = { ... }` template) as false-positive feature names, which
/// then suppressed the mlua fallback and skipped any features the file
/// actually generated via for-loops.
fn parse_feature_lua(content: &str, features: &mut HashMap<String, FeatureDef>) {
    let before = features.len();
    parse_feature_lua_dynamic(content, features);
    if features.len() == before {
        parse_feature_lua_at_depth(content, 1, features);
        parse_feature_lua_at_depth(content, 0, features);
    }
}

/// Evaluate a Lua feature file with mlua and extract the returned name->def table.
/// Handles the dynamic `for`-loop style used in map-bundled feature files.
///
/// Sandbox: TABLE + STRING + MATH only (base library always present; no IO,
/// OS, package, debug, coroutine, or utf8). 10 MB memory cap. 500k instruction
/// hook aborts runaway scripts.
fn parse_feature_lua_dynamic(content: &str, features: &mut HashMap<String, FeatureDef>) {
    use mlua::prelude::*;

    // Base library (pairs/ipairs/tostring/pcall/...) is always loaded.
    // Add only the safe extras needed for typical feature Lua files.
    let libs = LuaStdLib::TABLE | LuaStdLib::STRING | LuaStdLib::MATH;
    let lua = match Lua::new_with(libs, LuaOptions::default()) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(err = %e, "failed to create Lua state for feature parsing");
            return;
        }
    };

    if let Err(e) = lua.set_memory_limit(10 * 1024 * 1024) {
        tracing::debug!(err = %e, "Lua memory limit not supported");
    }

    lua.set_hook(
        LuaHookTriggers::new().every_nth_instruction(500_000),
        |_lua, _debug| {
            Err(LuaError::RuntimeError(
                "feature Lua: instruction limit exceeded".into(),
            ))
        },
    );

    let table = match lua.load(content).eval::<LuaValue>() {
        Ok(LuaValue::Table(t)) => t,
        Ok(_) => return,
        Err(e) => {
            tracing::debug!(err = %e, "feature Lua eval failed (skipped)");
            return;
        }
    };

    for pair in table.pairs::<LuaValue, LuaValue>() {
        let Ok((key, val)) = pair else { continue };
        let name = match &key {
            LuaValue::String(s) => match s.to_str() {
                Ok(s) => s.to_lowercase(),
                Err(_) => continue,
            },
            _ => continue,
        };
        let def_table = match val {
            LuaValue::Table(t) => t,
            _ => continue,
        };

        let object: String = def_table.get::<String>("object").unwrap_or_default();
        let footprint_x: u32 = def_table
            .get::<u32>("footprintX")
            .or_else(|_| def_table.get::<u32>("footprintx"))
            .unwrap_or(1);
        let footprint_z: u32 = def_table
            .get::<u32>("footprintZ")
            .or_else(|_| def_table.get::<u32>("footprintz"))
            .unwrap_or(1);

        if !name.is_empty() {
            features.entry(name.clone()).or_insert(FeatureDef {
                name,
                object,
                footprint_x,
                footprint_z,
            });
        }
    }
}

fn parse_feature_lua_at_depth(
    content: &str,
    outer: i32,
    features: &mut HashMap<String, FeatureDef>,
) {
    let inner = outer + 1;
    let mut depth: i32 = 0;
    let mut current_name: Option<String> = None;
    let mut partial = PartialDef::default();

    for line in content.lines() {
        let stripped = strip_comment(line);
        let (open, close) = count_braces(stripped);
        let net = open - close;
        let prev_depth = depth;
        depth += net;

        // Entering a feature definition
        if prev_depth == outer && depth >= inner {
            current_name = extract_feature_name(stripped);
            partial = PartialDef::default();
        }

        // Collecting fields inside a feature definition
        if prev_depth == inner && depth == inner {
            if let Some(v) = extract_string_field(stripped, "object") {
                partial.object = v.to_string();
            } else if let Some(v) = extract_u32_field(stripped, "footprintX") {
                partial.footprint_x = v;
            } else if let Some(v) = extract_u32_field(stripped, "footprintZ") {
                partial.footprint_z = v;
            }
        }

        // Closing a feature definition
        if prev_depth >= inner && depth <= outer {
            if let Some(name) = current_name.take() {
                // or_insert: depth-1 pass wins; depth-0 fills in any it missed
                features.entry(name.clone()).or_insert(FeatureDef {
                    name,
                    object: std::mem::take(&mut partial.object),
                    footprint_x: partial.footprint_x,
                    footprint_z: partial.footprint_z,
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
local featureDefs = {

arborreal_short_02 = {
    blocking = true,
    description = 'Short Arborreal',
    footprintX = 2,
    footprintZ = 2,
    object = 'arborreal_short_02',
},

['Tree Type 1'] = {
    footprintX = 3,
    footprintZ = 3,
    object = "tree_type1.s3o",
},

} -- end featureDefs
return featureDefs
"#;

    #[test]
    fn parses_bare_identifier() {
        let mut features = HashMap::new();
        parse_feature_lua(SAMPLE, &mut features);
        assert!(features.contains_key("arborreal_short_02"));
        let def = &features["arborreal_short_02"];
        assert_eq!(def.footprint_x, 2);
        assert_eq!(def.object, "arborreal_short_02");
    }

    #[test]
    fn parses_bracket_quoted_key() {
        let mut features = HashMap::new();
        parse_feature_lua(SAMPLE, &mut features);
        assert!(features.contains_key("tree type 1"));
        let def = &features["tree type 1"];
        assert_eq!(def.footprint_x, 3);
        assert_eq!(def.object, "tree_type1.s3o");
    }

    #[test]
    fn is_known_case_insensitive() {
        let mut features = HashMap::new();
        parse_feature_lua(SAMPLE, &mut features);
        let cat = FeatureCatalog { features };
        assert!(cat.is_known("arborreal_short_02"));
        assert!(cat.is_known("ARBORREAL_SHORT_02"));
        assert!(!cat.is_known("nonexistent_type"));
    }

    #[test]
    fn strip_comment_basic() {
        assert_eq!(strip_comment("foo = 1, -- comment"), "foo = 1, ");
        assert_eq!(strip_comment("no_comment"), "no_comment");
    }

    #[test]
    fn count_braces_ignores_strings() {
        let (o, c) = count_braces(r#"description = 'has { brace }',"#);
        assert_eq!(o, 0);
        assert_eq!(c, 0);
    }

    /// Regression test: previously the static parser's depth-1 pass extracted
    /// the nested `customparams` key as a fake feature, which then suppressed
    /// the mlua fallback and the for-loop-generated features (here: euro_birch_*)
    /// never got loaded. Verifies the fix in `parse_feature_lua` -- mlua runs
    /// first; the static parser only kicks in when mlua produced nothing.
    #[test]
    fn template_with_nested_customparams_loads_all_features() {
        let content = r#"
local Base = {
    description = "Birch tree",
    footprintx = 1,
    footprintz = 1,
    customparams = {
        author = "Nikuksis",
        treeshader = "yes",
    },
}

local trees = {}
for j = 1, 4 do
    for i = 1, 8 do
        local name = "euro_birch_tree_0" .. tostring(i) .. '_' .. tostring(j)
        local def = {}
        for k, v in pairs(Base) do
            def[k] = v
        end
        def.name = name
        def.object = name .. ".s3o"
        trees[name] = def
    end
end
return trees
"#;
        let mut features = HashMap::new();
        parse_feature_lua(content, &mut features);
        // 4 x 8 = 32 generated entries; "customparams" must NOT be one of them.
        assert_eq!(
            features.len(),
            32,
            "expected 32 euro_birch entries, got: {:?}",
            features.keys().collect::<Vec<_>>()
        );
        assert!(!features.contains_key("customparams"));
        assert!(features.contains_key("euro_birch_tree_01_1"));
        assert!(features.contains_key("euro_birch_tree_08_4"));
        let def = &features["euro_birch_tree_03_2"];
        assert_eq!(def.object, "euro_birch_tree_03_2.s3o");
        assert_eq!(def.footprint_x, 1);
    }
}

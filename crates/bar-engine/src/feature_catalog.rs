// SPDX-License-Identifier: GPL-2.0-or-later
//! BAR game archive feature catalog.
//!
//! Reads `gamedata/featuredata.lua` and `features/*.lua` from a BAR game
//! archive (.sdz = ZIP, .sdd = directory on disk) and builds a flat lookup
//! table keyed by lowercase feature type name.  Used by the 3D previewer to
//! decide tint color: green for known types, orange placeholder for unknown.

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
                if let Ok(file) = std::fs::File::open(archive) {
                    if let Ok(mut zip) = zip::ZipArchive::new(file) {
                        let mut lua_paths: Vec<String> = Vec::new();
                        for i in 0..zip.len() {
                            if let Ok(entry) = zip.by_index(i) {
                                if is_feature_lua(entry.name()) {
                                    lua_paths.push(entry.name().to_string());
                                }
                            }
                        }
                        for path in lua_paths {
                            if let Ok(mut entry) = zip.by_name(&path) {
                                let mut content = String::new();
                                let _ = entry.read_to_string(&mut content);
                                parse_feature_lua(&content, &mut catalog.features);
                            }
                        }
                    }
                }
            }
            "sdd" => {
                let paths = find_feature_lua_in_dir(archive);
                for path in paths {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        parse_feature_lua(&content, &mut catalog.features);
                    }
                }
            }
            _ => {}
        }

        catalog
    }
}

fn is_feature_lua(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "gamedata/featuredata.lua"
        || (lower.starts_with("features/") && lower.ends_with(".lua"))
}

fn find_feature_lua_in_dir(root: &Path) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    let featuredata = root.join("gamedata").join("featuredata.lua");
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

/// Extract a feature name from a line of the form `name = {` (depth-1 entry).
/// Handles bare identifiers (`armorplate = {`) and bracket-quoted keys
/// (`['Tree Type 1'] = {` or `["Rock"] = {`).
fn extract_feature_name(line: &str) -> Option<String> {
    let line = line.trim();
    let eq_pos = line.find('=')?;
    let key = line[..eq_pos].trim();
    let rest = line[eq_pos + 1..].trim();
    if !rest.starts_with('{') {
        return None;
    }
    if key.starts_with('[') && key.ends_with(']') {
        let inner = key[1..key.len() - 1].trim();
        if (inner.starts_with('\'') && inner.ends_with('\''))
            || (inner.starts_with('"') && inner.ends_with('"'))
        {
            return Some(inner[1..inner.len() - 1].to_lowercase());
        }
    } else if key.chars().all(|c| c.is_alphanumeric() || c == '_') && !key.is_empty() {
        return Some(key.to_lowercase());
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
/// Uses a depth-tracking state machine; nested tables inside features are skipped.
fn parse_feature_lua(content: &str, features: &mut HashMap<String, FeatureDef>) {
    let mut depth: i32 = 0;
    let mut current_name: Option<String> = None;
    let mut partial = PartialDef::default();

    for line in content.lines() {
        let stripped = strip_comment(line);
        let (open, close) = count_braces(stripped);
        let net = open - close;
        let prev_depth = depth;
        depth += net;

        // Entering a feature definition (depth 1 -> 2)
        if prev_depth == 1 && depth >= 2 {
            current_name = extract_feature_name(stripped);
            partial = PartialDef::default();
        }

        // Collecting fields while inside a feature definition at depth 2
        if prev_depth == 2 && depth == 2 {
            if let Some(v) = extract_string_field(stripped, "object") {
                partial.object = v.to_string();
            } else if let Some(v) = extract_u32_field(stripped, "footprintX") {
                partial.footprint_x = v;
            } else if let Some(v) = extract_u32_field(stripped, "footprintZ") {
                partial.footprint_z = v;
            }
        }

        // Closing a feature definition (depth 2 -> 1)
        if prev_depth >= 2 && depth <= 1 {
            if let Some(name) = current_name.take() {
                features.insert(
                    name.clone(),
                    FeatureDef {
                        name,
                        object: std::mem::take(&mut partial.object),
                        footprint_x: partial.footprint_x,
                        footprint_z: partial.footprint_z,
                    },
                );
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
}

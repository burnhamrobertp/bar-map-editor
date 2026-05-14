//! .sd7 map import — extracts baked heightmap data from a Spring .sd7 archive
//! and generates an BAR map editor project file referencing it as a FileInput node.
//!
//! Only the heightmap is imported; metalmap/typemap are skipped because they are
//! at half resolution (map_x/2) and FileInput resamples to output dimensions
//! (map_x+1), which would corrupt categorical typemap data via interpolation.

use std::collections::HashMap;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use image::{ImageBuffer, Luma};
use path_slash::PathExt as _;

use bar_graph::{NodeType, ParamValue};

use crate::project::{EditorLayout, Position, Project};
use crate::recipe::{
    MapSettings, OutputConfig, PlacedFeature, Recipe, RecipeConnection, RecipeNode,
};

/// Result of importing a .sd7 archive.
pub struct ImportResult {
    /// Map name (from mapinfo.lua or SMF filename stem as fallback).
    pub map_name: String,
    /// Heightmap width in samples (map_x + 1).
    pub width: u32,
    /// Heightmap height in samples (map_y + 1).
    pub height: u32,
    /// Minimum height in Spring world units (from SMF header).
    pub min_height: f32,
    /// Maximum height in Spring world units (from SMF header).
    pub max_height: f32,
    /// Path to the written 16-bit grayscale heightmap PNG.
    pub heightmap_png: PathBuf,
    /// Raw mapinfo.lua contents from the archive, if present. Lets
    /// `import_sd7_to_project` extract additional MapSettings fields
    /// (gravity, tidal_strength, etc.) without re-reading the archive.
    pub mapinfo_lua: Option<String>,
    /// Feature placements extracted from the SMF feature section.
    pub features: Vec<PlacedFeature>,
}

/// Extract baked heightmap data from a `.sd7` archive into `output_dir`.
///
/// Writes `heightmap.png` (16-bit grayscale) and returns metadata parsed from
/// the SMF header. The caller is responsible for writing the `.barproj` file.
pub fn import_sd7(archive_path: &Path, output_dir: &Path) -> Result<ImportResult> {
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output dir: {}", output_dir.display()))?;

    // Use a timestamped temp dir to avoid stale file collisions across retries
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let extract_dir = output_dir.join(format!(".extracted_{ts}"));
    std::fs::create_dir_all(&extract_dir)?;

    let result = do_import(archive_path, output_dir, &extract_dir);
    let _ = std::fs::remove_dir_all(&extract_dir);
    result
}

/// Import a `.sd7` archive and produce a ready-to-save [`Project`].
///
/// Writes `heightmap.png` to `output_dir`. The returned `Project` contains a
/// `FileInput` → `Bundler` pipeline referencing that PNG via an
/// absolute path.
pub fn import_sd7_to_project(archive_path: &Path, output_dir: &Path) -> Result<Project> {
    let result = import_sd7(archive_path, output_dir)?;

    // Normalise to forward-slash for cross-platform recipe portability
    let hm_path_str = result.heightmap_png.to_slash_lossy().to_string();

    let nodes = vec![
        RecipeNode {
            key: "hm_input".to_string(),
            node_type: NodeType::FileInput,
            label: format!("{} Heightmap", result.map_name),
            params: {
                let mut p = HashMap::new();
                p.insert("path".to_string(), ParamValue::String(hm_path_str));
                p
            },
        },
        RecipeNode {
            key: "bundler".to_string(),
            node_type: NodeType::Bundler,
            label: "Export".to_string(),
            params: HashMap::new(),
        },
    ];

    let connections = vec![RecipeConnection {
        from: "hm_input.output".to_string(),
        to: "bundler.heightmap".to_string(),
    }];

    // Pull additional MapSettings fields from the imported mapinfo.lua
    // when they're available. Fields that aren't found in the file fall
    // back to MapSettings::default() — same as a freshly-created project.
    let map_settings = MapSettings {
        min_height: result.min_height,
        max_height: result.max_height,
        gravity: result
            .mapinfo_lua
            .as_deref()
            .and_then(|l| parse_mapinfo_number(l, "gravity"))
            .unwrap_or(MapSettings::default().gravity),
        tidal_strength: result
            .mapinfo_lua
            .as_deref()
            .and_then(|l| parse_mapinfo_number(l, "tidalStrength"))
            .unwrap_or(MapSettings::default().tidal_strength),
        max_metal: result
            .mapinfo_lua
            .as_deref()
            .and_then(|l| parse_mapinfo_number(l, "maxMetal"))
            .unwrap_or(MapSettings::default().max_metal),
        extractor_radius: result
            .mapinfo_lua
            .as_deref()
            .and_then(|l| parse_mapinfo_number(l, "extractorRadius"))
            .unwrap_or(MapSettings::default().extractor_radius),
        map_hardness: result
            .mapinfo_lua
            .as_deref()
            .and_then(|l| parse_mapinfo_number(l, "mapHardness"))
            .map(|v| v as u32)
            .unwrap_or(MapSettings::default().map_hardness),
        ..MapSettings::default()
    };

    let recipe = Recipe {
        schema_version: bar_project::RECIPE_SCHEMA_VERSION,
        name: result.map_name.clone(),
        shortname: None,
        description: format!(
            "Imported from .sd7: {}",
            archive_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ),
        author: None,
        version: None,
        nodes,
        connections,
        output: OutputConfig {
            width: result.width,
            height: result.height,
            map_settings,
        },
        features: result.features,
        source_sd7: None,
    };

    let layout = EditorLayout {
        node_positions: {
            let mut m = HashMap::new();
            m.insert("hm_input".to_string(), Position { x: 100.0, y: 200.0 });
            m.insert("bundler".to_string(), Position { x: 420.0, y: 200.0 });
            m
        },
        node_sizes: HashMap::new(),
        canvas_offset: (0.0, 0.0),
        map_info_file: None,
        groups: Vec::new(),
        open_tabs: Vec::new(),
        active_tab: 0,
    };

    Ok(Project { recipe, layout })
}

// ── Internal helpers ────────────────────────────────────────────────────────

fn do_import(archive_path: &Path, output_dir: &Path, extract_dir: &Path) -> Result<ImportResult> {
    sevenz_rust::decompress_file(archive_path, extract_dir)
        .with_context(|| format!("Failed to extract archive: {}", archive_path.display()))?;

    let maps_dir = extract_dir.join("maps");
    let smf_path = find_single_smf(&maps_dir)?;

    let smf_stem = smf_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "imported_map".to_string());

    let mapinfo_path = extract_dir.join("mapinfo.lua");
    let mapinfo_lua: Option<String> = if mapinfo_path.exists() {
        std::fs::read_to_string(&mapinfo_path).ok()
    } else {
        None
    };
    let map_name = mapinfo_lua
        .as_deref()
        .and_then(parse_mapinfo_name)
        .unwrap_or(smf_stem);

    let smf_file = std::fs::File::open(&smf_path)
        .with_context(|| format!("Cannot open SMF: {}", smf_path.display()))?;
    let smf = bar_data::sd7::SmfMap::read(&mut BufReader::new(smf_file))
        .with_context(|| format!("Failed to parse SMF: {}", smf_path.display()))?;

    let (width, height) = smf.header.heightmap_size();

    let heightmap_png = output_dir.join("heightmap.png");
    write_heightmap_png(&smf.heightmap, &heightmap_png)?;

    // Spring/BAR's mapinfo.lua may override the SMF header's height range.
    // Use the override when present so the imported project matches the
    // engine's interpretation of the map.
    let (min_height, max_height) = mapinfo_lua
        .as_deref()
        .and_then(parse_mapinfo_smf_heights)
        .unwrap_or((smf.header.min_height, smf.header.max_height));

    let features = smf
        .features
        .iter()
        .map(|f| PlacedFeature {
            feature_type: f.feature_type.clone(),
            x: f.x,
            y: f.y,
            z: f.z,
            angle: f.angle,
            taken_damage: f.taken_damage,
        })
        .collect();

    Ok(ImportResult {
        map_name,
        width,
        height,
        min_height,
        max_height,
        heightmap_png,
        mapinfo_lua,
        features,
    })
}

fn find_single_smf(maps_dir: &Path) -> Result<PathBuf> {
    if !maps_dir.exists() {
        bail!(
            "No 'maps/' directory found in archive (expected at: {})",
            maps_dir.display()
        );
    }
    let smf_files: Vec<PathBuf> = std::fs::read_dir(maps_dir)
        .context("Cannot read maps/ directory")?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("smf"))
        .collect();

    match smf_files.len() {
        0 => bail!("No .smf file found in maps/ directory"),
        1 => Ok(smf_files.into_iter().next().unwrap()),
        n => bail!("Found {n} .smf files in maps/ directory; expected exactly 1"),
    }
}

fn write_heightmap_png(heightmap: &bar_data::Heightmap, path: &Path) -> Result<()> {
    let w = heightmap.width();
    let h = heightmap.height();
    let u16_data = heightmap.to_u16();
    let img: ImageBuffer<Luma<u16>, Vec<u16>> =
        ImageBuffer::from_raw(w, h, u16_data).context("Failed to construct image buffer")?;
    img.save(path)
        .with_context(|| format!("Failed to write heightmap PNG: {}", path.display()))
}

/// Parse the `smf.minheight` / `smf.maxheight` overrides from `mapinfo.lua`.
///
/// Spring/BAR uses these (when present) to reinterpret the heightmap u16
/// values. They override whatever's in the SMF binary header. Many maps
/// allocate generous header headroom (e.g. `[-50, 100]`) but specify the
/// real working range here (e.g. `[-250, 670]`) — using the binary header
/// alone produces flat-looking previews that don't match the engine.
///
/// Looks for `minheight = N` and `maxheight = N` (numeric values) anywhere
/// inside an `smf = { ... }` block. Returns `Some((min, max))` only when
/// both are found; `None` if the block is missing or malformed.
pub fn parse_mapinfo_smf_heights(lua: &str) -> Option<(f32, f32)> {
    // Find the `smf = {` block (case-insensitive). Match the full
    // `smf <whitespace>* = <whitespace>* {` shape so we don't trip over
    // earlier occurrences of "smf" inside comments (e.g. "// .smf file")
    // or in unrelated identifiers.
    let lower = lua.to_lowercase();
    let mut search_from = 0usize;
    let (smf_idx, brace_open) = loop {
        let rel = lower[search_from..].find("smf")?;
        let abs = search_from + rel;
        // Look at what follows: must be optional whitespace, then '=',
        // optional whitespace, then '{'. Anything else and we keep searching.
        let after = &lua[abs + 3..];
        let mut chars = after.char_indices().peekable();
        let mut saw_eq = false;
        let mut found_brace: Option<usize> = None;
        while let Some(&(i, c)) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
                continue;
            }
            if !saw_eq {
                if c == '=' {
                    saw_eq = true;
                    chars.next();
                    continue;
                }
                break;
            }
            // After '=' — must be a brace.
            if c == '{' {
                found_brace = Some(abs + 3 + i);
            }
            break;
        }
        if let Some(bo) = found_brace {
            break (abs, bo);
        }
        search_from = abs + 3;
    };
    let _ = smf_idx; // documents intent; not used below.
    let after_smf = lua; // the absolute index `brace_open` references the original string.
                         // Track depth so a sub-table inside smf doesn't fool us. Find the
                         // matching closing brace.
    let mut depth = 0;
    let mut end = brace_open;
    for (i, c) in after_smf[brace_open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = brace_open + i;
                    break;
                }
            }
            _ => {}
        }
    }
    // body starts at the opening `{` and ends just before the matching `}`.
    // Strip the leading brace so the first piece on a single-line table
    // (`{ minheight = 0, … }`) doesn't carry it into key matching.
    let body = after_smf[brace_open + 1..end].trim();

    let parse_field = |key: &str| -> Option<f32> {
        // Split on both newlines AND commas so we can parse both
        // multi-line and inline `{ a = 1, b = 2 }` forms.
        for piece in body.split(['\n', ',']) {
            let trimmed = piece.trim().to_lowercase();
            // Match `<key> = ` or `<key>=`. Comments and trailing chars are
            // tolerated; we just want the first numeric token after `=`.
            if let Some(rest) = trimmed.strip_prefix(key).map(str::trim_start) {
                if let Some(after_eq) = rest.strip_prefix('=').map(str::trim_start) {
                    // Take a numeric prefix (signed float, integer, or scientific).
                    let mut end = 0;
                    for (i, c) in after_eq.char_indices() {
                        if c.is_ascii_digit()
                            || c == '-'
                            || c == '+'
                            || c == '.'
                            || c == 'e'
                            || c == 'E'
                        {
                            end = i + c.len_utf8();
                        } else {
                            break;
                        }
                    }
                    if end > 0 {
                        if let Ok(v) = after_eq[..end].parse::<f32>() {
                            return Some(v);
                        }
                    }
                }
            }
        }
        None
    };

    let min = parse_field("minheight")?;
    let max = parse_field("maxheight")?;
    Some((min, max))
}

/// Parse a top-level numeric field from `mapinfo.lua` — e.g. `gravity = 130`,
/// `tidalStrength = 18`, `mapHardness = 100`. Returns `None` if not found
/// or unparseable. Used by the SD7 importer to populate MapSettings beyond
/// just heights.
pub fn parse_mapinfo_number(lua: &str, key: &str) -> Option<f32> {
    let pat = format!("{}=", key);
    for line in lua.lines() {
        let trimmed = line.trim();
        // Match `<key> =` ignoring whitespace around the equals
        let stripped = trimmed.strip_prefix(key).map(|rest| rest.trim_start());
        let rest = match stripped {
            Some(r) if r.starts_with('=') => &r[1..],
            _ => {
                // Try the no-space form too
                if !trimmed.starts_with(&pat) {
                    continue;
                }
                &trimmed[pat.len()..]
            }
        };
        let value = rest
            .trim()
            .trim_end_matches(',')
            .trim_end_matches(';')
            .trim();
        if let Ok(v) = value.parse::<f32>() {
            return Some(v);
        }
    }
    None
}

/// Parse the map `name` field from `mapinfo.lua`.
///
/// Looks for a line matching `name = "..."` or `name = '...'` at any
/// indentation level. Uses the SMF filename stem if not found.
fn parse_mapinfo_name(lua: &str) -> Option<String> {
    for line in lua.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("name") {
            continue;
        }
        let eq_pos = trimmed.find('=')?;
        let after = trimmed[eq_pos + 1..].trim();
        for &q in &['"', '\''] {
            if after.starts_with(q) {
                if let Some(end) = after[1..].find(q) {
                    let name = &after[1..=end];
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
        }
    }
    None
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mapinfo_smf_heights_basic() {
        let lua = r#"
local mapinfo = {
    name = "Test",
    smf = {
        minheight = -250,
        maxheight = 670,
        smtFileName0 = "maps/test.smt",
    },
}
"#;
        assert_eq!(parse_mapinfo_smf_heights(lua), Some((-250.0, 670.0)));
    }

    #[test]
    fn parse_mapinfo_smf_heights_negative_zero() {
        let lua = "smf = { minheight = 0, maxheight = 1024.5 }";
        assert_eq!(parse_mapinfo_smf_heights(lua), Some((0.0, 1024.5)));
    }

    #[test]
    fn parse_mapinfo_smf_heights_missing_returns_none() {
        let lua = "smf = { smtFileName0 = \"foo.smt\" }";
        assert_eq!(parse_mapinfo_smf_heights(lua), None);
    }

    #[test]
    fn parse_mapinfo_smf_heights_no_smf_block() {
        let lua = "name = \"foo\"";
        assert_eq!(parse_mapinfo_smf_heights(lua), None);
    }

    #[test]
    fn parse_mapinfo_smf_heights_skips_smf_in_comment() {
        // Regression: kolmogorov_remake.mapinfo.lua had `smf` first appear
        // in a comment ("// location of smf/sm3 file") followed by an
        // unrelated `depend = {}` table — earlier the parser would lock
        // onto that empty table and miss the real smf block.
        let lua = r#"
local mapinfo = {
    name = "Kolmog",
    --mapfile = "", --// location of smf/sm3 file (optional)
    depend  = {},
    replace = {},

    smf = {
        minheight = -250,
        maxheight = 670,
    },
}
"#;
        assert_eq!(parse_mapinfo_smf_heights(lua), Some((-250.0, 670.0)));
    }

    #[test]
    fn parse_name_double_quotes() {
        let lua = r#"
local mapinfo = {
    name = "Open Plains v2",
    author = "Somebody",
}
"#;
        assert_eq!(parse_mapinfo_name(lua), Some("Open Plains v2".to_string()));
    }

    #[test]
    fn parse_name_single_quotes() {
        let lua = "name = 'rocky_hills'";
        assert_eq!(parse_mapinfo_name(lua), Some("rocky_hills".to_string()));
    }

    #[test]
    fn parse_name_with_indentation() {
        let lua = "    name = \"Indented Map Name\",";
        assert_eq!(
            parse_mapinfo_name(lua),
            Some("Indented Map Name".to_string())
        );
    }

    #[test]
    fn parse_name_missing_returns_none() {
        let lua = "author = \"nobody\"";
        assert_eq!(parse_mapinfo_name(lua), None);
    }

    #[test]
    fn find_single_smf_no_dir() {
        let result = find_single_smf(Path::new("/nonexistent/maps"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No 'maps/'"));
    }

    #[test]
    fn find_single_smf_empty_dir() {
        let dir = std::env::temp_dir().join("om_importer_test_empty_maps");
        std::fs::create_dir_all(&dir).unwrap();
        let result = find_single_smf(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No .smf"));
    }

    #[test]
    fn find_single_smf_multiple_error() {
        let dir = std::env::temp_dir().join("om_importer_test_multi_maps");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.smf"), b"").unwrap();
        std::fs::write(dir.join("b.smf"), b"").unwrap();
        let result = find_single_smf(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("2 .smf files"));
    }

    #[test]
    fn find_single_smf_success() {
        let dir = std::env::temp_dir().join("om_importer_test_ok_maps");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mymap.smf"), b"").unwrap();
        let result = find_single_smf(&dir);
        std::fs::remove_dir_all(&dir).ok();
        let path = result.unwrap();
        assert_eq!(path.file_name().unwrap(), "mymap.smf");
    }

    #[test]
    fn import_sd7_to_project_preserves_features() {
        use bar_data::sd7::{SmfFeaturePlacement, SmfMap};

        let tmp = std::env::temp_dir().join("om_importer_features_test");
        std::fs::remove_dir_all(&tmp).ok();
        let maps_dir = tmp.join("src").join("maps");
        std::fs::create_dir_all(&maps_dir).unwrap();

        let mut smf = SmfMap::new(128, 128).unwrap();
        smf.features = vec![
            SmfFeaturePlacement {
                feature_type: "arborreal".to_string(),
                x: 512.0,
                y: 0.0,
                z: 256.0,
                angle: 1.57,
                taken_damage: 0,
            },
            SmfFeaturePlacement {
                feature_type: "GeoTherm_Lava_Rock".to_string(),
                x: 100.0,
                y: 0.0,
                z: 200.0,
                angle: 0.0,
                taken_damage: 5,
            },
        ];

        let smf_path = maps_dir.join("test.smf");
        let mut smf_file = std::fs::File::create(&smf_path).unwrap();
        smf.write(&mut smf_file).unwrap();

        let sd7_path = tmp.join("test.sd7");
        sevenz_rust::compress_to_path(tmp.join("src"), &sd7_path).unwrap();

        let out_dir = tmp.join("out");
        let project = import_sd7_to_project(&sd7_path, &out_dir).unwrap();

        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(project.recipe.features.len(), 2);
        let arb = project
            .recipe
            .features
            .iter()
            .find(|f| f.feature_type == "arborreal")
            .expect("arborreal feature missing");
        assert!((arb.x - 512.0).abs() < 0.001);
        assert!((arb.z - 256.0).abs() < 0.001);
        let geo = project
            .recipe
            .features
            .iter()
            .find(|f| f.feature_type == "GeoTherm_Lava_Rock")
            .expect("GeoTherm_Lava_Rock feature missing");
        assert_eq!(geo.taken_damage, 5);
    }
}

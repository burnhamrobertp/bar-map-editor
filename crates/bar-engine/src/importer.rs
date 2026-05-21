//! .sd7 map import: thin wrapper that extracts an archive to a
//! caller-chosen destination, scans the contents, and runs them
//! through `bar_project::scan_to_project` to produce a [`Project`].
//!
//! Both the GUI's "Import .sd7" flow and the CLI's `bar-cli import`
//! command pass through [`import_sd7_to_project`] so identity-field
//! parsing, mapinfo merging, FeatureSource tagging, grassmap
//! fallback, etc. all live in exactly one place. Earlier this file
//! held a parallel importer that built its own minimal recipe; that
//! caused `Edit Map Info > Identity` to come up blank for GUI imports
//! because the two paths drifted apart on which mapinfo fields they
//! parsed.

use std::path::Path;

use anyhow::{Context, Result};

use crate::project::Project;

// Re-export the mapinfo.lua parsers so callers of this crate can use
// them without depending on `bar_project` directly. Canonical
// implementations live in `bar_project::mapinfo`.
pub use bar_project::{parse_mapinfo_number, parse_mapinfo_smf_heights, parse_mapinfo_vec3};

/// Import a `.sd7` archive and produce a ready-to-save [`Project`]
/// alongside any pending binary assets / raw files the scan turned
/// up.
///
/// `dest_dir` is the user-chosen `.barproj` directory the archive
/// extracts into. Caller is responsible for writing `recipe.json` /
/// `layout.json` (via `Project::save`) and the returned pending
/// assets / raw files into `<dest_dir>/assets/`. The directory is
/// created if missing; if it already contains files, extraction is
/// skipped so prior edits are preserved.
pub fn import_sd7_to_project(
    archive_path: &Path,
    dest_dir: &Path,
) -> Result<(
    Project,
    Vec<bar_project::PendingAsset>,
    Vec<bar_project::PendingRawFile>,
)> {
    let scan = crate::extract::extract_sd7_to_dir_with_progress(archive_path, dest_dir, &|_| {})
        .with_context(|| format!("Failed to extract: {}", archive_path.display()))?;
    let (project, assets, raw_files) = bar_project::scan_to_project(&scan);
    Ok((project, assets, raw_files))
}

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
        let (project, _assets, _raw_files) = import_sd7_to_project(&sd7_path, &out_dir).unwrap();

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

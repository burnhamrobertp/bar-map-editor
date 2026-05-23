//! Filesystem helpers shared between the project layer and its
//! consumers. Currently a single utility: case-insensitive recursive
//! lookup for files referenced by mapinfo bare-filename (skybox,
//! grassShadingTex, splat textures, etc.). The engine resolves these
//! via VFS, which is case-insensitive on Windows builds and lenient
//! about subdirectories; reproducing that here means previews and
//! renderers agree on which file backs a given mapinfo string.

use std::path::{Path, PathBuf};

/// Walk `dir` recursively for a file whose basename matches `name`
/// (case-insensitive). Returns the first hit. `None` when `dir` is
/// not a directory or no match is found.
///
/// `name` is treated as a path-or-bare-filename -- any directory
/// prefix is stripped before comparison. Mapinfo strings commonly
/// include a `maps/` prefix (`"maps/Onyx Cauldron 2.0_grassDist.tga"`,
/// `"maps/foo.dds"`) that BAR's VFS treats as a hint inside the map
/// archive; the actual on-disk extraction lands the file flat under
/// the project's passthrough tree. Stripping the prefix here lets
/// both forms resolve the same way without callers having to
/// pre-process.
pub fn find_file_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    // Strip any path prefix so a mapinfo string like
    // "maps/foo.dds" matches against a basename of "foo.dds".
    let basename = Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(name);
    let needle = basename.to_ascii_lowercase();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_ascii_lowercase() == needle)
                .unwrap_or(false)
            {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mapinfo string like `"maps/file.tga"` should resolve against
    /// a file living anywhere in the project tree (including a
    /// sibling `maps/` subdirectory or directly under the root).
    /// Regression test for the grass-widget asset lookup that
    /// silently failed when the path prefix wasn't stripped.
    #[test]
    fn strips_path_prefix_when_matching() {
        let tmp = std::env::temp_dir().join("bme-fs-util-prefix-test");
        let _ = std::fs::remove_dir_all(&tmp);
        let maps = tmp.join("maps");
        std::fs::create_dir_all(&maps).unwrap();
        let target = maps.join("MyMap_grassDist.tga");
        std::fs::write(&target, b"x").unwrap();
        let hit = find_file_in_dir(&tmp, "maps/MyMap_grassDist.tga");
        assert_eq!(hit.as_deref(), Some(target.as_path()));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Case-insensitivity (mapinfo strings often differ in case from
    /// the archive's actual filenames; BAR's VFS papers over this).
    #[test]
    fn case_insensitive_basename_match() {
        let tmp = std::env::temp_dir().join("bme-fs-util-case-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let target = tmp.join("Foam.JPG");
        std::fs::write(&target, b"x").unwrap();
        let hit = find_file_in_dir(&tmp, "foam.jpg");
        assert_eq!(hit.as_deref(), Some(target.as_path()));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

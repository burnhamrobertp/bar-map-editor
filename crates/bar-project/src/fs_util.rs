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
pub fn find_file_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    let needle = name.to_ascii_lowercase();
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

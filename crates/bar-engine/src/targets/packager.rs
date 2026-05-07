//! Packager implementations: archive/bundle output files.
//!
//! A Packager takes a staging directory of exported files and produces
//! a final artifact (7z archive, zip archive, or plain directory copy).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use path_slash::PathExt as _;

use super::packaging::FileMapping;

/// Trait for packaging exported files into a deliverable artifact.
pub trait Packager: Send + Sync {
    /// Package the files in `source_dir` into `output_path`.
    ///
    /// The `layout` mappings describe which files in source_dir map to
    /// which paths in the final archive. If layout is empty, all files
    /// in source_dir are included at their relative paths.
    fn package(
        &self,
        source_dir: &Path,
        output_path: &Path,
        layout: &[FileMapping],
    ) -> Result<()>;

    /// Human-readable name for this packager.
    fn name(&self) -> &str;
}

/// Packages files into a 7-Zip archive (.sd7, .7z).
/// Shells out to the system `7z` executable.
pub struct SevenZipPackager {
    /// Path to the 7z executable. If None, searches PATH.
    pub executable: Option<PathBuf>,
}

impl SevenZipPackager {
    pub fn new() -> Self {
        Self { executable: None }
    }

    pub fn with_executable(path: impl Into<PathBuf>) -> Self {
        Self {
            executable: Some(path.into()),
        }
    }

    fn find_executable(&self) -> PathBuf {
        if let Some(ref path) = self.executable {
            return path.clone();
        }
        // Common locations on Windows
        let candidates = [
            r"C:\Program Files\7-Zip\7z.exe",
            r"C:\Program Files (x86)\7-Zip\7z.exe",
        ];
        for candidate in &candidates {
            if Path::new(candidate).exists() {
                return PathBuf::from(candidate);
            }
        }
        // Fall back to hoping it's on PATH
        PathBuf::from("7z")
    }
}

impl Default for SevenZipPackager {
    fn default() -> Self {
        Self::new()
    }
}

impl Packager for SevenZipPackager {
    fn name(&self) -> &str {
        "7-Zip"
    }

    fn package(
        &self,
        source_dir: &Path,
        output_path: &Path,
        _layout: &[FileMapping],
    ) -> Result<()> {
        let exe = self.find_executable();

        // Remove existing archive if present
        if output_path.exists() {
            std::fs::remove_file(output_path)
                .with_context(|| format!("Failed to remove existing archive: {}", output_path.display()))?;
        }

        // Create parent directory
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Build file list from source_dir (all files recursively)
        let output = Command::new(&exe)
            .arg("a")
            .arg("-t7z")
            .arg(output_path)
            .arg(format!("{}\\*", source_dir.display()))
            .arg("-r")
            .output()
            .with_context(|| format!("Failed to execute 7z at: {}", exe.display()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            anyhow::bail!(
                "7z failed with status {}:\nstdout: {}\nstderr: {}",
                output.status,
                stdout,
                stderr
            );
        }

        Ok(())
    }
}

/// Packages files into a ZIP archive.
pub struct ZipPackager;

impl Packager for ZipPackager {
    fn name(&self) -> &str {
        "ZIP"
    }

    fn package(
        &self,
        source_dir: &Path,
        output_path: &Path,
        _layout: &[FileMapping],
    ) -> Result<()> {
        use std::io::Write;

        // Remove existing archive if present
        if output_path.exists() {
            std::fs::remove_file(output_path)?;
        }

        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = std::fs::File::create(output_path)
            .with_context(|| format!("Failed to create zip: {}", output_path.display()))?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        // Walk source_dir recursively
        for entry in walkdir::WalkDir::new(source_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() {
                let relative = path
                    .strip_prefix(source_dir)
                    .unwrap_or(path);
                let name = relative.to_slash_lossy();
                zip.start_file(name.as_ref(), options)
                    .with_context(|| format!("Failed to add {} to zip", name))?;
                let data = std::fs::read(path)?;
                zip.write_all(&data)?;
            }
        }

        zip.finish()?;
        Ok(())
    }
}

/// "Packages" files by copying them to an output directory (no archiving).
pub struct DirectoryPackager;

impl Packager for DirectoryPackager {
    fn name(&self) -> &str {
        "Directory"
    }

    fn package(
        &self,
        source_dir: &Path,
        output_path: &Path,
        _layout: &[FileMapping],
    ) -> Result<()> {
        // Create the output directory
        std::fs::create_dir_all(output_path)
            .with_context(|| format!("Failed to create output directory: {}", output_path.display()))?;

        // Copy all files recursively
        copy_dir_recursive(source_dir, output_path)
            .with_context(|| format!("Failed to copy files to {}", output_path.display()))?;

        Ok(())
    }
}

/// Recursively copy a directory's contents.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest_path = dst.join(entry.file_name());

        if path.is_dir() {
            std::fs::create_dir_all(&dest_path)?;
            copy_dir_recursive(&path, &dest_path)?;
        } else {
            std::fs::copy(&path, &dest_path)?;
        }
    }
    Ok(())
}

/// Validate a bundle path to prevent path traversal attacks.
/// Bundle paths use forward slashes (archive-portable format).
pub fn validate_bundle_path(bundle_path: &str) -> Result<()> {
    if bundle_path.is_empty() {
        anyhow::bail!("Bundle path cannot be empty");
    }
    // Reject any absolute path indicators (Unix or Windows)
    if bundle_path.starts_with('/') || bundle_path.starts_with('\\') {
        anyhow::bail!("Bundle path must be relative: {}", bundle_path);
    }
    // Windows drive letter (e.g., C:\...)
    if bundle_path.len() >= 2 && bundle_path.as_bytes()[1] == b':' {
        anyhow::bail!("Bundle path must be relative: {}", bundle_path);
    }
    // Path traversal
    if bundle_path.contains("..") {
        anyhow::bail!("Bundle path cannot contain '..': {}", bundle_path);
    }
    // Backslashes should not appear (use forward slashes for portability)
    if bundle_path.contains('\\') {
        anyhow::bail!("Bundle path must use forward slashes: {}", bundle_path);
    }
    Ok(())
}

/// Create a packager instance from an archive format.
pub fn create_packager(format: &super::packaging::ArchiveFormat) -> Box<dyn Packager> {
    match format {
        super::packaging::ArchiveFormat::SevenZip => Box::new(SevenZipPackager::new()),
        super::packaging::ArchiveFormat::Zip => Box::new(ZipPackager),
        super::packaging::ArchiveFormat::Directory => Box::new(DirectoryPackager),
    }
}

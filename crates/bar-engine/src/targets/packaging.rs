//! Packaging configuration for export targets.

/// Archive format for the final output.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveFormat {
    /// 7-Zip archive (.sd7 for Spring).
    SevenZip,
    /// Standard ZIP archive.
    Zip,
    /// Plain directory (no archiving).
    Directory,
}

/// A mapping from a logical output to a file path within the archive.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileMapping {
    /// Logical source (e.g., "smf", "smt", "metadata", "heightmap_png").
    pub source: String,
    /// Destination path pattern within the archive.
    /// Supports `{name}` placeholder for map name.
    pub dest: String,
}

impl FileMapping {
    /// Resolve the destination path with a given map name.
    pub fn resolve_dest(&self, map_name: &str) -> String {
        self.dest.replace("{name}", map_name)
    }
}

/// Packaging configuration: how exported files are assembled into the final output.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PackagingConfig {
    /// Archive format to use.
    pub archive_format: ArchiveFormat,
    /// File extension for the final output (e.g., ".sd7").
    pub extension: String,
    /// File mappings within the archive.
    pub layout: Vec<FileMapping>,
}

impl Default for PackagingConfig {
    fn default() -> Self {
        Self {
            archive_format: ArchiveFormat::Directory,
            extension: String::new(),
            layout: Vec::new(),
        }
    }
}

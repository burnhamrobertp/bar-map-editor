use serde::{Deserialize, Serialize};

/// Unique identifier for a port (node_id + port_name).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PortId {
    pub node_id: super::NodeId,
    pub port_name: String,
}

/// The data type a port carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortKind {
    /// 2D heightmap (f32 buffer)
    Heightmap,
    /// Mask/selection (f32 buffer, 0-1)
    Mask,
    /// RGBA color/texture data
    Color,
    /// Scalar float value
    Scalar,
    /// External file reference (path + bundle destination)
    File,
    /// A bag of external file references (used by PassThrough nodes)
    FileList,
}

impl PortKind {
    /// Parse from the human-readable name used in `param_choices`
    /// (e.g. SubgraphInput's `kind` param). Returns `None` for
    /// unknown strings rather than panicking.
    pub fn parse_name(s: &str) -> Option<Self> {
        match s {
            "Heightmap" => Some(PortKind::Heightmap),
            "Mask" => Some(PortKind::Mask),
            "Color" => Some(PortKind::Color),
            "Scalar" => Some(PortKind::Scalar),
            "File" => Some(PortKind::File),
            "FileList" => Some(PortKind::FileList),
            _ => None,
        }
    }
}

/// How many connections an input port accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PortCardinality {
    /// Exactly one connection (default for most ports).
    #[default]
    One,
    /// Multiple connections allowed (e.g., Bundler.files).
    Many,
}

/// A port definition on a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Port {
    pub name: String,
    pub label: String,
    pub kind: PortKind,
    #[serde(default)]
    pub cardinality: PortCardinality,
}

impl Port {
    pub fn new(name: impl Into<String>, label: impl Into<String>, kind: PortKind) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind,
            cardinality: PortCardinality::One,
        }
    }

    pub fn new_many(name: impl Into<String>, label: impl Into<String>, kind: PortKind) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind,
            cardinality: PortCardinality::Many,
        }
    }
}

/// A reference to an external file to be included in a bundle.
#[derive(Debug, Clone)]
pub struct FileRef {
    /// Path to the source file (project-relative).
    pub path: String,
    /// Destination path within the archive/bundle.
    pub bundle_path: String,
}

/// Runtime value that flows between ports during graph evaluation.
#[derive(Debug, Clone)]
pub enum PortValue {
    Heightmap(bar_data::Heightmap),
    Mask(bar_data::Heightmap),
    Color(bar_data::ColorBuffer),
    Scalar(f32),
    File(FileRef),
    /// A bag of external file references (produced by PassThrough nodes).
    FileList(Vec<FileRef>),
    Empty,
}

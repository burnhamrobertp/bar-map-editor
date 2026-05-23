//! Export target system: codec-parameterized export configuration.
//!
//! An export target defines how terrain data is written to a specific format.
//! It consists of:
//! - A **codec** (Rust implementation) that handles binary format writing
//! - A **config** that parameterizes the codec (dimensions, layers, packaging)
//!
//! Built-in targets ship compiled into the binary. Projects can reference
//! custom targets via TOML files.

mod codec;
mod config;
mod dimensions;
mod layers;
mod lua_table;
mod packager;
mod packaging;
mod raw_layers;
mod registry;
pub(crate) mod spring_smf;
mod target_io;
mod validation;

pub use codec::{ExportCodec, ExportPlan, WrittenFiles};
pub use config::TargetConfig;
pub use dimensions::{DimensionBase, DimensionConstraint, DimensionRule, DimensionSet};
pub use layers::{LayerFormat, LayerRequirement, LayerSet, LayerStatus};
pub use packager::{
    create_packager, validate_bundle_path, DirectoryPackager, Packager, SevenZipPackager,
    ZipPackager,
};
pub use packaging::{ArchiveFormat, FileMapping, PackagingConfig};
pub use raw_layers::RawLayersCodec;
pub use registry::TargetRegistry;
pub use spring_smf::SpringSmfCodec;
pub use target_io::{
    load_target_config, parse_target_toml, save_target_config, serialize_target_toml,
};
pub use validation::{Severity, ValidationError};

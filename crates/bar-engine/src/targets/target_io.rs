//! Target file I/O: loading and saving target configurations from TOML.

use std::path::Path;

use anyhow::{Context, Result};

use super::config::TargetConfig;

/// Load a target configuration from a TOML file.
pub fn load_target_config(path: &Path) -> Result<TargetConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read target file: {}", path.display()))?;
    parse_target_toml(&content)
        .with_context(|| format!("Failed to parse target file: {}", path.display()))
}

/// Parse a target configuration from a TOML string.
pub fn parse_target_toml(content: &str) -> Result<TargetConfig> {
    let config: TargetConfig =
        toml::from_str(content).context("Invalid target TOML configuration")?;
    Ok(config)
}

/// Serialize a target configuration to a TOML string.
pub fn serialize_target_toml(config: &TargetConfig) -> Result<String> {
    toml::to_string_pretty(config).context("Failed to serialize target config to TOML")
}

/// Save a target configuration to a TOML file.
pub fn save_target_config(config: &TargetConfig, path: &Path) -> Result<()> {
    let content = serialize_target_toml(config)?;
    std::fs::write(path, content)
        .with_context(|| format!("Failed to write target file: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::spring_smf::SpringSmfCodec;

    #[test]
    fn test_roundtrip_spring_smf_config() {
        let config = SpringSmfCodec::default_config();
        let toml_str = serialize_target_toml(&config).unwrap();

        // Verify it contains expected fields
        assert!(toml_str.contains("spring-smf"));
        assert!(toml_str.contains("square_size = 8"));

        // Roundtrip
        let parsed = parse_target_toml(&toml_str).unwrap();
        assert_eq!(parsed.id, config.id);
        assert_eq!(parsed.codec, config.codec);
        assert_eq!(
            parsed.codec_params.square_size,
            config.codec_params.square_size
        );
        assert_eq!(parsed.layers.len(), config.layers.len());
    }

    #[test]
    fn test_parse_minimal_config() {
        let toml = r#"
            id = "test-target"
            name = "Test Target"
            codec = "spring-smf"
        "#;

        let config = parse_target_toml(toml).unwrap();
        assert_eq!(config.id, "test-target");
        assert_eq!(config.codec, "spring-smf");
        // Defaults should be applied
        assert_eq!(config.codec_params.square_size, 8);
    }
}

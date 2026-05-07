//! Registry of export codecs and target configs.

use std::collections::HashMap;

use super::codec::ExportCodec;
use super::config::TargetConfig;
use super::raw_layers::RawLayersCodec;
use super::spring_smf::SpringSmfCodec;

/// Registry for available export targets and codecs.
pub struct TargetRegistry {
    /// Codec implementations indexed by codec ID.
    codecs: HashMap<String, Box<dyn ExportCodec>>,
    /// Built-in and custom target configurations indexed by target ID.
    targets: HashMap<String, TargetConfig>,
}

impl TargetRegistry {
    /// Create a new registry with built-in codecs and targets.
    pub fn new() -> Self {
        let mut registry = Self {
            codecs: HashMap::new(),
            targets: HashMap::new(),
        };
        registry.register_builtins();
        registry
    }

    /// Register all built-in codecs and their default targets.
    fn register_builtins(&mut self) {
        // Spring/Recoil SMF codec
        let smf_codec = SpringSmfCodec;
        let smf_config = SpringSmfCodec::default_config();
        self.codecs
            .insert(smf_codec.id().to_string(), Box::new(smf_codec));
        self.targets.insert(smf_config.id.clone(), smf_config);

        // Raw layers codec (generic PNG export)
        let raw_codec = RawLayersCodec;
        let raw_config = RawLayersCodec::default_config();
        self.codecs
            .insert(raw_codec.id().to_string(), Box::new(raw_codec));
        self.targets.insert(raw_config.id.clone(), raw_config);
    }

    /// Register a custom codec.
    pub fn register_codec(&mut self, codec: Box<dyn ExportCodec>) {
        self.codecs.insert(codec.id().to_string(), codec);
    }

    /// Register a custom target configuration.
    pub fn register_target(&mut self, config: TargetConfig) {
        self.targets.insert(config.id.clone(), config);
    }

    /// Look up a codec by ID.
    pub fn get_codec(&self, id: &str) -> Option<&dyn ExportCodec> {
        self.codecs.get(id).map(|c| c.as_ref())
    }

    /// Look up a target config by ID.
    pub fn get_target(&self, id: &str) -> Option<&TargetConfig> {
        self.targets.get(id)
    }

    /// List all registered target IDs.
    pub fn target_ids(&self) -> Vec<&str> {
        self.targets.keys().map(|s| s.as_str()).collect()
    }

    /// List all registered codec IDs.
    pub fn codec_ids(&self) -> Vec<&str> {
        self.codecs.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for TargetRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_has_spring_smf() {
        let registry = TargetRegistry::new();
        assert!(registry.get_codec("spring-smf").is_some());
        assert!(registry.get_target("spring-smf").is_some());
    }

    #[test]
    fn test_registry_target_ids() {
        let registry = TargetRegistry::new();
        let ids = registry.target_ids();
        assert!(ids.contains(&"spring-smf"));
    }

    #[test]
    fn test_registry_codec_ids() {
        let registry = TargetRegistry::new();
        let ids = registry.codec_ids();
        assert!(ids.contains(&"spring-smf"));
    }
}

//! Typed dimension model for export targets.

/// The base unit for dimension calculations.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionBase {
    /// Heightmap sample count (e.g., 4097 for a 4096-square map).
    HeightSamples,
    /// Map square count (e.g., 4096 = heightmap - 1).
    MapSquares,
    /// Fixed pixel dimensions (independent of map size).
    Fixed,
}

/// A constraint on a dimension value.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DimensionConstraint {
    /// Must be a multiple of this value (0 = no constraint).
    #[serde(default)]
    pub multiple_of: u32,
    /// Minimum allowed value.
    #[serde(default)]
    pub min: u32,
    /// Maximum allowed value (0 = no limit).
    #[serde(default)]
    pub max: u32,
}

impl DimensionConstraint {
    pub fn none() -> Self {
        Self {
            multiple_of: 0,
            min: 0,
            max: 0,
        }
    }

    /// Check if a value satisfies this constraint.
    pub fn check(&self, value: u32) -> Result<(), String> {
        if self.min > 0 && value < self.min {
            return Err(format!("value {} is below minimum {}", value, self.min));
        }
        if self.max > 0 && value > self.max {
            return Err(format!("value {} exceeds maximum {}", value, self.max));
        }
        if self.multiple_of > 0 && !value.is_multiple_of(self.multiple_of) {
            return Err(format!(
                "value {} is not a multiple of {}",
                value, self.multiple_of
            ));
        }
        Ok(())
    }
}

/// A rule for computing a layer's resolution from the base map dimensions.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DimensionRule {
    /// What unit the resolution is relative to.
    pub base: DimensionBase,
    /// Scale factor applied to the base (e.g., 0.5 for half-resolution).
    #[serde(default = "one")]
    pub scale: f32,
    /// Additive offset after scaling (e.g., +1 for heightmap = mapx + 1).
    #[serde(default)]
    pub offset: i32,
    /// Fixed dimensions (only used when base = Fixed).
    #[serde(default)]
    pub fixed_width: u32,
    #[serde(default)]
    pub fixed_height: u32,
}

fn one() -> f32 {
    1.0
}

impl DimensionRule {
    /// Full heightmap resolution: height_samples (mapx + 1).
    pub fn height_samples() -> Self {
        Self {
            base: DimensionBase::HeightSamples,
            scale: 1.0,
            offset: 0,
            fixed_width: 0,
            fixed_height: 0,
        }
    }

    /// Map squares resolution (mapx × mapy).
    pub fn map_squares() -> Self {
        Self {
            base: DimensionBase::MapSquares,
            scale: 1.0,
            offset: 0,
            fixed_width: 0,
            fixed_height: 0,
        }
    }

    /// Half of map squares (mapx/2 × mapy/2).
    pub fn half_map_squares() -> Self {
        Self {
            base: DimensionBase::MapSquares,
            scale: 0.5,
            offset: 0,
            fixed_width: 0,
            fixed_height: 0,
        }
    }

    /// Fixed resolution (e.g., 1024×1024 minimap).
    pub fn fixed(width: u32, height: u32) -> Self {
        Self {
            base: DimensionBase::Fixed,
            scale: 1.0,
            offset: 0,
            fixed_width: width,
            fixed_height: height,
        }
    }

    /// Compute actual pixel dimensions given the map's square count.
    pub fn resolve(&self, map_squares_x: u32, map_squares_y: u32) -> (u32, u32) {
        match self.base {
            DimensionBase::HeightSamples => {
                let base_x = map_squares_x + 1;
                let base_y = map_squares_y + 1;
                let w = (base_x as f32 * self.scale) as i32 + self.offset;
                let h = (base_y as f32 * self.scale) as i32 + self.offset;
                (w.max(1) as u32, h.max(1) as u32)
            }
            DimensionBase::MapSquares => {
                let w = (map_squares_x as f32 * self.scale) as i32 + self.offset;
                let h = (map_squares_y as f32 * self.scale) as i32 + self.offset;
                (w.max(1) as u32, h.max(1) as u32)
            }
            DimensionBase::Fixed => (self.fixed_width, self.fixed_height),
        }
    }
}

/// Computed dimensions for all layers in an export.
#[derive(Debug, Clone)]
pub struct DimensionSet {
    /// Map square count (width, height).
    pub map_squares: (u32, u32),
    /// Per-layer resolved dimensions.
    pub layer_dimensions: Vec<(String, u32, u32)>,
}

impl DimensionSet {
    /// Get the resolved dimensions for a named layer.
    pub fn get(&self, layer_name: &str) -> Option<(u32, u32)> {
        self.layer_dimensions
            .iter()
            .find(|(name, _, _)| name == layer_name)
            .map(|(_, w, h)| (*w, *h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_height_samples_rule() {
        let rule = DimensionRule::height_samples();
        assert_eq!(rule.resolve(4096, 4096), (4097, 4097));
        assert_eq!(rule.resolve(1024, 1024), (1025, 1025));
    }

    #[test]
    fn test_half_map_squares_rule() {
        let rule = DimensionRule::half_map_squares();
        assert_eq!(rule.resolve(4096, 4096), (2048, 2048));
        assert_eq!(rule.resolve(1024, 1024), (512, 512));
    }

    #[test]
    fn test_fixed_rule() {
        let rule = DimensionRule::fixed(1024, 1024);
        assert_eq!(rule.resolve(4096, 4096), (1024, 1024));
        assert_eq!(rule.resolve(256, 256), (1024, 1024));
    }

    #[test]
    fn test_constraint_check() {
        let c = DimensionConstraint {
            multiple_of: 128,
            min: 128,
            max: 32768,
        };
        assert!(c.check(4096).is_ok());
        assert!(c.check(1024).is_ok());
        assert!(c.check(100).is_err()); // not multiple of 128
        assert!(c.check(64).is_err()); // below min
        assert!(c.check(65536).is_err()); // above max
    }
}

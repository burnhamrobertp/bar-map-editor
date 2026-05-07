use bytemuck::{Pod, Zeroable};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum HeightmapError {
    #[error("dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },

    #[error("invalid dimensions: {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },
}

/// A 2D heightmap stored as 32-bit floats.
/// Values are normalized to [0.0, 1.0] during processing.
#[derive(Clone, Debug)]
pub struct Heightmap {
    width: u32,
    height: u32,
    data: Vec<f32>,
}

/// A single heightmap sample, suitable for GPU buffer transfer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct HeightSample {
    pub value: f32,
}

impl Heightmap {
    /// Create a new heightmap filled with zeros.
    pub fn new(width: u32, height: u32) -> Result<Self, HeightmapError> {
        if width == 0 || height == 0 {
            return Err(HeightmapError::InvalidDimensions { width, height });
        }
        let size = (width as usize) * (height as usize);
        Ok(Self {
            width,
            height,
            data: vec![0.0; size],
        })
    }

    /// Create a heightmap from existing data.
    pub fn frbar_data(width: u32, height: u32, data: Vec<f32>) -> Result<Self, HeightmapError> {
        let expected = (width as usize) * (height as usize);
        if data.len() != expected {
            return Err(HeightmapError::DimensionMismatch {
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Get the raw float data as a slice.
    pub fn data(&self) -> &[f32] {
        &self.data
    }

    /// Get mutable access to the raw float data.
    pub fn data_mut(&mut self) -> &mut [f32] {
        &mut self.data
    }

    /// Get the data as bytes for GPU upload.
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.data)
    }

    /// Get a sample at (x, y). Returns None if out of bounds.
    pub fn get(&self, x: u32, y: u32) -> Option<f32> {
        if x < self.width && y < self.height {
            Some(self.data[(y as usize) * (self.width as usize) + (x as usize)])
        } else {
            None
        }
    }

    /// Set a sample at (x, y). Returns Err if out of bounds.
    pub fn set(&mut self, x: u32, y: u32, value: f32) -> Result<(), HeightmapError> {
        if x < self.width && y < self.height {
            self.data[(y as usize) * (self.width as usize) + (x as usize)] = value;
            Ok(())
        } else {
            Err(HeightmapError::InvalidDimensions {
                width: x,
                height: y,
            })
        }
    }

    /// Convert to 16-bit unsigned values (for .sd7 export).
    /// Clamps input to [0.0, 1.0] range.
    pub fn to_u16(&self) -> Vec<u16> {
        self.data
            .iter()
            .map(|&v| (v.clamp(0.0, 1.0) * 65535.0) as u16)
            .collect()
    }

    /// Create from 16-bit unsigned values (for .sd7 import).
    pub fn from_u16(width: u32, height: u32, data: &[u16]) -> Result<Self, HeightmapError> {
        let expected = (width as usize) * (height as usize);
        if data.len() != expected {
            return Err(HeightmapError::DimensionMismatch {
                expected,
                actual: data.len(),
            });
        }
        let float_data: Vec<f32> = data.iter().map(|&v| v as f32 / 65535.0).collect();
        Ok(Self {
            width,
            height,
            data: float_data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_heightmap() {
        let hm = Heightmap::new(128, 128).unwrap();
        assert_eq!(hm.width(), 128);
        assert_eq!(hm.height(), 128);
        assert_eq!(hm.data().len(), 128 * 128);
        assert!(hm.data().iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_get_set() {
        let mut hm = Heightmap::new(64, 64).unwrap();
        hm.set(10, 20, 0.5).unwrap();
        assert_eq!(hm.get(10, 20), Some(0.5));
        assert_eq!(hm.get(100, 100), None);
    }

    #[test]
    fn test_u16_roundtrip() {
        let mut hm = Heightmap::new(4, 4).unwrap();
        hm.set(0, 0, 0.0).unwrap();
        hm.set(1, 0, 0.5).unwrap();
        hm.set(2, 0, 1.0).unwrap();

        let u16_data = hm.to_u16();
        assert_eq!(u16_data[0], 0);
        assert_eq!(u16_data[2], 65535);

        let restored = Heightmap::from_u16(4, 4, &u16_data).unwrap();
        // Allow small precision loss from u16 quantization
        assert!((restored.get(1, 0).unwrap() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_invalid_dimensions() {
        assert!(Heightmap::new(0, 100).is_err());
        assert!(Heightmap::new(100, 0).is_err());
    }

    #[test]
    fn test_data_mismatch() {
        let result = Heightmap::frbar_data(4, 4, vec![0.0; 10]);
        assert!(result.is_err());
    }
}

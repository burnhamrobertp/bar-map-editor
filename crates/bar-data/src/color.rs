//! RGBA color buffer for diffuse textures and color maps.

use crate::heightmap::HeightmapError;

/// An RGBA color buffer stored as `[r, g, b, a]` f32 per pixel, each in [0.0, 1.0].
#[derive(Clone, Debug)]
pub struct ColorBuffer {
    width: u32,
    height: u32,
    /// Packed RGBA: length = width * height * 4
    data: Vec<f32>,
}

impl ColorBuffer {
    /// Create a new color buffer filled with opaque black.
    pub fn new(width: u32, height: u32) -> Result<Self, HeightmapError> {
        if width == 0 || height == 0 {
            return Err(HeightmapError::InvalidDimensions { width, height });
        }
        let size = (width as usize) * (height as usize) * 4;
        let mut data = vec![0.0f32; size];
        // Set alpha to 1.0
        for i in (3..size).step_by(4) {
            data[i] = 1.0;
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// Create from raw RGBA f32 data.
    pub fn frbar_data(width: u32, height: u32, data: Vec<f32>) -> Result<Self, HeightmapError> {
        let expected = (width as usize) * (height as usize) * 4;
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

    pub fn data(&self) -> &[f32] {
        &self.data
    }

    /// Mutable access to the raw RGBA float data.
    pub fn data_mut(&mut self) -> &mut [f32] {
        &mut self.data
    }

    /// Get pixel RGBA at (x, y).
    pub fn get(&self, x: u32, y: u32) -> Option<[f32; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        Some([
            self.data[idx],
            self.data[idx + 1],
            self.data[idx + 2],
            self.data[idx + 3],
        ])
    }

    /// Set pixel RGBA at (x, y).
    pub fn set(&mut self, x: u32, y: u32, rgba: [f32; 4]) {
        if x < self.width && y < self.height {
            let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
            self.data[idx] = rgba[0];
            self.data[idx + 1] = rgba[1];
            self.data[idx + 2] = rgba[2];
            self.data[idx + 3] = rgba[3];
        }
    }

    /// Create from 8-bit RGBA bytes (e.g., decoded tile data).
    pub fn from_rgba8(width: u32, height: u32, rgba8: &[u8]) -> Result<Self, HeightmapError> {
        let expected = (width as usize) * (height as usize) * 4;
        if rgba8.len() != expected {
            return Err(HeightmapError::DimensionMismatch {
                expected,
                actual: rgba8.len(),
            });
        }
        let data = rgba8.iter().map(|&v| v as f32 / 255.0).collect();
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// Convert to 8-bit RGBA packed bytes (for image encoding).
    pub fn to_rgba8(&self) -> Vec<u8> {
        self.data
            .iter()
            .map(|&v| (v.clamp(0.0, 1.0) * 255.0) as u8)
            .collect()
    }

    /// Resize to a new dimension using bilinear interpolation.
    pub fn resize(&self, new_w: u32, new_h: u32) -> Self {
        let mut out = ColorBuffer::new(new_w, new_h).unwrap();
        let sx = self.width as f32 / new_w as f32;
        let sy = self.height as f32 / new_h as f32;

        for y in 0..new_h {
            for x in 0..new_w {
                let src_x = x as f32 * sx;
                let src_y = y as f32 * sy;
                let x0 = (src_x as u32).min(self.width - 1);
                let y0 = (src_y as u32).min(self.height - 1);
                let x1 = (x0 + 1).min(self.width - 1);
                let y1 = (y0 + 1).min(self.height - 1);
                let fx = src_x.fract();
                let fy = src_y.fract();

                let c00 = self.get(x0, y0).unwrap_or([0.0; 4]);
                let c10 = self.get(x1, y0).unwrap_or([0.0; 4]);
                let c01 = self.get(x0, y1).unwrap_or([0.0; 4]);
                let c11 = self.get(x1, y1).unwrap_or([0.0; 4]);

                let mut c = [0.0f32; 4];
                for i in 0..4 {
                    let top = c00[i] * (1.0 - fx) + c10[i] * fx;
                    let bot = c01[i] * (1.0 - fx) + c11[i] * fx;
                    c[i] = top * (1.0 - fy) + bot * fy;
                }
                out.set(x, y, c);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_buffer_basic() {
        let mut buf = ColorBuffer::new(4, 4).unwrap();
        buf.set(0, 0, [1.0, 0.0, 0.0, 1.0]);
        let c = buf.get(0, 0).unwrap();
        assert_eq!(c, [1.0, 0.0, 0.0, 1.0]);
        // Default is opaque black
        let c2 = buf.get(1, 1).unwrap();
        assert_eq!(c2, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn test_to_rgba8() {
        let mut buf = ColorBuffer::new(1, 1).unwrap();
        buf.set(0, 0, [0.5, 1.0, 0.0, 0.75]);
        let bytes = buf.to_rgba8();
        assert_eq!(bytes.len(), 4);
        assert!((bytes[0] as i32 - 127).abs() <= 1); // 0.5 * 255 ≈ 127-128
        assert_eq!(bytes[1], 255);
        assert_eq!(bytes[2], 0);
        assert!((bytes[3] as i32 - 191).abs() <= 1);
    }

    #[test]
    fn test_resize() {
        let mut buf = ColorBuffer::new(2, 2).unwrap();
        buf.set(0, 0, [1.0, 0.0, 0.0, 1.0]);
        buf.set(1, 0, [0.0, 1.0, 0.0, 1.0]);
        buf.set(0, 1, [0.0, 0.0, 1.0, 1.0]);
        buf.set(1, 1, [1.0, 1.0, 1.0, 1.0]);
        let resized = buf.resize(4, 4);
        assert_eq!(resized.width(), 4);
        assert_eq!(resized.height(), 4);
    }
}

//! RGBA color buffer for diffuse textures and color maps.

use crate::heightmap::{Heightmap, HeightmapError};

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

    /// Extract one channel (0=R, 1=G, 2=B, 3=A) as a Heightmap of the same dims.
    pub fn channel(&self, c: usize) -> Heightmap {
        let n = (self.width as usize) * (self.height as usize);
        let mut data = vec![0.0f32; n];
        for (i, px) in self.data.chunks_exact(4).enumerate() {
            data[i] = px[c];
        }
        Heightmap::frbar_data(self.width, self.height, data).unwrap()
    }

    /// Interleave three (or four) single-channel Heightmaps into an RGBA buffer.
    /// Uses the minimum common dimensions across all inputs; when `a` is None,
    /// alpha is filled with 1.0 (fully opaque).
    pub fn from_channels(
        r: &Heightmap,
        g: &Heightmap,
        b: &Heightmap,
        a: Option<&Heightmap>,
    ) -> ColorBuffer {
        let mut w = r.width().min(g.width()).min(b.width());
        let mut h = r.height().min(g.height()).min(b.height());
        if let Some(a) = a {
            w = w.min(a.width());
            h = h.min(a.height());
        }

        let mut data = vec![0.0f32; (w as usize) * (h as usize) * 4];
        for y in 0..h {
            for x in 0..w {
                let idx = ((y as usize) * (w as usize) + (x as usize)) * 4;
                data[idx] = r.get(x, y).unwrap_or(0.0);
                data[idx + 1] = g.get(x, y).unwrap_or(0.0);
                data[idx + 2] = b.get(x, y).unwrap_or(0.0);
                data[idx + 3] = a.map_or(1.0, |a| a.get(x, y).unwrap_or(1.0));
            }
        }
        ColorBuffer::frbar_data(w, h, data).unwrap()
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

    #[test]
    fn test_channel_extracts_each_plane() {
        let mut buf = ColorBuffer::new(2, 1).unwrap();
        buf.set(0, 0, [0.1, 0.2, 0.3, 0.4]);
        buf.set(1, 0, [0.5, 0.6, 0.7, 0.8]);

        let r = buf.channel(0);
        let g = buf.channel(1);
        let b = buf.channel(2);
        let a = buf.channel(3);

        assert_eq!(r.data(), &[0.1, 0.5]);
        assert_eq!(g.data(), &[0.2, 0.6]);
        assert_eq!(b.data(), &[0.3, 0.7]);
        assert_eq!(a.data(), &[0.4, 0.8]);
        assert_eq!((r.width(), r.height()), (2, 1));
    }

    #[test]
    fn test_split_then_merge_round_trips() {
        let mut buf = ColorBuffer::new(3, 2).unwrap();
        for y in 0..2 {
            for x in 0..3 {
                let f = (y * 3 + x) as f32 / 6.0;
                buf.set(x, y, [f, 1.0 - f, f * 0.5, 0.25 + f * 0.5]);
            }
        }

        let r = buf.channel(0);
        let g = buf.channel(1);
        let b = buf.channel(2);
        let a = buf.channel(3);
        let merged = ColorBuffer::from_channels(&r, &g, &b, Some(&a));

        assert_eq!(merged.width(), buf.width());
        assert_eq!(merged.height(), buf.height());
        for (orig, got) in buf.data().iter().zip(merged.data()) {
            assert!((orig - got).abs() < 1e-6, "{orig} != {got}");
        }
    }

    #[test]
    fn test_from_channels_missing_alpha_is_opaque() {
        let r = Heightmap::frbar_data(2, 1, vec![0.2, 0.4]).unwrap();
        let g = Heightmap::frbar_data(2, 1, vec![0.0, 0.0]).unwrap();
        let b = Heightmap::frbar_data(2, 1, vec![0.0, 0.0]).unwrap();

        let merged = ColorBuffer::from_channels(&r, &g, &b, None);

        assert_eq!(merged.get(0, 0).unwrap()[3], 1.0);
        assert_eq!(merged.get(1, 0).unwrap()[3], 1.0);
        assert_eq!(merged.get(0, 0).unwrap()[0], 0.2);
    }
}

//! DDS cubemap loader for `mapinfo.atmosphere.skyBox` files.
//!
//! BAR maps ship their skybox as a single `.dds` cubemap (six faces packed
//! into one file). The engine loads it via DevIL; we use the `ddsfile`
//! crate, which understands the DDS_HEADER_CAPS2_CUBEMAP flag and gives
//! us per-face byte access.
//!
//! Output is one decoded `Cubemap` -- 6 face buffers in cross-platform
//! `Rgba8Unorm` format. The renderer uploads these into a `TextureView`
//! with `dimension: D2`, `array_layer_count: 6`, which wgpu treats as a
//! cubemap when bound to a `TextureViewDimension::Cube` slot.

use std::path::Path;

use ddsfile::{Caps2, D3DFormat, Dds, DxgiFormat};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SkyboxError {
    #[error("DDS file not found: {0}")]
    NotFound(std::path::PathBuf),
    #[error("Failed to read DDS file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to parse DDS: {0}")]
    Parse(#[from] ddsfile::Error),
    #[error("Not a cubemap (missing CUBEMAP caps flag)")]
    NotACubemap,
    #[error("Unsupported DDS format -- only uncompressed RGB(A) and BC1/2/3 are handled")]
    UnsupportedFormat,
}

/// Decoded skybox cubemap: 6 face buffers, all in `Rgba8Unorm` layout,
/// row-major top-to-bottom. Face order is the wgpu / D3D convention:
/// +X, -X, +Y, -Y, +Z, -Z.
pub struct Cubemap {
    pub width: u32,
    pub height: u32,
    /// Length 6 -- one `width*height*4` buffer per face.
    pub faces: [Vec<u8>; 6],
}

/// Load a DDS cubemap from disk and decode each face to `Rgba8Unorm`.
pub fn load_dds_cubemap(path: &Path) -> Result<Cubemap, SkyboxError> {
    if !path.exists() {
        return Err(SkyboxError::NotFound(path.to_path_buf()));
    }
    let bytes = std::fs::read(path)?;
    let dds = Dds::read(&bytes[..])?;

    // Confirm this DDS is actually a cubemap. Older maps sometimes set
    // CUBEMAP without all face flags; we treat the presence of CUBEMAP
    // as authoritative and trust the file has all 6 faces.
    let caps2 = dds.header.caps2;
    if !caps2.contains(Caps2::CUBEMAP) {
        return Err(SkyboxError::NotACubemap);
    }

    let width = dds.header.width;
    let height = dds.header.height;

    // `ddsfile::Dds::get_data(face_index)` returns one face's data,
    // including its mip chain (base mip first). We only need the base
    // level, so we trim each face to `face_byte_len`. Mistaking this
    // for "all faces concatenated" is what made the loader reject every
    // mipmapped cubemap (`raw.len() = base_mip + smaller_mips < 6 *
    // base_mip` → it bailed out with `UnsupportedFormat`).
    let face_byte_len = mip_byte_len_for(&dds, width, height)?;
    let mut faces: [Vec<u8>; 6] = Default::default();
    for (i, face) in faces.iter_mut().enumerate() {
        let face_data = dds.get_data(i as u32)?;
        if face_data.len() < face_byte_len {
            return Err(SkyboxError::UnsupportedFormat);
        }
        *face = decode_face(&dds, &face_data[..face_byte_len], width, height)?;
    }

    Ok(Cubemap {
        width,
        height,
        faces,
    })
}

/// One mip level decoded to `Rgba8Unorm`.
pub struct DdsMip {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Load a 2D DDS file (NOT a cubemap) and decode every mip level present
/// in the file. The first entry is the base mip; subsequent entries are
/// the half-size chain (1024 -> 512 -> 256 -> ...) down to 1x1. Same
/// supported pixel formats as `load_dds_cubemap`. Used for splat
/// distribution, splat detail-normal, sky reflection mask, and map
/// detail textures that BAR ships as 2D DDS.
///
/// Reading the file's pre-baked mip chain matches engine behaviour --
/// the BAR engine loads DDS as-is via DevIL with all mips intact, then
/// samples trilinearly. Without the mip chain, oblique-angle sampling
/// of the high-frequency detail-normal textures aliases into per-pixel
/// noise that lights every shadowed surface inconsistently.
pub fn load_dds_2d_with_mips(path: &Path) -> Result<Vec<DdsMip>, SkyboxError> {
    if !path.exists() {
        return Err(SkyboxError::NotFound(path.to_path_buf()));
    }
    let bytes = std::fs::read(path)?;
    let dds = Dds::read(&bytes[..])?;
    let base_w = dds.header.width;
    let base_h = dds.header.height;
    let mip_count = dds.get_num_mipmap_levels().max(1);
    let raw = dds.get_data(0)?;

    let mut out = Vec::with_capacity(mip_count as usize);
    let mut offset: usize = 0;
    for level in 0..mip_count {
        let w = (base_w >> level).max(1);
        let h = (base_h >> level).max(1);
        let byte_len = mip_byte_len_for(&dds, w, h)?;
        if offset + byte_len > raw.len() {
            // DDS claimed more mips than the file actually carries. Stop
            // gracefully -- engine does the same: it just uses whatever
            // chain length is present and lets the driver clamp.
            break;
        }
        let rgba = decode_face(&dds, &raw[offset..offset + byte_len], w, h)?;
        out.push(DdsMip {
            width: w,
            height: h,
            rgba,
        });
        offset += byte_len;
    }
    if out.is_empty() {
        return Err(SkyboxError::UnsupportedFormat);
    }
    Ok(out)
}

/// Convenience wrapper for callers that only need the base mip (sky
/// reflection mask, detail tex). Equivalent to
/// `load_dds_2d_with_mips(path)?.remove(0)` flattened to a tuple.
pub fn load_dds_2d(path: &Path) -> Result<(Vec<u8>, u32, u32), SkyboxError> {
    let mut mips = load_dds_2d_with_mips(path)?;
    let base = mips.remove(0);
    Ok((base.rgba, base.width, base.height))
}

/// Decode the base mip of a DDS from in-memory bytes. Used by callers
/// that already have the file bytes loaded (e.g. feature textures pulled
/// out of an archive) and shouldn't have to round-trip through a temp
/// file just to reach the decoder.
pub fn load_dds_2d_bytes(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), SkyboxError> {
    let dds = Dds::read(bytes)?;
    let base_w = dds.header.width;
    let base_h = dds.header.height;
    let raw = dds.get_data(0)?;
    let byte_len = mip_byte_len_for(&dds, base_w, base_h)?;
    if byte_len > raw.len() {
        return Err(SkyboxError::UnsupportedFormat);
    }
    let rgba = decode_face(&dds, &raw[..byte_len], base_w, base_h)?;
    Ok((rgba, base_w, base_h))
}

/// Bytes for one mip level of size `w` x `h`, derived from the DDS pixel
/// format. Supports uncompressed 32bpp RGBA (most BAR map skyboxes are
/// this) and the BC1/2/3 block-compressed formats. Other formats need
/// explicit decoding paths.
fn mip_byte_len_for(dds: &Dds, w: u32, h: u32) -> Result<usize, SkyboxError> {
    let pixels = (w * h) as usize;
    if let Some(fmt) = dds.get_d3d_format() {
        match fmt {
            D3DFormat::A8B8G8R8
            | D3DFormat::A8R8G8B8
            | D3DFormat::X8R8G8B8
            | D3DFormat::X8B8G8R8 => Ok(pixels * 4),
            D3DFormat::R8G8B8 => Ok(pixels * 3),
            D3DFormat::DXT1 => Ok((w.div_ceil(4) * h.div_ceil(4) * 8) as usize),
            D3DFormat::DXT3 | D3DFormat::DXT5 => Ok((w.div_ceil(4) * h.div_ceil(4) * 16) as usize),
            _ => Err(SkyboxError::UnsupportedFormat),
        }
    } else if let Some(fmt) = dds.get_dxgi_format() {
        match fmt {
            DxgiFormat::R8G8B8A8_UNorm | DxgiFormat::R8G8B8A8_UNorm_sRGB => Ok(pixels * 4),
            DxgiFormat::BC1_UNorm | DxgiFormat::BC1_UNorm_sRGB => {
                Ok((w.div_ceil(4) * h.div_ceil(4) * 8) as usize)
            }
            DxgiFormat::BC3_UNorm | DxgiFormat::BC3_UNorm_sRGB => {
                Ok((w.div_ceil(4) * h.div_ceil(4) * 16) as usize)
            }
            _ => Err(SkyboxError::UnsupportedFormat),
        }
    } else {
        Err(SkyboxError::UnsupportedFormat)
    }
}

/// Decode a single face into a `width*height*4` `Rgba8Unorm` buffer.
fn decode_face(dds: &Dds, src: &[u8], w: u32, h: u32) -> Result<Vec<u8>, SkyboxError> {
    let pixels = (w * h) as usize;
    let mut out = vec![0u8; pixels * 4];

    // Uncompressed paths -- most BAR skyboxes (`cleardesert.dds`,
    // `lava.dds`, etc.) ship as A8R8G8B8 or X8R8G8B8.
    if let Some(fmt) = dds.get_d3d_format() {
        match fmt {
            D3DFormat::A8R8G8B8 | D3DFormat::X8R8G8B8 => {
                // DDS stores pixels as BGRA; swap to RGBA.
                for i in 0..pixels {
                    let s = i * 4;
                    out[s] = src[s + 2];
                    out[s + 1] = src[s + 1];
                    out[s + 2] = src[s];
                    out[s + 3] = if matches!(fmt, D3DFormat::X8R8G8B8) {
                        255
                    } else {
                        src[s + 3]
                    };
                }
                return Ok(out);
            }
            D3DFormat::A8B8G8R8 | D3DFormat::X8B8G8R8 => {
                out.copy_from_slice(&src[..pixels * 4]);
                if matches!(fmt, D3DFormat::X8B8G8R8) {
                    for i in 0..pixels {
                        out[i * 4 + 3] = 255;
                    }
                }
                return Ok(out);
            }
            D3DFormat::R8G8B8 => {
                for i in 0..pixels {
                    out[i * 4] = src[i * 3];
                    out[i * 4 + 1] = src[i * 3 + 1];
                    out[i * 4 + 2] = src[i * 3 + 2];
                    out[i * 4 + 3] = 255;
                }
                return Ok(out);
            }
            // BC1/2/3 fall through to the block-compressed handler below.
            D3DFormat::DXT1 | D3DFormat::DXT3 | D3DFormat::DXT5 => {}
            _ => return Err(SkyboxError::UnsupportedFormat),
        }
    }
    if let Some(fmt) = dds.get_dxgi_format() {
        match fmt {
            DxgiFormat::R8G8B8A8_UNorm | DxgiFormat::R8G8B8A8_UNorm_sRGB => {
                out.copy_from_slice(&src[..pixels * 4]);
                return Ok(out);
            }
            DxgiFormat::BC1_UNorm
            | DxgiFormat::BC1_UNorm_sRGB
            | DxgiFormat::BC3_UNorm
            | DxgiFormat::BC3_UNorm_sRGB => {}
            _ => return Err(SkyboxError::UnsupportedFormat),
        }
    }

    // Block-compressed path: BC1 / BC2 / BC3. We reuse the BC1 tile
    // decoder from `smt` for BC1, since SMT tiles are also BC1. BC2/BC3
    // have an extra alpha block but otherwise identical colour layout;
    // for the skybox we only need the RGB channels, so we treat all
    // three as BC1 with a stride of 8 / 16 bytes per block.
    let is_bc3 = matches!(
        dds.get_d3d_format(),
        Some(D3DFormat::DXT3) | Some(D3DFormat::DXT5)
    ) || matches!(
        dds.get_dxgi_format(),
        Some(DxgiFormat::BC3_UNorm) | Some(DxgiFormat::BC3_UNorm_sRGB)
    );
    let block_stride = if is_bc3 { 16 } else { 8 };
    let colour_off = if is_bc3 { 8 } else { 0 };
    let blocks_w = w.div_ceil(4);
    let blocks_h = h.div_ceil(4);
    for by in 0..blocks_h {
        for bx in 0..blocks_w {
            let bi = (by * blocks_w + bx) as usize;
            let block_off = bi * block_stride + colour_off;
            let block: [u8; 8] = src[block_off..block_off + 8]
                .try_into()
                .map_err(|_| SkyboxError::UnsupportedFormat)?;
            let pixels4x4 = crate::smt::decode_dxt1_block(&block);
            for py in 0..4u32 {
                let dy = by * 4 + py;
                if dy >= h {
                    continue;
                }
                for px in 0..4u32 {
                    let dx = bx * 4 + px;
                    if dx >= w {
                        continue;
                    }
                    let src_pixel = pixels4x4[(py * 4 + px) as usize];
                    let dst_i = (dy * w + dx) as usize * 4;
                    out[dst_i] = src_pixel[0];
                    out[dst_i + 1] = src_pixel[1];
                    out[dst_i + 2] = src_pixel[2];
                    out[dst_i + 3] = 255;
                }
            }
        }
    }
    Ok(out)
}

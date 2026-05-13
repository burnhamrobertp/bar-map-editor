//! .sd7 map format reader/writer for Spring/Recoil engine.
//!
//! The SMF (Spring Map Format) file contains:
//! - Map header (dimensions, water level, tile size)
//! - Heightmap (16-bit unsigned)
//! - Metalmap (8-bit)
//! - Typemap (8-bit)
//! - Featuremap (feature placements)
//! - Minimap (DXT1 compressed)
//!
//! Reference: SpringRTS source (rts/Map/SMF/SmfReadMap.cpp)

use std::io::{self, Read, Seek, SeekFrom, Write};

use thiserror::Error;

use crate::Heightmap;

#[derive(Error, Debug)]
pub enum Sd7Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("invalid magic number: expected {expected:#010x}, got {actual:#010x}")]
    InvalidMagic { expected: u32, actual: u32 },

    #[error("unsupported version: {0}")]
    UnsupportedVersion(u32),

    #[error("invalid map dimensions: {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },

    #[error("data size mismatch: expected {expected} bytes, got {actual}")]
    DataSizeMismatch { expected: usize, actual: usize },

    #[error("heightmap error: {0}")]
    Heightmap(#[from] crate::heightmap::HeightmapError),
}

/// A placed feature instance read from or written to an SMF feature section.
///
/// Feature type names are resolved from the in-file index table during read
/// and re-deduplicated into an index table during write.
#[derive(Debug, Clone)]
pub struct SmfFeaturePlacement {
    /// Feature type name (e.g. "arborreal", "GeoTherm_Lava_Rock").
    pub feature_type: String,
    pub x: f32,
    /// World Y position (engine snaps to terrain; typically 0 at import).
    pub y: f32,
    pub z: f32,
    /// Rotation angle in degrees (Spring convention).
    pub angle: f32,
    pub taken_damage: i16,
}

/// SMF file magic number: "spring map file\0" → first 4 bytes = "spri"
pub const SMF_MAGIC: &[u8; 16] = b"spring map file\0";

/// Current supported SMF version
pub const SMF_VERSION: i32 = 1;

/// Header for the Spring Map Format (.smf)
#[derive(Debug, Clone)]
pub struct SmfHeader {
    pub version: i32,
    /// Unique map ID
    pub map_id: i32,
    /// Map width in spring units
    pub map_x: i32,
    /// Map height in spring units
    pub map_y: i32,
    /// Square size (typically 8)
    pub square_size: i32,
    /// Texels per square (typically 8)
    pub texels_per_square: i32,
    /// Tile size (typically 32)
    pub tile_size: i32,
    /// Minimum terrain height (world units)
    pub min_height: f32,
    /// Maximum terrain height (world units)
    pub max_height: f32,
    /// Offset to heightmap data
    pub heightmap_ptr: i32,
    /// Offset to typemap data
    pub typemap_ptr: i32,
    /// Offset to tile index map
    pub tilesmap_ptr: i32,
    /// Offset to minimap
    pub minimap_ptr: i32,
    /// Offset to metalmap
    pub metalmap_ptr: i32,
    /// Offset to feature data
    pub featuremap_ptr: i32,
    /// Number of extra headers
    pub num_extra_headers: i32,
}

impl SmfHeader {
    /// Header size in bytes (magic + all fields)
    pub const SIZE: usize = 16 + 16 * 4; // 16 magic + 16 i32/f32 fields = 80 bytes

    /// Get heightmap dimensions in pixels.
    /// In Spring, the heightmap is (mapx + 1) × (mapy + 1) vertices.
    pub fn heightmap_size(&self) -> (u32, u32) {
        let w = self.map_x + 1;
        let h = self.map_y + 1;
        (w as u32, h as u32)
    }

    /// Get metalmap dimensions (half resolution of map squares).
    /// In Spring, metalmap/typemap are (mapx / 2) × (mapy / 2).
    pub fn metalmap_size(&self) -> (u32, u32) {
        let w = self.map_x / 2;
        let h = self.map_y / 2;
        (w as u32, h as u32)
    }

    /// Get typemap dimensions (same as metalmap).
    pub fn typemap_size(&self) -> (u32, u32) {
        self.metalmap_size()
    }

    /// Full diffuse texture dimensions: map_x * texels_per_square × map_y * texels_per_square.
    pub fn texture_size(&self) -> (u32, u32) {
        (
            self.map_x as u32 * self.texels_per_square as u32,
            self.map_y as u32 * self.texels_per_square as u32,
        )
    }

    /// Tile grid dimensions: how many 32×32 tiles span the texture.
    pub fn tile_grid_size(&self) -> (u32, u32) {
        let tile_res = (self.tile_size / self.square_size).max(1) as u32;
        (self.map_x as u32 / tile_res, self.map_y as u32 / tile_res)
    }

    /// Create a default header for a given map size in spring units.
    pub fn new(map_x: i32, map_y: i32) -> Self {
        Self {
            version: SMF_VERSION,
            map_id: 0,
            map_x,
            map_y,
            square_size: 8,
            texels_per_square: 8,
            tile_size: 32,
            min_height: 0.0,
            max_height: 800.0,
            heightmap_ptr: 0,
            typemap_ptr: 0,
            tilesmap_ptr: 0,
            minimap_ptr: 0,
            metalmap_ptr: 0,
            featuremap_ptr: 0,
            num_extra_headers: 0,
        }
    }

    /// Read header from a reader.
    pub fn read<R: Read>(reader: &mut R) -> Result<Self, Sd7Error> {
        let mut magic = [0u8; 16];
        reader.read_exact(&mut magic)?;
        if &magic != SMF_MAGIC {
            return Err(Sd7Error::InvalidMagic {
                expected: u32::from_le_bytes(SMF_MAGIC[0..4].try_into().unwrap()),
                actual: u32::from_le_bytes(magic[0..4].try_into().unwrap()),
            });
        }

        let version = read_i32(reader)?;
        if version != SMF_VERSION {
            return Err(Sd7Error::UnsupportedVersion(version as u32));
        }

        Ok(Self {
            version,
            map_id: read_i32(reader)?,
            map_x: read_i32(reader)?,
            map_y: read_i32(reader)?,
            square_size: read_i32(reader)?,
            texels_per_square: read_i32(reader)?,
            tile_size: read_i32(reader)?,
            min_height: read_f32(reader)?,
            max_height: read_f32(reader)?,
            heightmap_ptr: read_i32(reader)?,
            typemap_ptr: read_i32(reader)?,
            tilesmap_ptr: read_i32(reader)?,
            minimap_ptr: read_i32(reader)?,
            metalmap_ptr: read_i32(reader)?,
            featuremap_ptr: read_i32(reader)?,
            num_extra_headers: read_i32(reader)?,
        })
    }

    /// Write header to a writer.
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<(), Sd7Error> {
        writer.write_all(SMF_MAGIC)?;
        write_i32(writer, self.version)?;
        write_i32(writer, self.map_id)?;
        write_i32(writer, self.map_x)?;
        write_i32(writer, self.map_y)?;
        write_i32(writer, self.square_size)?;
        write_i32(writer, self.texels_per_square)?;
        write_i32(writer, self.tile_size)?;
        write_f32(writer, self.min_height)?;
        write_f32(writer, self.max_height)?;
        write_i32(writer, self.heightmap_ptr)?;
        write_i32(writer, self.typemap_ptr)?;
        write_i32(writer, self.tilesmap_ptr)?;
        write_i32(writer, self.minimap_ptr)?;
        write_i32(writer, self.metalmap_ptr)?;
        write_i32(writer, self.featuremap_ptr)?;
        write_i32(writer, self.num_extra_headers)?;
        Ok(())
    }
}

/// Complete map data as read from/written to an SMF file.
#[derive(Debug, Clone)]
pub struct SmfMap {
    pub header: SmfHeader,
    pub heightmap: Heightmap,
    /// Metal distribution (8-bit, values 0-255)
    pub metalmap: Vec<u8>,
    /// Terrain type indices (8-bit)
    pub typemap: Vec<u8>,
    /// SMT filename referenced in the tile header (e.g., "maps/mymap.smt").
    pub smt_filename: String,
    /// Tile index map: one i32 per tile position (tiles_x × tiles_y, row-major).
    /// Each value is the index of the tile in the .smt file.
    pub tile_indices: Vec<i32>,
    /// Pre-compressed DXT1 minimap with 9 mipmap levels (exactly 699048 bytes).
    /// If empty, a solid-color placeholder is written.
    pub minimap_dxt1: Vec<u8>,
    /// Feature placements read from or written to the SMF feature section.
    pub features: Vec<SmfFeaturePlacement>,
}

impl SmfMap {
    /// Create a new empty map with given dimensions (in spring units).
    /// Spring units: map_x=2 means 2*512=1024 world units wide.
    pub fn new(map_x: i32, map_y: i32) -> Result<Self, Sd7Error> {
        let header = SmfHeader::new(map_x, map_y);
        let (hm_w, hm_h) = header.heightmap_size();
        let (mm_w, mm_h) = header.metalmap_size();

        let heightmap = Heightmap::new(hm_w, hm_h)?;
        let metalmap = vec![0u8; (mm_w as usize) * (mm_h as usize)];
        let typemap = vec![0u8; (mm_w as usize) * (mm_h as usize)];

        Ok(Self {
            header,
            heightmap,
            metalmap,
            typemap,
            smt_filename: String::new(),
            tile_indices: Vec::new(),
            minimap_dxt1: Vec::new(),
            features: Vec::new(),
        })
    }

    /// Read a map from an SMF file.
    pub fn read<R: Read + Seek>(reader: &mut R) -> Result<Self, Sd7Error> {
        let header = SmfHeader::read(reader)?;
        let (hm_w, hm_h) = header.heightmap_size();
        let (mm_w, mm_h) = header.metalmap_size();

        // Read heightmap
        reader.seek(SeekFrom::Start(header.heightmap_ptr as u64))?;
        let hm_size = (hm_w as usize) * (hm_h as usize);
        let mut hm_data = vec![0u16; hm_size];
        for sample in hm_data.iter_mut() {
            *sample = read_u16(reader)?;
        }
        let heightmap = Heightmap::from_u16(hm_w, hm_h, &hm_data)?;

        // Read metalmap
        let mm_size = (mm_w as usize) * (mm_h as usize);
        let mut metalmap = vec![0u8; mm_size];
        if header.metalmap_ptr > 0 {
            reader.seek(SeekFrom::Start(header.metalmap_ptr as u64))?;
            reader.read_exact(&mut metalmap)?;
        }

        // Read typemap
        let mut typemap = vec![0u8; mm_size];
        if header.typemap_ptr > 0 {
            reader.seek(SeekFrom::Start(header.typemap_ptr as u64))?;
            reader.read_exact(&mut typemap)?;
        }

        // Read tilesmap section to get SMT filename and tile index map.
        // Section layout: numTileFiles:i32, numTiles:i32,
        //   then per file: numTilesInFile:i32 + null-terminated filename,
        //   then tile_indices:[i32; numTiles].
        // Use the stored numTiles from the header (not a re-derived count) to match
        // exactly what the engine wrote. Failures here are non-fatal -- we can still
        // import heightmap/metalmap/typemap without the tile data.
        let mut smt_filename = String::new();
        let mut tile_indices = Vec::new();
        if header.tilesmap_ptr > 0 {
            let tile_result: Result<(), Sd7Error> = (|| {
                reader.seek(SeekFrom::Start(header.tilesmap_ptr as u64))?;
                let num_tile_files = read_i32(reader)?.max(0) as usize;
                let total_tiles = read_i32(reader)?.max(0) as usize;

                for _ in 0..num_tile_files {
                    let _tiles_in_file = read_i32(reader)?;
                    let mut name_bytes = Vec::new();
                    let mut b = [0u8; 1];
                    loop {
                        reader.read_exact(&mut b)?;
                        if b[0] == 0 {
                            break;
                        }
                        name_bytes.push(b[0]);
                    }
                    if smt_filename.is_empty() {
                        smt_filename = String::from_utf8_lossy(&name_bytes).into_owned();
                    }
                }

                tile_indices.reserve(total_tiles);
                for _ in 0..total_tiles {
                    tile_indices.push(read_i32(reader)?);
                }
                Ok(())
            })();
            if let Err(e) = tile_result {
                tracing::warn!(error = %e, "SMF tile section unreadable; tile data skipped");
            }
        }

        // Read feature section. Non-fatal: features are optional for map import.
        let mut features = Vec::new();
        if header.featuremap_ptr > 0 {
            let feat_result: Result<(), Sd7Error> = (|| {
                reader.seek(SeekFrom::Start(header.featuremap_ptr as u64))?;
                let num_types = read_i32(reader)?.max(0) as usize;
                let num_feature_records = read_i32(reader)?.max(0) as usize;

                // Feature type names are null-terminated variable-length strings
                // (not fixed 256-byte buffers as older docs suggest).
                let mut type_names: Vec<String> = Vec::with_capacity(num_types);
                for _ in 0..num_types {
                    let mut name_bytes = Vec::new();
                    let mut b = [0u8; 1];
                    loop {
                        reader.read_exact(&mut b)?;
                        if b[0] == 0 {
                            break;
                        }
                        name_bytes.push(b[0]);
                    }
                    type_names.push(String::from_utf8_lossy(&name_bytes).into_owned());
                }

                for _ in 0..num_feature_records {
                    let type_idx = read_i32(reader)?.max(0) as usize;
                    let x = read_f32(reader)?;
                    let y = read_f32(reader)?;
                    let z = read_f32(reader)?;
                    let angle = read_f32(reader)?;
                    let taken_damage = read_i16(reader)?;
                    let _pad = read_i16(reader)?;
                    let feature_type = type_names.get(type_idx).cloned().unwrap_or_default();
                    features.push(SmfFeaturePlacement {
                        feature_type,
                        x,
                        y,
                        z,
                        angle,
                        taken_damage,
                    });
                }
                Ok(())
            })();
            if let Err(e) = feat_result {
                tracing::warn!(error = %e, "SMF feature section unreadable; features skipped");
            }
        }

        Ok(Self {
            header,
            heightmap,
            metalmap,
            typemap,
            smt_filename,
            tile_indices,
            minimap_dxt1: Vec::new(),
            features,
        })
    }

    /// Write a map to SMF format.
    pub fn write<W: Write + Seek>(&self, writer: &mut W) -> Result<(), Sd7Error> {
        let (hm_w, hm_h) = self.header.heightmap_size();
        let hm_size = (hm_w as usize) * (hm_h as usize);
        let (mm_w, mm_h) = self.header.metalmap_size();
        let mm_size = (mm_w as usize) * (mm_h as usize);

        // Tile index map: number of tiles in each direction
        // tile_res = tile_size / square_size = typically 32/8 = 4
        let tile_res = (self.header.tile_size / self.header.square_size) as u32;
        let tiles_x = self.header.map_x as u32 / tile_res;
        let tiles_y = self.header.map_y as u32 / tile_res;
        let num_tile_indices = (tiles_x * tiles_y) as usize;

        // The SMT filename stored in the SMF (relative path, null-terminated)
        let smt_name = if self.smt_filename.is_empty() {
            "maps/unknown.smt".to_string()
        } else {
            self.smt_filename.clone()
        };
        let smt_filename_bytes = {
            let mut v = smt_name.into_bytes();
            v.push(0); // null terminator
            v
        };
        // MapTileHeader: numTileFiles(4) + numTiles(4)
        // Then per file: numTilesInFile(4) + null-terminated filename
        let tile_header_size = 4 + 4 + 4 + smt_filename_bytes.len();
        let tilesmap_bytes = tile_header_size + num_tile_indices * 4;

        // DXT1 minimap: 1024×1024 with 9 mipmap levels = exactly 699048 bytes
        let minimap_size: usize = 699048;

        // Build deduplicated feature type name table (insertion order).
        let mut type_names: Vec<String> = Vec::new();
        for feat in &self.features {
            if !type_names.contains(&feat.feature_type) {
                type_names.push(feat.feature_type.clone());
            }
        }
        // Feature section: numTypes(4) + numFeatures(4) + type_table(n*256) + records(m*24).
        let feature_section_size = (8 + type_names.len() * 256 + self.features.len() * 24) as i32;

        // Calculate offsets sequentially
        let heightmap_ptr = SmfHeader::SIZE as i32;
        let heightmap_bytes = hm_size * 2; // u16
        let typemap_ptr = heightmap_ptr + heightmap_bytes as i32;
        let tilesmap_ptr = typemap_ptr + mm_size as i32;
        let minimap_ptr = tilesmap_ptr + tilesmap_bytes as i32;
        let metalmap_ptr = minimap_ptr + minimap_size as i32;
        let featuremap_ptr = metalmap_ptr + mm_size as i32;

        // Write header with calculated offsets
        let mut header = self.header.clone();
        header.heightmap_ptr = heightmap_ptr;
        header.typemap_ptr = typemap_ptr;
        header.tilesmap_ptr = tilesmap_ptr;
        header.minimap_ptr = minimap_ptr;
        header.metalmap_ptr = metalmap_ptr;
        header.featuremap_ptr = featuremap_ptr;
        header.write(writer)?;

        // Write heightmap as u16
        let hm_u16 = self.heightmap.to_u16();
        for &sample in &hm_u16 {
            write_u16(writer, sample)?;
        }

        // Write typemap
        writer.write_all(&self.typemap)?;

        // Write tile header (MapTileHeader struct: numTileFiles + numTiles)
        write_i32(writer, 1)?; // numTileFiles = 1
        write_i32(writer, num_tile_indices as i32)?; // numTiles (total across all files)
                                                     // Per tile file entry: numTilesInFile + null-terminated filename
        write_i32(writer, num_tile_indices as i32)?;
        writer.write_all(&smt_filename_bytes)?;

        // Write tile index map (sequential indices: tile 0, 1, 2, ...)
        for i in 0..num_tile_indices {
            write_i32(writer, i as i32)?;
        }

        // Write minimap (DXT1 1024×1024 with 9 mipmap levels = 699048 bytes)
        if self.minimap_dxt1.len() == minimap_size {
            writer.write_all(&self.minimap_dxt1)?;
        } else {
            // Solid dark-green placeholder fallback
            let green_565: u16 = 0x03E0;
            let block = {
                let mut b = [0u8; 8];
                b[0..2].copy_from_slice(&green_565.to_le_bytes());
                b[2..4].copy_from_slice(&green_565.to_le_bytes());
                b
            };
            let (mut w, mut h) = (1024u32, 1024u32);
            for _level in 0..9 {
                let bx = w.max(4) / 4;
                let by = h.max(4) / 4;
                let num_blocks = (bx * by) as usize;
                for _ in 0..num_blocks {
                    writer.write_all(&block)?;
                }
                w = (w / 2).max(1);
                h = (h / 2).max(1);
            }
        }

        // Write metalmap
        writer.write_all(&self.metalmap)?;

        // Write feature section
        write_i32(writer, type_names.len() as i32)?;
        write_i32(writer, self.features.len() as i32)?;
        // Type name table: 256 bytes each, null-padded.
        for name in &type_names {
            let mut name_buf = [0u8; 256];
            let bytes = name.as_bytes();
            let copy_len = bytes.len().min(255);
            name_buf[..copy_len].copy_from_slice(&bytes[..copy_len]);
            writer.write_all(&name_buf)?;
        }
        // Feature records: 24 bytes each.
        for feat in &self.features {
            let type_idx = type_names
                .iter()
                .position(|n| n == &feat.feature_type)
                .unwrap_or(0);
            write_i32(writer, type_idx as i32)?;
            write_f32(writer, feat.x)?;
            write_f32(writer, feat.y)?;
            write_f32(writer, feat.z)?;
            write_f32(writer, feat.angle)?;
            write_i16(writer, feat.taken_damage)?;
            write_i16(writer, 0)?; // padding
        }

        debug_assert_eq!(
            writer.stream_position()? as i32,
            featuremap_ptr + feature_section_size,
            "Feature section end position mismatch"
        );

        writer.flush()?;

        Ok(())
    }
}

// Binary I/O helpers (little-endian)
fn read_i32<R: Read>(r: &mut R) -> Result<i32, io::Error> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(i32::from_le_bytes(buf))
}

fn read_f32<R: Read>(r: &mut R) -> Result<f32, io::Error> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(f32::from_le_bytes(buf))
}

fn read_u16<R: Read>(r: &mut R) -> Result<u16, io::Error> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

fn write_i32<W: Write>(w: &mut W, v: i32) -> Result<(), io::Error> {
    w.write_all(&v.to_le_bytes())
}

fn write_f32<W: Write>(w: &mut W, v: f32) -> Result<(), io::Error> {
    w.write_all(&v.to_le_bytes())
}

fn write_u16<W: Write>(w: &mut W, v: u16) -> Result<(), io::Error> {
    w.write_all(&v.to_le_bytes())
}

fn read_i16<R: Read>(r: &mut R) -> Result<i16, io::Error> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)?;
    Ok(i16::from_le_bytes(buf))
}

fn write_i16<W: Write>(w: &mut W, v: i16) -> Result<(), io::Error> {
    w.write_all(&v.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_header_dimensions() {
        // mapx=4096 means heightmap of (4096+1) = 4097 samples
        let header = SmfHeader::new(4096, 4096);
        let (w, h) = header.heightmap_size();
        assert_eq!(w, 4097);
        assert_eq!(h, 4097);

        let (mw, mh) = header.metalmap_size();
        assert_eq!(mw, 2048);
        assert_eq!(mh, 2048);
    }

    #[test]
    fn test_smf_roundtrip() {
        // mapx=128, mapy=128 → heightmap 129×129, metalmap 64×64
        let mut map = SmfMap::new(128, 128).unwrap();

        // Set some height values
        map.heightmap.set(0, 0, 0.5).unwrap();
        map.heightmap.set(4, 4, 1.0).unwrap();

        // Set metalmap values
        map.metalmap[0] = 128;
        map.metalmap[1] = 255;

        // Set typemap values
        map.typemap[0] = 1;
        map.typemap[2] = 3;

        // Write to buffer
        let mut buf = Cursor::new(Vec::new());
        map.write(&mut buf).unwrap();

        // Read back
        buf.set_position(0);
        let loaded = SmfMap::read(&mut buf).unwrap();

        // Verify header
        assert_eq!(loaded.header.map_x, 128);
        assert_eq!(loaded.header.map_y, 128);

        // Verify heightmap (u16 quantization allows small error)
        let orig_val = map.heightmap.get(0, 0).unwrap();
        let loaded_val = loaded.heightmap.get(0, 0).unwrap();
        assert!((orig_val - loaded_val).abs() < 0.001);

        // Verify metalmap
        assert_eq!(loaded.metalmap[0], 128);
        assert_eq!(loaded.metalmap[1], 255);

        // Verify typemap
        assert_eq!(loaded.typemap[0], 1);
        assert_eq!(loaded.typemap[2], 3);

        // Verify SMT filename and tile indices are read back
        assert!(!loaded.smt_filename.is_empty());
        let (tiles_x, tiles_y) = loaded.header.tile_grid_size();
        let expected_indices = (tiles_x * tiles_y) as usize;
        assert_eq!(
            loaded.tile_indices.len(),
            expected_indices,
            "expected {expected_indices} tile indices ({}×{})",
            tiles_x,
            tiles_y
        );
        // Sequential tile indices
        for (i, &idx) in loaded.tile_indices.iter().enumerate() {
            assert_eq!(idx, i as i32, "tile index {i} mismatch");
        }
    }

    #[test]
    fn test_smf_feature_roundtrip() {
        let mut map = SmfMap::new(128, 128).unwrap();
        map.features = vec![
            SmfFeaturePlacement {
                feature_type: "arborreal".to_string(),
                x: 512.0,
                y: 0.0,
                z: 768.0,
                angle: std::f32::consts::FRAC_PI_2,
                taken_damage: 0,
            },
            SmfFeaturePlacement {
                feature_type: "GeoTherm_Lava_Rock".to_string(),
                x: 100.0,
                y: 0.0,
                z: 200.0,
                angle: 0.0,
                taken_damage: 5,
            },
            // Second arborreal -- type table must deduplicate
            SmfFeaturePlacement {
                feature_type: "arborreal".to_string(),
                x: 300.0,
                y: 0.0,
                z: 400.0,
                angle: std::f32::consts::PI,
                taken_damage: 0,
            },
        ];

        let mut buf = Cursor::new(Vec::new());
        map.write(&mut buf).unwrap();
        buf.set_position(0);
        let loaded = SmfMap::read(&mut buf).unwrap();

        assert_eq!(loaded.features.len(), 3);
        assert_eq!(loaded.features[0].feature_type, "arborreal");
        assert!((loaded.features[0].x - 512.0).abs() < 0.001);
        assert!((loaded.features[0].z - 768.0).abs() < 0.001);
        assert!((loaded.features[0].angle - std::f32::consts::FRAC_PI_2).abs() < 0.001);
        assert_eq!(loaded.features[1].feature_type, "GeoTherm_Lava_Rock");
        assert_eq!(loaded.features[1].taken_damage, 5);
        assert_eq!(loaded.features[2].feature_type, "arborreal");
        assert!((loaded.features[2].angle - std::f32::consts::PI).abs() < 0.001);
    }

    #[test]
    fn test_invalid_magic() {
        let mut buf = Cursor::new(vec![0u8; 100]);
        let result = SmfHeader::read(&mut buf);
        assert!(result.is_err());
    }
}

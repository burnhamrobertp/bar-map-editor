# bar-data crate instructions

Binary format parsing for BAR map data. No GPU, no I/O beyond reading bytes.

## Modules

| File | Purpose |
|---|---|
| `heightmap.rs` | `Heightmap` -- f32 grid, get/set, resize |
| `color.rs` | `ColorBuffer` -- RGBA u8 grid |
| `sd7.rs` | SMF/SMT binary format reader/writer |
| `smt.rs` | SMT tile encoding (DXT1/BC1) -- also exposes `decode_dxt1_block` for one-block decoding (skybox.rs reuses it) |
| `s3o.rs` | S3O model parser |
| `skybox.rs` | DDS loaders -- cubemap (`load_dds_cubemap`, for `atmosphere.skyBox`) and 2D (`load_dds_2d`, for splat / detail / sky-reflect textures via `viewport::load_2d_image`) |

## S3O parser (`s3o.rs`)

`parse_s3o(data: &[u8]) -> Result<S3oMesh, S3oError>` reads Spring's S3O binary model
format. The output is a flat merged mesh -- the piece hierarchy is traversed recursively
and all piece geometry is concatenated with world-space offsets applied. No articulation
is preserved; this is intentional for a placement previewer.

`S3oMesh`:
- `vertices: Vec<S3oVertex>` -- position/normal/uv, 32 bytes each
- `indices: Vec<u32>` -- triangle list
- `aabb_min / aabb_max: [f32; 3]` -- bounding box in local space
- `texture1: String` -- diffuse + team-mask filename declared in the header,
  empty if the header offset was 0. Caller prefixes `unittextures/` and looks
  the file up in the same archive sources used to load the model. Recoil falls
  through alternate extensions (.dds/.tga/.png) when the declared one is not
  present, and the bar-app loader mirrors that behavior.
- `texture2: String` -- secondary texture (color2 + glow/specular), unused by
  the current feature renderer but extracted for completeness.

`S3oVertex` is `bytemuck::Pod + bytemuck::Zeroable` (32 bytes):
- `position: [f32; 3]`
- `normal: [f32; 3]`
- `uv: [f32; 2]`

Primitive types: triangles (0), tristrip (1), quads (2). Tristrips and quads are
expanded to triangle lists by `expand_tristrip` / `expand_quads`.

Header is 52 bytes; root piece offset is at bytes 36-39. Each piece struct is also
52 bytes with world-space offset at bytes 40/44/48 (x/y/z f32).

Degenerate triangles (tristrip end caps with repeated indices) are silently dropped.
Models that fail to parse (bad magic, truncated data) return `S3oError`.

## DDS loaders (`skybox.rs`)

Two entry points, both backed by the same pixel-format / block-decode
helpers:

`load_dds_cubemap(path) -> Result<Cubemap, SkyboxError>` -- 6-face
cubemap loader for `atmosphere.skyBox`. Rejects DDS files without the
`CUBEMAP` caps2 flag. Calls `Dds::get_data(face_idx)` per face -- *not*
`get_data(0)` once, because the latter returns the mip chain for face 0
only (a footgun: mipmapped cubemaps reject as `UnsupportedFormat` otherwise).

`load_dds_2d(path) -> Result<(Vec<u8>, u32, u32), SkyboxError>` -- 2D
loader for splat distribution, splat detail-normals, sky reflection mask,
and most map detail textures. Decodes `Dds::get_data(0)` directly with no
cubemap-cap check. **Required because `image` v0.25 dropped DDS from its
default features**: without this entry point those files don't decode at
all, and the renderer's splat-detail-normal / detail / sky-reflect paths
silently stay on their 1x1 defaults.

Supported pixel formats (both loaders):
- Uncompressed: `A8R8G8B8`, `X8R8G8B8` (BGRA order), `A8B8G8R8`,
  `X8B8G8R8` (RGBA order), `R8G8B8`, DXGI `R8G8B8A8_UNorm[_sRGB]`
- Block-compressed: `DXT1`/`BC1` and `DXT3`/`DXT5`/`BC3` (colour
  block only; alpha block discarded). Block decoding reuses
  `smt::decode_dxt1_block`.

`Cubemap` carries 6 face buffers in wgpu order (+X, -X, +Y, -Y, +Z, -Z).

## Future: SDP reader

For engine-shipped assets (foam textures, caustic animation,
waverand) we'll need a Spring Pool format reader -- planned location
`crates/bar-data/src/sdp.rs`. See `docs/recoil-shader-ports.md`
("Prerequisite: SDP reader") for the format details and which
deferred shader features unblock once it lands. Not implemented yet.

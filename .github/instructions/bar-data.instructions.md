# bar-data crate instructions

Binary format parsing for BAR map data. No GPU, no I/O beyond reading bytes.

## Modules

| File | Purpose |
|---|---|
| `heightmap.rs` | `Heightmap` -- f32 grid, get/set, resize |
| `color.rs` | `ColorBuffer` -- RGBA u8 grid |
| `sd7.rs` | SMF/SMT binary format reader/writer |
| `smt.rs` | SMT tile encoding (DXT1/BC1) |
| `s3o.rs` | S3O model parser |

## S3O parser (`s3o.rs`)

`parse_s3o(data: &[u8]) -> Result<S3oMesh, S3oError>` reads Spring's S3O binary model
format. The output is a flat merged mesh -- the piece hierarchy is traversed recursively
and all piece geometry is concatenated with world-space offsets applied. No articulation
is preserved; this is intentional for a placement previewer.

`S3oMesh`:
- `vertices: Vec<S3oVertex>` -- position/normal/uv, 32 bytes each
- `indices: Vec<u32>` -- triangle list
- `aabb_min / aabb_max: [f32; 3]` -- bounding box in local space

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

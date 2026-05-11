# BAR - Map Editor

<div align="center">
  <img src="assets/bar-map-editor.png" alt="BAR - Map Editor" width="200">
</div>

A standalone map editor for **Beyond All Reason** (Spring/Recoil engine),
consolidating into a single application what currently requires a suite of
separate tools — Springboard, image editors, manual `mapinfo.lua` edits,
manual `.sd7` packaging, separate start-position tools, and so on.

The goal is the editor BAR has always lacked: WC3/SC2-editor-style end-to-end
workflow without ever having to launch the game until you're ready to playtest.

## Features

- **Full `.sd7` import / export** — heightmap, metalmap, typemap, minimap,
  Spring SMF/SMT binary I/O, `mapinfo.lua` generation, archive packaging
- **Node graph for procedural generation** — 34 node types (noise, erosion,
  blend, mask, splat, texture); subgraph nesting; compose into a DAG
- **GPU-accelerated preview** — compute shaders for noise and erosion;
  engine-faithful terrain renderer (Recoil sky, ground, and water shaders);
  GPU vertex displacement; animated water; CPU fallback when no GPU
- **Brush-based sculpting** — raise, lower, smooth, flatten; color, metalmap,
  typemap paint layers; live 3D viewport feedback; edits persist in project
- **Structured mapinfo editor** — form-based UI for all `mapinfo.lua` fields;
  start-position placement on the 2D inspector
- **Pre-export validation** — checks spawn positions, layer dimensions,
  mapinfo references, height range, and archive paths before allowing export
- **One-click "Test in BAR"** — exports project, drops SD7 into BAR maps
  dir, and spawns the lobby; detects install automatically
- **Self-contained project format** — single `.barproj` JSON + sibling
  `.assets/` directory; portable, version-controllable; full undo/redo

See [ROADMAP.md](ROADMAP.md) for current status, in-progress gaps, and
upcoming work.

### Explicitly NOT a goal
- Embedding or running the BAR/Recoil game inside this app — selectively
  using engine source code as reference is the integration path
- Catering to non-BAR Spring/Recoil games — the architecture stays
  pluggable but multi-game work is deferred

## Building

### Prerequisites

- **Rust toolchain** (stable, 1.75+) — install via [rustup](https://rustup.rs/);
  this provides both `rustc` and `cargo`. No other package manager step is
  needed; cargo handles every Rust dependency listed in `Cargo.toml`.
- GPU drivers supporting Vulkan (Linux/Windows), Metal (macOS), or DX12
  (Windows). `wgpu` selects the backend at runtime.
- Linux only: a few system dev packages for the file dialog / windowing layer:
  `libgtk-3-dev libxkbcommon-dev libwayland-dev libxcb-shape0-dev libxcb-xfixes0-dev`.

### Install Rust (one-time)

```bash
# Linux / macOS
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Windows: download and run https://win.rustup.rs/x86_64
# Restart the shell so `cargo` lands on PATH.
```

### Build & Run

The workspace ships two binaries: `bar-editor` (the GUI) and `bar-cli` (a headless CLI).

```bash
# Run the GUI (debug build -- fast compile, slower runtime)
cargo run --bin bar-editor

# Run the GUI (release build -- slower compile, fast runtime)
cargo run --release --bin bar-editor

# Run the CLI
cargo run --bin bar-cli -- --help
```

### Run Tests

```bash
cargo test
```

## Architecture

bar-editor is a Rust workspace of small, focused crates with strict
dependency boundaries:

| Crate | Responsibility |
|-------|---------------|
| `bar-data` | Heightmap buffers, .sd7 I/O, image formats |
| `bar-compute` | GPU compute shaders, CPU fallback noise/erosion |
| `bar-graph` | Node graph DAG evaluation engine |
| `bar-project` | `.barproj` file format, recipe schema, validation |
| `bar-engine` | Export pipeline (SD7 / SMF / SMT packing, codecs) |
| `bar-render` | 3D terrain viewport rendering (wgpu) |
| `bar-gui` | egui-based node editor + UI; i18n via `language/` |
| `bar-app` | Application binary (`bar-editor`); session lifecycle |
| `bar-cli` | CLI binary (`bar-cli`); headless eval, SD7 export, preview |

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

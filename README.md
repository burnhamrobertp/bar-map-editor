# BAR - Map Editor

<div align="center">
  <img src="assets/bar.png" alt="BAR - Map Editor" width="200">
</div>

A standalone map editor for **Beyond All Reason** (Spring/Recoil engine),
consolidating into a single application what currently requires a suite of
separate tools — Springboard, image editors, manual `mapinfo.lua` edits,
manual `.sd7` packaging, separate start-position tools, and so on.

The goal is the editor BAR has always lacked: WC3/SC2-editor-style end-to-end
workflow without ever having to launch the game until you're ready to playtest.

## Features

### Working today
- **Full `.sd7` import / export** — heightmap, metalmap, typemap, minimap,
  Spring SMF/SMT binary I/O, archive packaging
- **Node graph for procedural generation** — compose noise, erosion, blend,
  and filter nodes into a DAG that evaluates to a heightmap
- **GPU-accelerated** — compute shaders for noise + erosion via wgpu;
  CPU fallback when no GPU
- **`mapinfo.lua` honored** — `smf.minheight`/`maxheight` overrides applied
  on import so the preview matches the engine's interpretation
- **Self-contained project format** — single `.barproj` JSON + sibling
  `.assets/` directory; portable, version-controllable

### In progress (v0.2 pivot — see plan)
- Streamlined preset-driven generation (less configurability, stronger
  defaults; node graph remains as the "advanced" tier)
- Brush-based heightmap / metalmap / typemap sculpting in the editor
- Structured `mapinfo.lua` editor (forms instead of raw text)
- Visual start-position placement on a 2D inspector
- Pre-export semantic validation
- One-click "Test in BAR" launcher
- Engine-fidelity rendering by porting Recoil's actual map shaders rather
  than inventing parallel implementations

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

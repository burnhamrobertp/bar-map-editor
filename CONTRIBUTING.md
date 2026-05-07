# Contributing to bar-editor

Thank you for your interest in contributing! **bar-editor** is a standalone
map editor for Beyond All Reason (Spring/Recoil engine), consolidating the
full BAR map-creation workflow — procedural generation, sculpting, mapinfo
editing, validation, and `.sd7` packaging — into a single application.

## Table of Contents

- [Getting Started](#getting-started)
- [Building](#building)
- [Running Tests](#running-tests)
- [Project Structure](#project-structure)
- [Code Style](#code-style)
- [Submitting Changes](#submitting-changes)
- [Architecture Notes](#architecture-notes)

---

## Getting Started

### Prerequisites

| Requirement | Minimum version |
|---|---|
| **Rust toolchain** (provides `cargo` and `rustc`) | 1.76 (stable) |
| `wgpu`-capable GPU | Any Vulkan/Metal/DX12 GPU |
| Linux system packages | `libgtk-3-dev libxkbcommon-dev libwayland-dev` |

`cargo` ships with the Rust toolchain — installing Rust is the only
dependency-install step. Cargo resolves and downloads every crate
listed in `Cargo.toml` on first build.

Install Rust with [rustup](https://rustup.rs/):

```sh
# Linux / macOS
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Windows: download and run https://win.rustup.rs/x86_64
```

Clone the repository:

```sh
git clone https://github.com/burnhamrobertp/bar-map-editor.git
cd bar-map-editor
```

---

## Building

**Debug build** (faster compile, includes debug info):

```sh
cargo build
```

**Release build** (optimized — use this to benchmark):

```sh
cargo build --release
```

**Run the GUI application:**

```sh
cargo run --bin bar-editor
```

**Run the CLI tool:**

```sh
cargo run --bin bar-cli -- --help
```

### Linux dependency installation

```sh
sudo apt-get install -y libgtk-3-dev libxkbcommon-dev libwayland-dev \
  libxcb-shape0-dev libxcb-xfixes0-dev
```

---

## Running Tests

```sh
cargo test
```

Run tests for a specific crate:

```sh
cargo test -p bar-engine
```

Run benchmarks (requires nightly or `cargo bench`):

```sh
cargo bench
```

---

## Project Structure

```
bar-map-editor/
├── crates/
│   ├── bar-data/        # Core data types (HeightMap, layer buffers)
│   ├── bar-graph/       # Node graph engine (nodes, evaluation, connections)
│   ├── bar-engine/      # Export pipeline (SD7/SMF/SMT packing, codecs)
│   ├── bar-render/      # GPU terrain rendering (wgpu, terrain mesh, water)
│   ├── bar-project/     # Project file format (.barproj save/load, recipe schema)
│   ├── bar-app/         # Application shell (session, preview thread, SD7 extract)
│   ├── bar-gui/         # egui-based GUI (node editor canvas, properties panel)
│   └── bar-cli/         # CLI binary (`bar-cli` command)
├── shaders/            # WGSL shaders (terrain, water, erosion compute)
│   └── recoil/         # WGSL ports of vendored Recoil GLSL (GPL-3)
├── assets/             # Icons, textures, preset files
│   ├── macros/         # Drop-and-tune macro templates surfaced in Welcome
│   └── presets/        # Built-in `.barproj` terrain preset projects
├── language/           # i18n source (en/common.json, en/editor.json)
├── vendor/recoil/      # Pinned upstream Recoil shaders (never edited)
├── installer/          # Platform installer scripts (NSIS, AppImage, .deb)
├── .github/
│   ├── workflows/      # CI/CD (build, test, release)
│   └── instructions/   # Per-crate Copilot instructions (architecture notes)
└── benches/            # Criterion benchmarks
```

Each crate has a `.github/instructions/<crate>.instructions.md` document describing its responsibilities, data ownership, and interaction boundaries.

---

## Code Style

- **Rust edition 2021** throughout.
- Format with `rustfmt` (configuration in `rustfmt.toml`): `cargo fmt`
- Lint with `clippy` (configuration in `clippy.toml`): `cargo clippy`
- Comments only where clarification is needed; avoid restating the obvious.
- Use `anyhow::Result` for fallible functions at the crate boundary; use typed errors internally where appropriate.
- Prefer flat module hierarchies — avoid deep nesting.

### Naming conventions

| Item | Convention |
|---|---|
| Types, traits | `PascalCase` |
| Functions, methods | `snake_case` |
| Constants | `SCREAMING_SNAKE_CASE` |
| Module files | `snake_case.rs` |

---

## Submitting Changes

1. **Fork** the repository and create a feature branch:
   ```sh
   git checkout -b my-feature
   ```

2. **Make your changes**, keeping commits focused and logical.

3. **Run tests and formatting** before pushing:
   ```sh
   cargo fmt && cargo clippy && cargo test
   ```

4. **Open a pull request** against `develop` (the active integration branch).
   `master` is reserved for release commits. Describe what the change does and
   why.

### PR expectations

- All CI checks must pass (build + tests on Linux, Windows, macOS).
- New features should include tests where practical.
- Breaking changes to the project file format (`.barproj`) must increment `PROJECT_VERSION` and add a migration or clear error.
- GPU-only paths (shaders, `bar-render`, compute pipelines) should degrade gracefully when a compatible GPU is unavailable.

---

## Architecture Notes

bar-editor is organized as a workspace of small, focused crates with strict dependency boundaries:

```
bar-data → bar-graph → bar-engine → bar-app → bar-gui
                               ↘ bar-cli
bar-render ← bar-app
bar-project ← bar-engine, bar-gui
```

**Key invariants:**

- `bar-data` and `bar-project` have no runtime dependencies — only standard library and `serde`.
- `bar-graph` knows nothing about rendering or file I/O.
- `bar-render` is a pure GPU rendering layer; it receives a `HeightMap` and map settings, never touching the node graph directly.
- The GUI (`bar-gui`) calls into `bar-app` through a thin command/event interface — it does not hold a reference to the engine or render context.
- Session state (project, preview thread, SD7 extraction) is owned by `bar-app`; the GUI holds only editor visual state.

See the individual `.github/instructions/*.instructions.md` files for per-crate detail.

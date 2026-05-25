# Linux build and runtime requirements

This file captures the Linux-side build + runtime story for `bar-editor`,
validated under WSL2 (WSLg display + software Vulkan via lavapipe) on
two host distros. The maintainer develops on Windows; everything here
came out of cross-checking the editor on Linux to make sure the binary
artifacts the release workflow produces will run for Linux testers.

## What's needed on each distro

| Distro          | Build prerequisites                 | Runtime libraries (for `bar-app` GUI) |
| --------------- | ----------------------------------- | -------------------------------------- |
| Ubuntu 24.04    | `build-essential` (default toolchain) + `rustup` | None beyond the WSLg defaults |
| Arch Linux      | `base-devel rust`                   | `wayland libxkbcommon vulkan-icd-loader mesa` |

Both distros build the entire workspace (`bar-cli`, `bar-app`, all
library crates) without any further system packages. The CLI workflow
(`import` + `run --target spring-smf`) runs headless and needs nothing
beyond the build toolchain on either distro.

The GUI workflow runtime requirements are environment-dependent: WSLg
ships its own Wayland compositor + display server, but only Ubuntu's
default install ships the matching client-side libraries. On Arch you
must install `wayland`, `libxkbcommon`, `vulkan-icd-loader`, and `mesa`
explicitly before `bar-app` can open a window. On a desktop Linux
install most environments include these via the desktop session.

## wgpu adapter limits

The editor used to ask wgpu for fixed limits (`max_storage_buffer_binding_size:
512 MB`, etc.). Software Vulkan stacks (lavapipe under WSLg without GPU
passthrough, some Mesa fallbacks, VMs) cap these much lower; the
request_device call would then fail with errors like:

    Limit 'max_storage_buffer_binding_size' value 536870912 is better than allowed 134217728

The current approach sources `required_limits` from `adapter.limits()`
verbatim and only caps a few memory-bound fields downward (storage
buffer / general buffer size). Runtime paths that need more headroom
(GPU erosion, large heightmaps) report a clean per-call error via
`check_buffer_size` instead of crashing at startup. See
`crates/bar-app/src/main.rs` and `crates/bar-compute/src/device.rs`.

## WSL2-specific notes

- WSLg exposes both X11 (`DISPLAY=:0`) and Wayland (`WAYLAND_DISPLAY=wayland-0`).
  `winit` prefers Wayland on Linux when available; this is fine.
- WSL2 without GPU passthrough drops to Mesa's `lavapipe` (software
  Vulkan, "llvmpipe (LLVM <version>)"). Adapter limits are noticeably
  lower than hardware. The wgpu setup above already accommodates this.
- WSL2 with NVIDIA's WSL driver or Intel/AMD vGPU passthrough exposes
  hardware Vulkan; nothing in BME needs to change in that case.
- `libEGL warning: failed to get driver name for fd -1` and
  `MESA: error: ZINK: vkCreateInstance failed` are background noise
  from Mesa's EGL/Zink probes. They appear during startup and do not
  block the application -- wgpu uses the native Vulkan ICD directly.

## CLI smoke test (works on both Ubuntu and Arch)

The full import + recompile round-trip:

```bash
cargo run -p bar-cli -- import path/to/yourmap.sd7 -o ./imported
cargo run -p bar-cli -- run \
    ./imported/yourmap.barproj/recipe.json \
    --target spring-smf \
    -o ./recompiled
```

This produces `maps/yourmap.smf`, `maps/yourmap.smt`, and `mapinfo.lua`
under `./recompiled/`. The output is deterministic across distros: SMF
and SMT byte sizes match between Ubuntu 24.04 and Arch on the same
input.

## CARGO_TARGET_DIR when sharing a /mnt/c checkout

When building under WSL against a Windows-side repo checkout, point
`CARGO_TARGET_DIR` somewhere Linux-side so the Linux build artifacts
don't share a `target/` directory with the Windows builds. The two
toolchains produce incompatible artifacts and clobber each other's
build caches if they share.

```bash
export CARGO_TARGET_DIR=$HOME/.cargo-target-om
```

## Distro coverage

The CI release workflow produces a `.deb` (Debian/Ubuntu) and an
AppImage (any glibc-based distro). The Arch validation above gives us
informal coverage of the AppImage path: an Arch user can run the
AppImage as long as they have `wayland`, `libxkbcommon`,
`vulkan-icd-loader`, and `mesa` installed.

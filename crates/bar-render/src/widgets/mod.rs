//! BAR LuaUI widget effect ports.
//!
//! Each submodule corresponds to one widget BAR ships that's driven
//! by `mapinfo.custom.*` per-map authored content. Widget effects
//! live here -- not in the engine-native pipeline modules -- so the
//! engine-faithful renderer stays clean and widget effects can be
//! added, removed, or updated independently as BAR ships changes.
//! See `feedback_no_game_widget_porting.md` memory for the
//! in-scope-vs-out-of-scope rule.
//!
//! Convention:
//!
//! - Each widget owns its own state struct (e.g. `CustomFogWidget`).
//! - The widget builds from `MapSettings` via `from_settings`.
//! - The widget exposes pack methods that produce uniform-ready
//!   `[f32; 4]` slots; the renderer composes those into its
//!   `CameraUniform` or per-pass uniform buffers.
//! - The shader half lives in `shaders/widgets/<name>.wgsl`.

pub mod custom_fog;
pub mod map_grass;

//! Design tokens — single source of truth for named colours in the editor.
//!
//! Scattered `Color32::from_rgb(…)` literals that represent a semantic role
//! ("the error colour", "the selected-node border") belong here. Inline
//! literals remain acceptable for computed one-off colours (e.g. topo-gradient
//! lerps in the inspector heightmap view) where no semantic name applies.

use eframe::egui::Color32;

// ── Port kind colours ────────────────────────────────────────────────────────

pub const PORT_HEIGHTMAP: Color32 = Color32::from_rgb(100, 200, 100);
pub const PORT_MASK: Color32 = Color32::from_rgb(200, 200, 100);
pub const PORT_COLOR: Color32 = Color32::from_rgb(180, 100, 220);
pub const PORT_SCALAR: Color32 = Color32::from_rgb(150, 150, 200);
pub const PORT_FILE: Color32 = Color32::from_rgb(200, 160, 80);
pub const PORT_FILE_LIST: Color32 = Color32::from_rgb(180, 140, 60);
pub const PORT_CONTROL: Color32 = Color32::from_rgb(90, 170, 230);
pub const PORT_DENSITY: Color32 = Color32::from_rgb(230, 130, 200);

// ── Node category title-bar colours ─────────────────────────────────────────

pub const NODE_CAT_GENERATOR: Color32 = Color32::from_rgb(80, 140, 80);
pub const NODE_CAT_FILTER: Color32 = Color32::from_rgb(140, 100, 60);
pub const NODE_CAT_COMBINER: Color32 = Color32::from_rgb(80, 100, 160);
pub const NODE_CAT_TEXTURE: Color32 = Color32::from_rgb(140, 80, 160);
pub const NODE_CAT_MASK: Color32 = Color32::from_rgb(120, 120, 60);
pub const NODE_CAT_BUNDLER: Color32 = Color32::from_rgb(200, 130, 50);
pub const NODE_CAT_SOURCE: Color32 = Color32::from_rgb(50, 160, 160);
pub const NODE_CAT_PREVIEW: Color32 = Color32::from_rgb(150, 150, 200);
pub const NODE_CAT_IO: Color32 = Color32::from_rgb(40, 110, 130);

// ── Node body state colours ──────────────────────────────────────────────────

pub const NODE_BG: Color32 = Color32::from_rgb(45, 50, 60);
/// In multi-selection (not the primary).
pub const NODE_BG_SEL: Color32 = Color32::from_rgb(55, 62, 76);
/// Primary selection (properties panel open).
pub const NODE_BG_PRI: Color32 = Color32::from_rgb(70, 80, 100);
pub const NODE_BORDER: Color32 = Color32::from_rgb(80, 85, 95);
pub const NODE_BORDER_SEL: Color32 = Color32::from_rgb(100, 160, 255);

// ── IO node colours ──────────────────────────────────────────────────────────

pub const IO_BODY: Color32 = Color32::from_rgb(0x2F, 0x39, 0x45);
pub const IO_BORDER: Color32 = Color32::from_rgb(0x4A, 0x55, 0x63);
pub const IO_BORDER_SEL: Color32 = Color32::from_rgb(0x4F, 0xD1, 0xC5);
pub const IO_LABEL_PRI: Color32 = Color32::from_rgb(0xE6, 0xED, 0xF3);
pub const IO_LABEL_SEC: Color32 = Color32::from_rgb(0x9A, 0xA7, 0xB2);

// ── Canvas tab strip ─────────────────────────────────────────────────────────

pub const TAB_BG_ACTIVE: Color32 = Color32::from_rgb(45, 50, 60);
pub const TAB_BG_INACTIVE: Color32 = Color32::from_rgb(28, 32, 40);
pub const TAB_BG_HOVER: Color32 = Color32::from_rgb(55, 60, 70);
pub const TAB_BASELINE: Color32 = Color32::from_rgb(60, 65, 75);
pub const TAB_LABEL_ACTIVE: Color32 = Color32::from_rgb(230, 230, 240);
pub const TAB_LABEL_DIM: Color32 = Color32::from_rgb(160, 165, 180);

// ── Validation / severity ────────────────────────────────────────────────────

pub const SEVERITY_ERROR: Color32 = Color32::from_rgb(220, 80, 80);
pub const SEVERITY_WARN: Color32 = Color32::from_rgb(220, 180, 80);
pub const SEVERITY_INFO: Color32 = Color32::from_rgb(120, 160, 220);

// ── Wire / connection colours ────────────────────────────────────────────────

/// Selected wire — same accent as NODE_BORDER_SEL.
pub const WIRE_SELECTED: Color32 = Color32::from_rgb(100, 160, 255);
/// Default (unselected) wire colour. All wires share one colour for now;
/// port-kind-coloured wires would use port_kind_color() per connection.
pub const WIRE_DEFAULT: Color32 = Color32::from_rgb(100, 200, 100);
/// In-progress drag wire (Mask port yellow).
pub const WIRE_DRAG: Color32 = Color32::from_rgb(200, 200, 100);

// ── Miscellaneous UI ─────────────────────────────────────────────────────────

/// Stroke colour for toolbar icons and IO-node directional icons.
pub const ICON_STROKE: Color32 = Color32::from_rgb(0xE6, 0xED, 0xF3);
/// Dark canvas / inspector background panel.
pub const CANVAS_BG: Color32 = Color32::from_rgb(28, 35, 50);
/// Gold ring drawn around the active colour swatch in the group colour picker.
pub const SWATCH_RING: Color32 = Color32::from_rgb(255, 220, 120);

pub const PALETTE_HOVER: Color32 = Color32::from_rgb(55, 60, 80);
pub const PALETTE_IDLE: Color32 = Color32::from_rgb(48, 53, 70);
pub const PALETTE_TEXT: Color32 = Color32::from_rgb(190, 190, 200);

// ── Toolbar button bg states ─────────────────────────────────────────────────
// Each toolbar button has its own semantic hue. Three interaction states each:
// normal / hover / press. Buttons that can be "blocked" (running) get a fourth.

pub const BTN_EXPORT_NORMAL: Color32 = Color32::from_rgb(35, 110, 50);
pub const BTN_EXPORT_HOVER: Color32 = Color32::from_rgb(48, 132, 62);
pub const BTN_EXPORT_PRESS: Color32 = Color32::from_rgb(22, 80, 36);
pub const BTN_EXPORT_BUSY: Color32 = Color32::from_rgb(80, 80, 30);
pub const BTN_EXPORT_BLOCKED: Color32 = Color32::from_rgb(40, 60, 40);

pub const BTN_MAPINFO_NORMAL: Color32 = Color32::from_rgb(55, 75, 125);
pub const BTN_MAPINFO_HOVER: Color32 = Color32::from_rgb(72, 96, 150);
pub const BTN_MAPINFO_PRESS: Color32 = Color32::from_rgb(40, 60, 105);

pub const BTN_BAR_NORMAL: Color32 = Color32::from_rgb(125, 75, 38);
pub const BTN_BAR_HOVER: Color32 = Color32::from_rgb(150, 90, 50);
pub const BTN_BAR_PRESS: Color32 = Color32::from_rgb(110, 65, 30);
pub const BTN_BAR_BLOCKED: Color32 = Color32::from_rgb(70, 50, 30);

pub const BTN_INSPECTOR_NORMAL: Color32 = Color32::from_rgb(60, 105, 85);
pub const BTN_INSPECTOR_HOVER: Color32 = Color32::from_rgb(80, 130, 105);
pub const BTN_INSPECTOR_PRESS: Color32 = Color32::from_rgb(50, 90, 70);

pub const BTN_MAPSET_NORMAL: Color32 = Color32::from_rgb(95, 75, 125);
pub const BTN_MAPSET_HOVER: Color32 = Color32::from_rgb(120, 90, 150);
pub const BTN_MAPSET_PRESS: Color32 = Color32::from_rgb(80, 60, 95);

pub const BTN_SPAWNS_NORMAL: Color32 = Color32::from_rgb(75, 100, 125);
pub const BTN_SPAWNS_HOVER: Color32 = Color32::from_rgb(95, 120, 150);
pub const BTN_SPAWNS_PRESS: Color32 = Color32::from_rgb(60, 80, 95);

// ── Param slider ─────────────────────────────────────────────────────────────
pub const SLIDER_BG: Color32 = Color32::from_rgb(28, 30, 40);
pub const SLIDER_FILL: Color32 = Color32::from_rgb(55, 95, 165);
pub const SLIDER_HANDLE: Color32 = Color32::from_rgb(130, 175, 235);
pub const SLIDER_TEXT: Color32 = Color32::from_rgb(220, 220, 230);

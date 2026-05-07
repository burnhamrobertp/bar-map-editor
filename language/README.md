# Localization

This directory holds translation source files for the bar-editor.

## Layout

```
language/
  en/
    common.json     # Strings shared with other BAR applications.
                    # Top-level JSON key is `"common"`. Add only
                    # genuinely cross-app strings here (Cancel,
                    # Save, OK, etc.) — anything specific to the
                    # editor goes in `editor.json`.
    editor.json     # Strings specific to the bar-editor.
                    # Top-level JSON key is `"editor"`.
```

The directory layout (`language/<locale>/<namespace>.json`) and JSON
format match the upstream `bar-localizations` repo so files round-trip
through that aggregator without a transform step. Interpolation uses
`%{var}` — same convention as bar-game and bar-lobby.

At runtime, rust-i18n auto-detects the multi-file layout, merges all
JSON files under each locale, and exposes the nested keys as
`t!("editor.menu.file")` / `t!("common.cancel")`.

## Why two files (and namespaces)

`common.json` is the seam for sharing strings with other BAR apps.
The maintainer's `bar-localizations` repo aggregates per-namespace
JSON across BAR's tools so a translator localising "Cancel" once
covers every app. App-specific strings stay in `editor.json` so
changes there don't ripple through other apps' release cycles.

The `"common"` / `"editor"` top-level JSON keys produce the runtime
namespacing — `t!("common.cancel")` and `t!("editor.menu.file")`.

## Adding a string

1. Pick the right file:
   - `common.json` (under `"common"`) for genuinely cross-app strings.
   - `editor.json` (under `"editor"`) for editor-specific UI.
2. Add the English value with a stable, descriptive key. Keep keys
   alphabetical within their group so diffs stay clean.
3. Reference it from Rust as `t!("editor.welcome.heading")` or
   `t!("common.cancel")`.

For interpolated values use `%{var}` and pass the var:

```json
{
  "editor": {
    "notify": {
      "template_started": "Started a new project with the '%{name}' template."
    }
  }
}
```
```rust
let msg = t!("editor.notify.template_started", name = template_name);
```

## Adding a language

Add a sibling locale folder, e.g. `language/de/`, mirroring the
`en/` layout: `de/common.json` and `de/editor.json`. rust-i18n
auto-loads it. Switch the active locale at startup or runtime via
`rust_i18n::set_locale("de")`.

## Sync with `bar-localizations`

This directory's structure (`language/<locale>/<namespace>.json`,
`%{var}` interpolation) is identical to `bar-localizations`, so the
upstream/downstream sync workflows in that repo can round-trip these
files without a format transform. Editor-owned namespace is
`editor`; `common` is shared and will need namespace ownership
arbitration upstream when adopted.

Do **not** edit non-English locale files in-tree by hand once the
sync flow is live — round-trip them through `bar-localizations`.

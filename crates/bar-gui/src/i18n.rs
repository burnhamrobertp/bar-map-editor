//! Translation lookup.
//!
//! The `language/` tree is embedded at compile time via `include_dir`
//! and parsed once at startup. Layout matches `bar-localizations`:
//!
//! ```text
//! language/<locale>/<namespace>.json
//! ```
//!
//! Each JSON file's keys are merged into the locale's flat catalogue
//! using dotted-path notation, so `editor.menu.file` looks up the
//! string `"File"` from the `editor` namespace. Interpolation uses
//! `%{var}` to match bar-game and bar-lobby's convention.
//!
//! ## Why not rust-i18n
//!
//! rust-i18n's default loader treats the file stem as the locale
//! name. With our subdirectory layout that means `editor.json`
//! becomes locale "editor", not namespace "editor" under locale
//! "en". rust-i18n 3.x has no public API to override the backend
//! after compile-time generation, so we ship our own — small enough
//! that the dependency wasn't carrying its weight.

use include_dir::{include_dir, Dir};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// Compile-time copy of `language/`. Anchored to this crate's
/// manifest so the binary, library tests, and downstream consumers
/// all see the same translations regardless of CWD.
static LANGUAGE: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../language");

/// Loaded translations, populated by [`init`]. `locale → key → value`.
static CATALOGUE: OnceLock<HashMap<String, HashMap<String, String>>> = OnceLock::new();

/// Currently active locale — the one [`t`] looks up first. Changes
/// at runtime via [`set_locale`].
static CURRENT_LOCALE: RwLock<&'static str> = RwLock::new("en");

/// Locale used as the fallback when a key is missing from the
/// current locale's catalogue.
const FALLBACK_LOCALE: &str = "en";

/// Parse the embedded `language/` tree into a flat catalogue and
/// install it as the active translation source. Idempotent and
/// thread-safe; the catalogue is built once and cached.
///
/// Call this exactly once near the top of `main` (the binary does
/// this) and from any test that needs `t!()` to resolve. Calling it
/// multiple times is harmless — the second invocation is a no-op.
pub fn init() {
    CATALOGUE.get_or_init(build_catalogue);
}

/// Switch the active locale. Future `t!()` lookups read from this
/// locale first, falling back to `en` for missing keys.
pub fn set_locale(locale: &'static str) {
    *CURRENT_LOCALE.write().expect("locale lock") = locale;
}

/// Look up a key in the active locale, falling back to English and
/// finally to the literal key when nothing matches. Returned string
/// has no `%{var}` substitution applied — use [`t_args`] for that
/// or the [`t!`] macro which dispatches automatically.
pub fn t(key: &str) -> String {
    let cat = CATALOGUE.get_or_init(build_catalogue);
    let active = *CURRENT_LOCALE.read().expect("locale lock");
    if let Some(s) = cat.get(active).and_then(|m| m.get(key)) {
        return s.clone();
    }
    if active != FALLBACK_LOCALE {
        if let Some(s) = cat.get(FALLBACK_LOCALE).and_then(|m| m.get(key)) {
            return s.clone();
        }
    }
    key.to_string()
}

/// Look up `key` and replace `%{name}` patterns with the supplied
/// values. Unknown patterns are left intact.
pub fn t_args(key: &str, args: &[(&str, &str)]) -> String {
    interpolate(&t(key), args)
}

/// `%{name}` → value. Stable when an arg is missing — the literal
/// `%{name}` stays in place rather than producing empty output, so
/// missing-arg bugs surface visibly during testing.
fn interpolate(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Look for the literal sequence `%{`
        if i + 1 < bytes.len() && bytes[i] == b'%' && bytes[i + 1] == b'{' {
            if let Some(close_rel) = template[i + 2..].find('}') {
                let close_abs = i + 2 + close_rel;
                let name = &template[i + 2..close_abs];
                if let Some((_, v)) = args.iter().find(|(k, _)| *k == name) {
                    out.push_str(v);
                    i = close_abs + 1;
                    continue;
                }
                // Unknown arg — pass the pattern through verbatim so
                // the bug is visible in the UI.
                out.push_str(&template[i..=close_abs]);
                i = close_abs + 1;
                continue;
            }
        }
        out.push(template[i..].chars().next().unwrap());
        i += template[i..].chars().next().unwrap().len_utf8();
    }
    out
}

fn build_catalogue() -> HashMap<String, HashMap<String, String>> {
    let mut catalogue: HashMap<String, HashMap<String, String>> = HashMap::new();

    for locale_dir in LANGUAGE.dirs() {
        let Some(locale) = locale_dir
            .path()
            .file_name()
            .and_then(|n| n.to_str())
        else {
            continue;
        };

        let entry = catalogue.entry(locale.to_string()).or_default();

        for file in locale_dir.files() {
            let ext = file.path().extension().and_then(|e| e.to_str());
            if !matches!(ext, Some("json")) {
                continue;
            }
            let Some(content) = file.contents_utf8() else {
                continue;
            };
            let value: Value = match serde_json::from_str(content) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse {}: {e}",
                        file.path().display()
                    );
                    continue;
                }
            };
            flatten_into("", &value, entry);
        }
    }

    catalogue
}

/// Recursive flatten: `{a: {b: "x"}}` → `{"a.b": "x"}`. Skips arrays
/// and bools — translation files only carry strings.
fn flatten_into(prefix: &str, v: &Value, out: &mut HashMap<String, String>) {
    match v {
        Value::Object(map) => {
            for (k, vv) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_into(&key, vv, out);
            }
        }
        Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        _ => {}
    }
}

/// Translation lookup macro. Two forms:
///
/// - `t!("editor.menu.file")` → resolves the key.
/// - `t!("editor.notify.template_started", name = "Plains")` →
///   resolves and interpolates `%{name}`.
///
/// Returned value is `String`. Drop-in replacement for rust-i18n's
/// `t!` so call sites read the same way.
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::t($key)
    };
    ($key:expr, $($name:ident = $val:expr),+ $(,)?) => {{
        let _args: &[(&str, &str)] = &[
            $( (stringify!($name), &$val.to_string()) ),+
        ];
        $crate::i18n::t_args($key, _args)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_nests() {
        let v: Value = serde_json::from_str(
            r#"{"a": {"b": "x", "c": {"d": "y"}}}"#,
        )
        .unwrap();
        let mut out = HashMap::new();
        flatten_into("", &v, &mut out);
        assert_eq!(out.get("a.b"), Some(&"x".to_string()));
        assert_eq!(out.get("a.c.d"), Some(&"y".to_string()));
    }

    #[test]
    fn t_resolves_editor_keys() {
        init();
        assert_eq!(t("editor.menu.file"), "File");
        assert_eq!(t("editor.welcome.heading"), "BAR - Map Editor");
        assert_eq!(t("editor.welcome.blank_project"), "Blank Project");
    }

    #[test]
    fn t_resolves_common_keys() {
        init();
        assert_eq!(t("common.cancel"), "Cancel");
        assert_eq!(t("common.save"), "Save");
    }

    #[test]
    fn t_falls_back_to_key_for_missing() {
        init();
        assert_eq!(t("editor.does.not.exist"), "editor.does.not.exist");
    }

    #[test]
    fn t_args_interpolates() {
        init();
        assert_eq!(
            t_args(
                "editor.notify.template_started",
                &[("name", "Plains")],
            ),
            "Started a new project with the 'Plains' template."
        );
    }

    #[test]
    fn t_args_unknown_var_is_visible() {
        let s = interpolate("Hello %{name}", &[("other", "x")]);
        assert_eq!(s, "Hello %{name}");
    }
}

//! Tiny Lua-table builder used by the mapinfo emitter.
//!
//! The bundled `mapinfo.lua` is constructed by chaining `LuaTable`
//! entries: one method call per field, with `None` values skipped at
//! the builder rather than the call site. The previous emitter pattern
//! was a wall of `if let Some(v) = X { out.push_str(format!(...)) }`
//! lines; adding a new mapinfo field meant copying the boilerplate and
//! getting the comma / indent / format conventions right per copy. This
//! builder takes those decisions once.
//!
//! Trade-off: the builder is intentionally minimal -- no nested tables,
//! no automatic sub-block flattening, no key-quoting modes beyond what
//! mapinfo needs. The point is to remove the per-field boilerplate from
//! the emitter, not to be a full Lua serializer.
//!
//! Format:
//! - One field per line, trailing comma.
//! - `key` printed unquoted (Lua identifier syntax).
//! - Indent is configured at constructor time (the codec uses 8 spaces
//!   for nested fields inside an 4-space subsection).
//! - `finish()` returns `None` when no fields were appended, so the
//!   caller can skip emitting an empty block.

/// Format an `f32` using Rust's default `Display`, which uses the
/// Ryū algorithm to produce the shortest decimal string that
/// round-trips bit-exactly to the same f32. So `0.71_f32` formats as
/// `"0.71"` (not `"0.7099999785"`), `1.0_f32` as `"1"`, and a value
/// like `0.58016002_f32` keeps just as many digits as the storage
/// representation actually needs.
pub fn fmt_f32(x: f32) -> String {
    x.to_string()
}

/// Escape a string for Lua double-quoted literal context.
pub fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Fluent builder for a single Lua sub-table emission. Use `opt_*`
/// methods to register fields; the builder skips any field whose
/// `Option` is `None`, so adding a new mapinfo key is one builder call
/// regardless of whether the recipe currently has it set.
pub struct LuaTable {
    body: String,
    field_indent: String,
    any: bool,
}

impl LuaTable {
    /// Construct an empty table builder. `field_indent` is the number
    /// of spaces before each child entry (i.e. the per-line indent of
    /// the entries this builder produces, not the outer `key = {`).
    pub fn new(field_indent: usize) -> Self {
        Self {
            body: String::new(),
            field_indent: " ".repeat(field_indent),
            any: false,
        }
    }

    /// Append `key = v,` when `value` is `Some(v)`. `Display`-formatted
    /// scalars (used for `u32`, etc.). Use [`opt_f32`] for floats so
    /// they format consistently with the rest of mapinfo.
    pub fn opt<T: std::fmt::Display>(&mut self, key: &str, value: Option<T>) -> &mut Self {
        if let Some(v) = value {
            self.body
                .push_str(&format!("{}{} = {},\n", self.field_indent, key, v));
            self.any = true;
        }
        self
    }

    /// Append `key = v,` formatted as a mapinfo float (trailing zeros
    /// stripped).
    pub fn opt_f32(&mut self, key: &str, value: Option<f32>) -> &mut Self {
        if let Some(v) = value {
            self.body
                .push_str(&format!("{}{} = {},\n", self.field_indent, key, fmt_f32(v)));
            self.any = true;
        }
        self
    }

    /// Append `key = { a, b, c, d },`.
    pub fn opt_vec4(&mut self, key: &str, value: Option<[f32; 4]>) -> &mut Self {
        if let Some(v) = value {
            self.body.push_str(&format!(
                "{}{} = {{ {}, {}, {}, {} }},\n",
                self.field_indent,
                key,
                fmt_f32(v[0]),
                fmt_f32(v[1]),
                fmt_f32(v[2]),
                fmt_f32(v[3]),
            ));
            self.any = true;
        }
        self
    }

    /// Append `key = { r, g, b },`.
    pub fn opt_vec3(&mut self, key: &str, value: Option<[f32; 3]>) -> &mut Self {
        if let Some(c) = value {
            self.body.push_str(&format!(
                "{}{} = {{ {}, {}, {} }},\n",
                self.field_indent,
                key,
                fmt_f32(c[0]),
                fmt_f32(c[1]),
                fmt_f32(c[2]),
            ));
            self.any = true;
        }
        self
    }

    /// Append `key = "value",`. Empty strings are treated as absent so
    /// the bundler doesn't write meaningless keys.
    pub fn opt_str(&mut self, key: &str, value: Option<&str>) -> &mut Self {
        if let Some(s) = value.filter(|s| !s.is_empty()) {
            self.body
                .push_str(&format!("{}{} = \"{}\",\n", self.field_indent, key, esc(s)));
            self.any = true;
        }
        self
    }

    /// Append `key = true,` / `key = false,` for explicit values;
    /// skip on `None`. Preserves the source mapinfo's intent: if the
    /// author wrote `voidWater = false` we round-trip it verbatim
    /// rather than dropping to "absent ≡ false" (the engine's
    /// internal default convention). A field the user never set
    /// stays absent.
    pub fn opt_bool(&mut self, key: &str, value: Option<bool>) -> &mut Self {
        if let Some(v) = value {
            self.body
                .push_str(&format!("{}{} = {},\n", self.field_indent, key, v));
            self.any = true;
        }
        self
    }

    /// Append `key = true,` only when value is `Some(true)`. Useful
    /// for inverse-flag forms like `notDeformable` where emitting
    /// `notDeformable = false` is redundant because the field's
    /// presence already implies inversion. Use [`opt_bool`] for
    /// fields where false is meaningful.
    #[allow(dead_code)]
    pub fn opt_bool_true_only(&mut self, key: &str, value: Option<bool>) -> &mut Self {
        if value == Some(true) {
            self.body
                .push_str(&format!("{}{} = true,\n", self.field_indent, key));
            self.any = true;
        }
        self
    }

    /// Append a verbatim sub-block (assumed to include its own trailing
    /// newline). Used for nested tables like `grassShaderParams = {...}`
    /// that this builder doesn't model directly.
    pub fn child(&mut self, sub: &str) -> &mut Self {
        if !sub.is_empty() {
            self.body.push_str(sub);
            self.any = true;
        }
        self
    }

    /// `true` when at least one field was appended.
    #[allow(dead_code)]
    pub fn has_entries(&self) -> bool {
        self.any
    }

    /// Render as a complete `key = { ... },` block at the parent's
    /// indent. Returns `None` when no fields were appended -- the
    /// caller skips emitting the wrapper entirely.
    pub fn finish_block(self, parent_indent: usize, name: &str) -> Option<String> {
        if !self.any {
            return None;
        }
        let parent = " ".repeat(parent_indent);
        Some(format!("{parent}{name} = {{\n{}{parent}}},\n", self.body))
    }

    /// Render as bare key=value lines (no surrounding `{ ... }`). Used
    /// for the physics block, which lives at mapinfo top level rather
    /// than under a named sub-table.
    pub fn finish_bare(self) -> Option<String> {
        if !self.any {
            return None;
        }
        Some(self.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_builder_returns_none() {
        let t = LuaTable::new(8);
        assert!(t.finish_block(4, "atmosphere").is_none());
    }

    #[test]
    fn single_field_renders_full_block() {
        let mut t = LuaTable::new(8);
        t.opt_f32("minWind", Some(3.0));
        let out = t.finish_block(4, "atmosphere").unwrap();
        assert_eq!(out, "    atmosphere = {\n        minWind = 3,\n    },\n");
    }

    #[test]
    fn none_fields_skipped() {
        let mut t = LuaTable::new(8);
        t.opt_f32("a", Some(1.0))
            .opt_f32("b", None)
            .opt_f32("c", Some(2.0));
        let out = t.finish_block(4, "x").unwrap();
        assert!(out.contains("a = 1"));
        assert!(out.contains("c = 2"));
        assert!(!out.contains("b ="));
    }

    #[test]
    fn fmt_f32_strips_trailing_zeros() {
        assert_eq!(fmt_f32(2.0), "2");
        assert_eq!(fmt_f32(0.5), "0.5");
        assert_eq!(fmt_f32(0.0), "0");
        assert_eq!(fmt_f32(0.0075), "0.0075");
    }
}

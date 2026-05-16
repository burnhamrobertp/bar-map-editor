//! FinalComposition layer asset helpers.
//!
//! FC owns four paintable layers (heightmap, color, metalmap, typemap),
//! each backed by a binary file at `<project>/final_composition/<id>.bin`.
//! Layer identity (the `asset_id` UUID) is minted at FC creation and
//! persists with the recipe; the file itself only comes into existence
//! on the user's first stroke into that kind.
//!
//! `mint_fc_layer_ids` is called at FC instantiation (scan, new project,
//! macro drop) so every FC ships with a stable per-kind id from day zero.
//! `populate_fc_layer_paths` resolves those ids to absolute paths against
//! a known base directory (project dir for saved projects, temp dir for
//! unsaved ones) — needed because `asset_path` is a runtime-only param
//! that lives outside the recipe.
//!
//! The "file does not exist yet" state is a normal state. The executor's
//! composite functions read the asset file and fall through to
//! pass-through on `Err` — so an unpainted layer reads as if it weren't
//! there.

use std::collections::HashMap;
use std::path::Path;

use bar_graph::ParamValue;

use crate::package::AssetId;

/// Paintable FC layer kinds, in canonical iteration order. The strings
/// are the param-name prefixes: `{kind}_layer_asset_id` and
/// `{kind}_layer_asset_path`.
pub const FC_LAYER_KINDS: [&str; 4] = ["heightmap", "color", "metalmap", "typemap"];

/// Mint a fresh UUID into each `{kind}_layer_asset_id` slot that is
/// currently empty / missing. Idempotent: already-populated ids are
/// left untouched, so this is safe to call on already-bootstrapped FC
/// nodes (it'll only mint for kinds that don't have an id yet).
pub fn mint_fc_layer_ids(params: &mut HashMap<String, ParamValue>) {
    for kind in FC_LAYER_KINDS {
        let id_key = format!("{kind}_layer_asset_id");
        let needs_mint = match params.get(&id_key) {
            Some(ParamValue::String(s)) => s.is_empty(),
            _ => true,
        };
        if needs_mint {
            let id = AssetId::new().0;
            params.insert(id_key, ParamValue::String(id));
        }
    }
}

/// For each populated `{kind}_layer_asset_id`, set
/// `{kind}_layer_asset_path` to `base_dir/<id>.bin`. Does not create
/// the file on disk -- the file only exists once the user paints into
/// the layer. Overwrites any existing path value (callers should pass
/// the currently-effective base dir; e.g. switch from temp to project
/// dir after Save-As).
pub fn populate_fc_layer_paths(params: &mut HashMap<String, ParamValue>, base_dir: &Path) {
    for kind in FC_LAYER_KINDS {
        let id_key = format!("{kind}_layer_asset_id");
        let path_key = format!("{kind}_layer_asset_path");
        let id_str = match params.get(&id_key) {
            Some(ParamValue::String(s)) if !s.is_empty() => s.clone(),
            _ => continue,
        };
        let path = base_dir.join(format!("{id_str}.bin"));
        params.insert(
            path_key,
            ParamValue::String(path.to_string_lossy().into_owned()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_populates_empty_slots() {
        let mut params: HashMap<String, ParamValue> = HashMap::new();
        mint_fc_layer_ids(&mut params);
        for kind in FC_LAYER_KINDS {
            let key = format!("{kind}_layer_asset_id");
            match params.get(&key) {
                Some(ParamValue::String(s)) => assert!(!s.is_empty(), "{key} unset"),
                _ => panic!("{key} missing"),
            }
        }
    }

    #[test]
    fn mint_is_idempotent_for_populated_slots() {
        let mut params: HashMap<String, ParamValue> = HashMap::new();
        params.insert(
            "heightmap_layer_asset_id".to_string(),
            ParamValue::String("already-set".to_string()),
        );
        mint_fc_layer_ids(&mut params);
        match params.get("heightmap_layer_asset_id") {
            Some(ParamValue::String(s)) => assert_eq!(s, "already-set"),
            _ => panic!("heightmap id missing"),
        }
        // Other kinds still got minted.
        match params.get("color_layer_asset_id") {
            Some(ParamValue::String(s)) => assert!(!s.is_empty()),
            _ => panic!("color id missing"),
        }
    }

    #[test]
    fn populate_paths_uses_base_dir_and_id() {
        let mut params: HashMap<String, ParamValue> = HashMap::new();
        params.insert(
            "heightmap_layer_asset_id".to_string(),
            ParamValue::String("abc-123".to_string()),
        );
        populate_fc_layer_paths(&mut params, Path::new("/tmp/proj/final_composition"));
        match params.get("heightmap_layer_asset_path") {
            Some(ParamValue::String(p)) => {
                assert!(p.contains("abc-123"));
                assert!(p.ends_with(".bin"));
            }
            _ => panic!("heightmap path not populated"),
        }
    }

    #[test]
    fn populate_paths_skips_unminted_kinds() {
        let mut params: HashMap<String, ParamValue> = HashMap::new();
        params.insert(
            "heightmap_layer_asset_id".to_string(),
            ParamValue::String("xyz".to_string()),
        );
        populate_fc_layer_paths(&mut params, Path::new("/tmp"));
        assert!(params.contains_key("heightmap_layer_asset_path"));
        assert!(!params.contains_key("color_layer_asset_path"));
    }
}

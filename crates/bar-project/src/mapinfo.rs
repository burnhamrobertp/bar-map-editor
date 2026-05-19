//! Pure-string parsers for `mapinfo.lua` and a helper that populates a
//! `MapSettings` from the parsed values.
//!
//! Lives in `bar-project` (not `bar-engine`) so both the SD7 importer in
//! bar-engine AND the SD7 work-directory scan path (`scan_to_project`) can
//! reach the same code. Previously these parsers lived only in
//! `bar-engine::importer`, which was bypassed by the UI's import flow
//! (`extract_sd7_to_work_dir` -> `scan_to_project`).

use crate::recipe::MapSettings;

/// Apply per-map overrides from a `mapinfo.lua` string to `settings` in place.
///
/// Only keys that are present (and parseable) overwrite the corresponding
/// field; missing keys leave the existing value alone, so the caller can
/// seed `settings` with `MapSettings::default()` -- or any other baseline --
/// and selectively patch from the lua. `min_height`/`max_height` are not
/// touched here (the SMF header + `parse_mapinfo_smf_heights` already handle
/// those upstream).
pub fn apply_mapinfo_overrides(lua: &str, settings: &mut MapSettings) {
    // Top-level scalars on `MapSettings`.
    if let Some(v) = parse_mapinfo_number(lua, "gravity") {
        settings.gravity = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "tidalStrength") {
        settings.tidal_strength = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "maxMetal") {
        settings.max_metal = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "extractorRadius") {
        settings.extractor_radius = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "mapHardness") {
        settings.map_hardness = v as u32;
    }

    // Water table — keys mirror `rts/Map/MapInfo.cpp` (the engine's
    // BumpWater shader sources these per-map).
    let water = &mut settings.water;
    if let Some(v) = parse_mapinfo_vec3(lua, "basecolor") {
        water.base_color = v;
    }
    if let Some(v) = parse_mapinfo_vec3(lua, "absorb") {
        water.absorb = v;
    }
    if let Some(v) = parse_mapinfo_vec3(lua, "mincolor") {
        water.min_color = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "damage") {
        water.damage = v;
    }
    if let Some(v) = parse_mapinfo_vec3(lua, "surfaceColor") {
        water.surface_color = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "surfaceAlpha") {
        water.surface_alpha = v;
    }
    if let Some(v) = parse_mapinfo_vec3(lua, "diffuseColor") {
        water.diffuse_color = v;
    }
    if let Some(v) = parse_mapinfo_vec3(lua, "specularColor") {
        water.specular_color = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "ambientFactor") {
        water.ambient_factor = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "diffuseFactor") {
        water.diffuse_factor = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "specularFactor") {
        water.specular_factor = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "specularPower") {
        water.specular_power = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "fresnelMin") {
        water.fresnel_min = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "fresnelMax") {
        water.fresnel_max = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "fresnelPower") {
        water.fresnel_power = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "reflectionDistortion") {
        water.reflection_distortion = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "perlinAmplitude") {
        water.perlin_amplitude = v;
    }

    // Lighting table.
    let lighting = &mut settings.lighting;
    // Engine reads `sunDir` as a `float4` (`bar-recoil/rts/Map/MapInfo.cpp:207`)
    // with `.w` as sun intensity, packed into `sunColor.w` for the sky shader
    // (`ModernSky.cpp:82`). Older / simpler mapinfo files write a 3-vector;
    // engine treats the 4th as 1.0 by default.
    if let Some(v) = parse_mapinfo_vec4(lua, "sunDir") {
        lighting.sun_dir = [v[0], v[1], v[2]];
        lighting.sun_intensity = v[3];
    } else if let Some(v) = parse_mapinfo_vec3(lua, "sunDir") {
        lighting.sun_dir = v;
    }
    if let Some(v) = parse_mapinfo_vec3(lua, "groundAmbientColor") {
        lighting.ground_ambient = v;
    }
    if let Some(v) = parse_mapinfo_vec3(lua, "groundDiffuseColor") {
        lighting.ground_diffuse = v;
    }
    if let Some(v) = parse_mapinfo_vec3(lua, "groundSpecularColor") {
        lighting.ground_specular = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "specularExponent") {
        lighting.spec_exponent = v;
    }
    // Per-map shadow strength. Engine: `lightTable.GetFloat("groundShadowDensity",
    // 0.8f)` then `std::clamp(..., 0, 1)` (`bar-recoil/rts/Map/MapInfo.cpp:214,223`).
    if let Some(v) = parse_mapinfo_number(lua, "groundShadowDensity") {
        lighting.ground_shadow_density = v.clamp(0.0, 1.0);
    }

    // Atmosphere table (procedural sky + standard distance fog inputs).
    let atm = &mut settings.atmosphere;
    if let Some(v) = parse_mapinfo_number(lua, "minWind") {
        atm.min_wind = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "maxWind") {
        atm.max_wind = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "fogStart") {
        atm.fog_start = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "fogEnd") {
        atm.fog_end = v;
    }
    if let Some(v) = parse_mapinfo_vec3(lua, "fogColor") {
        atm.fog_color = v;
    }
    if let Some(v) = parse_mapinfo_vec3(lua, "sunColor") {
        atm.sun_color = v;
    }
    if let Some(v) = parse_mapinfo_vec3(lua, "skyColor") {
        atm.sky_color = v;
    }
    if let Some(v) = parse_mapinfo_vec3(lua, "skyDir") {
        atm.sky_dir = v;
    }
    if let Some(v) = parse_mapinfo_number(lua, "cloudDensity") {
        atm.cloud_density = v;
    }
    if let Some(v) = parse_mapinfo_vec3(lua, "cloudColor") {
        atm.cloud_color = v;
    }
    if let Some(v) = parse_mapinfo_string(lua, "skyBox") {
        atm.skybox = v;
    }

    // Resources block (`resources = { detailTex = "...", ... }`).
    // Only `detailTex` is wired at the renderer side currently; the
    // others are stored when present so they don't disappear on a
    // re-save and are available when the splat / normal-splat paths
    // land.
    if let Some(body) = extract_table_body(lua, "resources") {
        if let Some(s) = parse_mapinfo_string(&body, "detailTex") {
            settings.resources.detail_tex = s;
        }
        if let Some(s) = parse_mapinfo_string(&body, "splatDistrTex") {
            settings.resources.splat_distr_tex = s;
        }
        if let Some(s) = parse_mapinfo_string(&body, "splatDetailNormalTex1") {
            settings.resources.splat_detail_normal_tex_1 = s;
        }
        if let Some(s) = parse_mapinfo_string(&body, "splatDetailNormalTex2") {
            settings.resources.splat_detail_normal_tex_2 = s;
        }
        if let Some(s) = parse_mapinfo_string(&body, "splatDetailNormalTex3") {
            settings.resources.splat_detail_normal_tex_3 = s;
        }
        if let Some(s) = parse_mapinfo_string(&body, "splatDetailNormalTex4") {
            settings.resources.splat_detail_normal_tex_4 = s;
        }
        if let Some(v) = parse_mapinfo_number(&body, "splatDetailNormalDiffuseAlpha") {
            settings.resources.splat_detail_normal_diffuse_alpha = v >= 0.5;
        }
        if let Some(s) = parse_mapinfo_string(&body, "skyReflectModTex") {
            settings.resources.sky_reflect_mod_tex = s;
        }
        if let Some(s) = parse_mapinfo_string(&body, "specularTex") {
            settings.resources.specular_tex = s;
        }
        if let Some(s) = parse_mapinfo_string(&body, "grassShadingTex") {
            settings.resources.grass_shading_tex = s;
        }
    }
    // `splats = { texScales = {...}, texMults = {...} }`. Note this is
    // a SIBLING of `resources`, not nested inside it.
    if let Some(body) = extract_table_body(lua, "splats") {
        if let Some(v) = parse_mapinfo_vec4(&body, "texScales") {
            settings.resources.splat_tex_scales = v;
        }
        if let Some(v) = parse_mapinfo_vec4(&body, "texMults") {
            settings.resources.splat_tex_mults = v;
        }
    }

    // Height-based custom fog (`custom = { fog = { ... } }` in mapinfo).
    // Not engine-stock; in-game it's a widget that tints fragments below
    // `height` by `color`. We bake the same behaviour into our terrain /
    // water shaders so previews match what the player sees.
    if let Some(fog) = parse_custom_fog(lua, settings.max_height) {
        settings.custom_fog = fog;
    }
}

/// Parse a string field of the form `key = "value"` or `key = 'value'`.
/// Used for `skyBox = "cleardesert.dds"` style settings. Returns None
/// when the key isn't found or the value isn't quoted.
pub fn parse_mapinfo_string(lua: &str, key: &str) -> Option<String> {
    let lower = lua.to_ascii_lowercase();
    let lower_key = key.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(off) = lower[search_from..].find(&lower_key) {
        let key_start = search_from + off;
        search_from = key_start + 1;
        if key_start > 0 {
            let prev = lua.as_bytes()[key_start - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                continue;
            }
        }
        let line_start = lua[..key_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        if lua[line_start..key_start].contains("--") {
            continue;
        }
        let bytes = lua.as_bytes();
        let mut idx = key_start + key.len();
        while idx < lua.len() && (bytes[idx] as char).is_whitespace() {
            idx += 1;
        }
        if idx >= lua.len() || bytes[idx] != b'=' {
            continue;
        }
        idx += 1;
        while idx < lua.len() && (bytes[idx] as char).is_whitespace() {
            idx += 1;
        }
        if idx >= lua.len() {
            continue;
        }
        let quote = bytes[idx];
        if quote != b'"' && quote != b'\'' {
            continue;
        }
        idx += 1;
        let close = lua[idx..].find(quote as char)?;
        return Some(lua[idx..idx + close].to_string());
    }
    None
}

/// Pull the `custom.fog = { color = {…}, height = …, fogatten = … }` block
/// out of `mapinfo.lua`. `max_h_elmos` is the map's MaxHeight from the SMF
/// header; we need it to resolve `height = "40%"` style percentage strings
/// into absolute elmos before storing (the rest of the pipeline only deals
/// in absolute elmos).
pub fn parse_custom_fog(lua: &str, max_h_elmos: f32) -> Option<crate::recipe::CustomFogSettings> {
    // The block lives at `custom = { fog = { ... } }`. There may be other
    // sub-tables (precipitation, etc.) under `custom`, so we have to find
    // the `fog =` sub-table inside the `custom = { ... }` body specifically.
    let custom_body = extract_table_body(lua, "custom")?;
    let fog_body = extract_table_body(&custom_body, "fog")?;

    let color = parse_mapinfo_vec3(&fog_body, "color")?;
    let atten = parse_mapinfo_number(&fog_body, "fogatten")?;
    let height = parse_mapinfo_height(&fog_body, "height", max_h_elmos)?;

    Some(crate::recipe::CustomFogSettings {
        enabled: true,
        color,
        height_elmos: height,
        atten,
    })
}

/// Extract the body (everything between the outermost `{` and matching `}`)
/// of a top-level table assignment like `name = { ... }`. Comments inside
/// the body are preserved as-is so downstream parsers (`parse_mapinfo_*`)
/// strip them with their own logic.
fn extract_table_body(lua: &str, key: &str) -> Option<String> {
    let lower = lua.to_ascii_lowercase();
    let lower_key = key.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(off) = lower[search_from..].find(&lower_key) {
        let key_start = search_from + off;
        search_from = key_start + 1;
        // Word boundary check on the preceding character.
        if key_start > 0 {
            let prev = lua.as_bytes()[key_start - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                continue;
            }
        }
        // Skip commented-out occurrences.
        let line_start = lua[..key_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        if lua[line_start..key_start].contains("--") {
            continue;
        }
        // Advance past the key.
        let mut idx = key_start + key.len();
        let bytes = lua.as_bytes();
        // Skip whitespace, then expect `=`.
        while idx < lua.len() && (bytes[idx] as char).is_whitespace() {
            idx += 1;
        }
        if idx >= lua.len() || bytes[idx] != b'=' {
            continue;
        }
        idx += 1;
        // Skip whitespace, then expect `{`.
        while idx < lua.len() && (bytes[idx] as char).is_whitespace() {
            idx += 1;
        }
        if idx >= lua.len() || bytes[idx] != b'{' {
            continue;
        }
        // Find matching closing brace with depth tracking.
        let mut depth = 1;
        let mut end = idx + 1;
        while end < lua.len() && depth > 0 {
            match bytes[end] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
            end += 1;
        }
        if depth != 0 {
            continue;
        }
        return Some(lua[idx + 1..end].to_string());
    }
    None
}

/// Parse a mapinfo height value that may be either a literal number
/// (`height = 80`) or a percentage of MaxHeight (`height = "40%"`).
fn parse_mapinfo_height(lua: &str, key: &str, max_h_elmos: f32) -> Option<f32> {
    // Try the literal number path first.
    if let Some(v) = parse_mapinfo_number(lua, key) {
        return Some(v);
    }
    // Fall back to `"NN%"` string form.
    let pat = format!("{}=", key);
    for line in lua.lines() {
        let no_comment = match line.find("--") {
            Some(i) => &line[..i],
            None => line,
        };
        let trimmed = no_comment.trim();
        let stripped = trimmed.strip_prefix(key).map(|r| r.trim_start());
        let rest = match stripped {
            Some(r) if r.starts_with('=') => &r[1..],
            _ => {
                if !trimmed.starts_with(&pat) {
                    continue;
                }
                &trimmed[pat.len()..]
            }
        };
        let value = rest.trim().trim_start_matches('"').trim_start_matches('\'');
        let pct_end = value.find('%')?;
        if let Ok(pct) = value[..pct_end].parse::<f32>() {
            return Some(pct * 0.01 * max_h_elmos);
        }
    }
    None
}

/// Parse min/max heights from an `smf = { ... }` block in `mapinfo.lua`.
///
/// Spring/BAR uses these (when present) to reinterpret the heightmap u16
/// values. They override whatever's in the SMF binary header. Many maps
/// allocate generous header headroom (e.g. `[-50, 100]`) but specify the
/// real working range here (e.g. `[-250, 670]`) — using the binary header
/// alone produces flat-looking previews that don't match the engine.
pub fn parse_mapinfo_smf_heights(lua: &str) -> Option<(f32, f32)> {
    let lower = lua.to_lowercase();
    let mut search_from = 0usize;
    let (_smf_idx, brace_open) = loop {
        let rel = lower[search_from..].find("smf")?;
        let abs = search_from + rel;
        let after = &lua[abs + 3..];
        let mut chars = after.char_indices().peekable();
        let mut saw_eq = false;
        let mut found_brace: Option<usize> = None;
        while let Some(&(i, c)) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
                continue;
            }
            if !saw_eq {
                if c == '=' {
                    saw_eq = true;
                    chars.next();
                    continue;
                }
                break;
            }
            if c == '{' {
                found_brace = Some(abs + 3 + i);
            }
            break;
        }
        if let Some(bo) = found_brace {
            break (abs, bo);
        }
        search_from = abs + 3;
    };
    let mut depth = 0;
    let mut end = brace_open;
    for (i, c) in lua[brace_open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = brace_open + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let body = lua[brace_open + 1..end].trim();

    let parse_field = |key: &str| -> Option<f32> {
        for piece in body.split(['\n', ',']) {
            let trimmed = piece.trim().to_lowercase();
            if let Some(rest) = trimmed.strip_prefix(key).map(str::trim_start) {
                if let Some(after_eq) = rest.strip_prefix('=').map(str::trim_start) {
                    let mut end = 0;
                    for (i, c) in after_eq.char_indices() {
                        if c.is_ascii_digit()
                            || c == '-'
                            || c == '+'
                            || c == '.'
                            || c == 'e'
                            || c == 'E'
                        {
                            end = i + c.len_utf8();
                        } else {
                            break;
                        }
                    }
                    if end > 0 {
                        if let Ok(v) = after_eq[..end].parse::<f32>() {
                            return Some(v);
                        }
                    }
                }
            }
        }
        None
    };

    let min = parse_field("minheight")?;
    let max = parse_field("maxheight")?;
    Some((min, max))
}

/// Parse a top-level numeric field from `mapinfo.lua` — e.g. `gravity = 130`,
/// `tidalStrength = 18`, `mapHardness = 100`. Returns `None` if not found
/// or unparseable. Handles inline `--` comments on the value line.
pub fn parse_mapinfo_number(lua: &str, key: &str) -> Option<f32> {
    let pat = format!("{}=", key);
    for line in lua.lines() {
        let no_comment = match line.find("--") {
            Some(i) => &line[..i],
            None => line,
        };
        let trimmed = no_comment.trim();
        let stripped = trimmed.strip_prefix(key).map(|rest| rest.trim_start());
        let rest = match stripped {
            Some(r) if r.starts_with('=') => &r[1..],
            _ => {
                if !trimmed.starts_with(&pat) {
                    continue;
                }
                &trimmed[pat.len()..]
            }
        };
        let value = rest
            .trim()
            .trim_end_matches(',')
            .trim_end_matches(';')
            .trim();
        if let Ok(v) = value.parse::<f32>() {
            return Some(v);
        }
    }
    None
}

/// Parse a top-level vec3-valued field from `mapinfo.lua` -- e.g.
/// `basecolor = { 0.05, 0.7, 0.6 }`, `sunDir = { -0.64, 0.66, -0.57 }`,
/// or the multi-line `groundAmbientColor = {\n  0.35,\n  0.35,\n  0.35,\n}`.
///
/// Key match is case-insensitive (BAR's Lua mapinfo parser lowercases keys),
/// and a word-boundary check rejects substring matches inside longer keys.
pub fn parse_mapinfo_vec3(lua: &str, key: &str) -> Option<[f32; 3]> {
    let bytes = lua.as_bytes();
    let lower_lua = lua.to_ascii_lowercase();
    let lower_key = key.to_ascii_lowercase();
    let key_len = key.len();

    let mut search_from = 0usize;
    while let Some(off) = lower_lua[search_from..].find(&lower_key) {
        let key_start = search_from + off;
        search_from = key_start + 1;
        let key_end = key_start + key_len;

        if key_start > 0 {
            let prev = bytes[key_start - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                continue;
            }
        }
        let line_start = lua[..key_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let line_before_key = &lua[line_start..key_start];
        if line_before_key.contains("--") {
            continue;
        }
        let mut idx = key_end;
        while idx < lua.len() && (bytes[idx] == b' ' || bytes[idx] == b'\t') {
            idx += 1;
        }
        if idx >= lua.len() || bytes[idx] != b'=' {
            continue;
        }
        idx += 1;
        while idx < lua.len() && (bytes[idx] as char).is_whitespace() {
            idx += 1;
        }
        if idx >= lua.len() || bytes[idx] != b'{' {
            continue;
        }
        idx += 1;
        let close = match lua[idx..].find('}') {
            Some(c) => idx + c,
            None => continue,
        };
        let inner = &lua[idx..close];
        let stripped: String = inner
            .lines()
            .map(|line| {
                if let Some(c) = line.find("--") {
                    &line[..c]
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let nums: Vec<f32> = stripped
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();
        if nums.len() >= 3 {
            return Some([nums[0], nums[1], nums[2]]);
        }
    }
    None
}

/// Same as `parse_mapinfo_vec3` but returns 4 components. Used for
/// `splats.texScales` / `texMults` and any other 4-tuple mapinfo
/// fields. Implemented as a thin specialisation; if we needed more
/// arities we'd genericise.
pub fn parse_mapinfo_vec4(lua: &str, key: &str) -> Option<[f32; 4]> {
    let bytes = lua.as_bytes();
    let lower_lua = lua.to_ascii_lowercase();
    let lower_key = key.to_ascii_lowercase();
    let key_len = key.len();

    let mut search_from = 0usize;
    while let Some(off) = lower_lua[search_from..].find(&lower_key) {
        let key_start = search_from + off;
        search_from = key_start + 1;
        let key_end = key_start + key_len;

        if key_start > 0 {
            let prev = bytes[key_start - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                continue;
            }
        }
        let line_start = lua[..key_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        if lua[line_start..key_start].contains("--") {
            continue;
        }
        let mut idx = key_end;
        while idx < lua.len() && (bytes[idx] == b' ' || bytes[idx] == b'\t') {
            idx += 1;
        }
        if idx >= lua.len() || bytes[idx] != b'=' {
            continue;
        }
        idx += 1;
        while idx < lua.len() && (bytes[idx] as char).is_whitespace() {
            idx += 1;
        }
        if idx >= lua.len() || bytes[idx] != b'{' {
            continue;
        }
        idx += 1;
        let close = match lua[idx..].find('}') {
            Some(c) => idx + c,
            None => continue,
        };
        let inner = &lua[idx..close];
        let stripped: String = inner
            .lines()
            .map(|line| {
                if let Some(c) = line.find("--") {
                    &line[..c]
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let nums: Vec<f32> = stripped
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();
        if nums.len() >= 4 {
            return Some([nums[0], nums[1], nums[2], nums[3]]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mapinfo_smf_heights_basic() {
        let lua = r#"
local mapinfo = {
    name = "Test",
    smf = {
        minheight = -250,
        maxheight = 670,
        smtFileName0 = "maps/test.smt",
    },
}
"#;
        assert_eq!(parse_mapinfo_smf_heights(lua), Some((-250.0, 670.0)));
    }

    #[test]
    fn parse_mapinfo_smf_heights_negative_zero() {
        let lua = "smf = { minheight = 0, maxheight = 1024.5 }";
        assert_eq!(parse_mapinfo_smf_heights(lua), Some((0.0, 1024.5)));
    }

    #[test]
    fn parse_mapinfo_smf_heights_missing_returns_none() {
        let lua = "smf = { smtFileName0 = \"foo.smt\" }";
        assert_eq!(parse_mapinfo_smf_heights(lua), None);
    }

    #[test]
    fn parse_mapinfo_smf_heights_skips_smf_in_comment() {
        let lua = r#"
local mapinfo = {
    name = "Kolmog",
    --mapfile = "", --// location of smf/sm3 file (optional)
    depend  = {},
    replace = {},

    smf = {
        minheight = -250,
        maxheight = 670,
    },
}
"#;
        assert_eq!(parse_mapinfo_smf_heights(lua), Some((-250.0, 670.0)));
    }

    #[test]
    fn parse_mapinfo_vec3_inline() {
        let lua = "basecolor = { 0.05, 0.7, 0.6 }, -- the color shallow water starts out at";
        assert_eq!(parse_mapinfo_vec3(lua, "basecolor"), Some([0.05, 0.7, 0.6]));
    }

    #[test]
    fn parse_mapinfo_vec3_case_insensitive() {
        let lua = "basecolor = { 0.1, 0.2, 0.3 }";
        assert_eq!(parse_mapinfo_vec3(lua, "baseColor"), Some([0.1, 0.2, 0.3]));
    }

    #[test]
    fn parse_mapinfo_vec3_word_boundary() {
        let lua = "unitbasecolor = { 0.9, 0.9, 0.9 }";
        assert_eq!(parse_mapinfo_vec3(lua, "basecolor"), None);
    }

    #[test]
    fn parse_mapinfo_vec3_skips_commented_key() {
        let lua = "-- basecolor = { 0.1, 0.2, 0.3 }\nbasecolor = { 0.5, 0.6, 0.7 }";
        assert_eq!(parse_mapinfo_vec3(lua, "basecolor"), Some([0.5, 0.6, 0.7]));
    }

    #[test]
    fn parse_mapinfo_number_handles_inline_comment() {
        let lua = "fresnelMin = 0.1, --This defines the minimum amount of light\n\
                   fresnelMax = 0.5, --Defines the maximum amount\n\
                   fresnelPower = 3.0, --Defines how much\n\
                   plain = 42";
        assert_eq!(parse_mapinfo_number(lua, "fresnelMin"), Some(0.1));
        assert_eq!(parse_mapinfo_number(lua, "fresnelMax"), Some(0.5));
        assert_eq!(parse_mapinfo_number(lua, "fresnelPower"), Some(3.0));
        assert_eq!(parse_mapinfo_number(lua, "plain"), Some(42.0));
    }

    #[test]
    fn parse_mapinfo_number_skips_commented_out_line() {
        let lua = "-- fresnelMin = 0.99\nfresnelMin = 0.1";
        assert_eq!(parse_mapinfo_number(lua, "fresnelMin"), Some(0.1));
    }

    #[test]
    fn apply_overrides_populates_water_and_lighting() {
        let lua = r#"
gravity = 130,
tidalStrength = 18,
fresnelMin = 0.1, -- min
fresnelMax = 0.5,
basecolor = { 0.05, 0.7, 0.6 },
sunDir = { -0.64, 0.66, -0.57 },
"#;
        let mut settings = MapSettings::default();
        apply_mapinfo_overrides(lua, &mut settings);
        assert_eq!(settings.gravity, 130.0);
        assert_eq!(settings.tidal_strength, 18.0);
        assert!((settings.water.fresnel_min - 0.1).abs() < 1e-6);
        assert!((settings.water.fresnel_max - 0.5).abs() < 1e-6);
        assert_eq!(settings.water.base_color, [0.05, 0.7, 0.6]);
        assert_eq!(settings.lighting.sun_dir, [-0.64, 0.66, -0.57]);
        // 3-element sunDir leaves intensity at its default 1.0.
        assert!((settings.lighting.sun_intensity - 1.0).abs() < 1e-6);
    }

    #[test]
    fn ground_shadow_density_parses_and_clamps() {
        // Engine reads then clamps to [0, 1] (`MapInfo.cpp:214, 223`).
        let lua = "groundShadowDensity = 0.65,";
        let mut s = MapSettings::default();
        apply_mapinfo_overrides(lua, &mut s);
        assert!((s.lighting.ground_shadow_density - 0.65).abs() < 1e-6);

        // Out-of-range values clamp.
        let lua_high = "groundShadowDensity = 1.4,";
        let mut s_high = MapSettings::default();
        apply_mapinfo_overrides(lua_high, &mut s_high);
        assert_eq!(s_high.lighting.ground_shadow_density, 1.0);

        let lua_low = "groundShadowDensity = -0.1,";
        let mut s_low = MapSettings::default();
        apply_mapinfo_overrides(lua_low, &mut s_low);
        assert_eq!(s_low.lighting.ground_shadow_density, 0.0);

        // Missing key leaves the engine default (0.8) untouched.
        let mut s_default = MapSettings::default();
        apply_mapinfo_overrides("", &mut s_default);
        assert!((s_default.lighting.ground_shadow_density - 0.8).abs() < 1e-6);
    }

    #[test]
    fn four_element_sun_dir_picks_up_intensity() {
        // Mirrors the engine's `light.sunDir = lightTable.GetFloat4("sunDir", ...)`
        // path (`bar-recoil/rts/Map/MapInfo.cpp:207`). The 4th component is
        // packed into `sunColor.w` by `ModernSky.cpp:82` and multiplied into
        // the sun corona by `ModernSkyFS.glsl:88`.
        let lua = "sunDir = { -0.64, 0.66, -0.57, 0.75 },";
        let mut settings = MapSettings::default();
        apply_mapinfo_overrides(lua, &mut settings);
        assert_eq!(settings.lighting.sun_dir, [-0.64, 0.66, -0.57]);
        assert!((settings.lighting.sun_intensity - 0.75).abs() < 1e-6);
    }

    #[test]
    fn parses_aurelia_custom_fog_block() {
        // Format taken verbatim from Aurelia v4.1's mapinfo.lua.
        let lua = r#"
local mapinfo = {
    name = "Aurelia",
    custom = {
        fog = {
            color    = {0.26, 0.30, 0.41},
            height   = "40%",
            fogatten = 0.0075,
        },
    },
}
"#;
        let mut settings = MapSettings {
            max_height: 261.0,
            ..MapSettings::default()
        };
        apply_mapinfo_overrides(lua, &mut settings);
        assert!(settings.custom_fog.enabled);
        assert_eq!(settings.custom_fog.color, [0.26, 0.30, 0.41]);
        assert!((settings.custom_fog.atten - 0.0075).abs() < 1e-6);
        // 40% of 261 elmos.
        assert!((settings.custom_fog.height_elmos - 104.4).abs() < 1e-3);
    }

    #[test]
    fn custom_fog_absent_leaves_disabled() {
        let lua = "name = \"NoFog\"";
        let mut settings = MapSettings::default();
        apply_mapinfo_overrides(lua, &mut settings);
        assert!(!settings.custom_fog.enabled);
    }

    #[test]
    fn parses_atmosphere_block_aurelia() {
        // Lifted from Aurelia v4.1's mapinfo.lua (note lowercase
        // `skycolor` and string-typed `skyBox`).
        let lua = r#"
atmosphere = {
    minWind      = 3,
    maxWind      = 16,
    fogStart     = 0.8,
    fogEnd       = 1,
    fogColor     = {0.8, 0.6, 0.5},
    sunColor     = {1.0, 0.7, 0.7},
    skycolor     = {0.2, 0.25, 0.05},
    skyDir       = {0.0, 0.0, -1.0},
    skyBox       = "cleardesert.dds",
    cloudDensity = 0.25,
    cloudColor   = {0.95, 0.85, 0.75},
},
"#;
        let mut settings = MapSettings::default();
        apply_mapinfo_overrides(lua, &mut settings);
        let a = &settings.atmosphere;
        assert_eq!(a.min_wind, 3.0);
        assert_eq!(a.max_wind, 16.0);
        assert!((a.fog_start - 0.8).abs() < 1e-6);
        assert_eq!(a.fog_color, [0.8, 0.6, 0.5]);
        assert_eq!(a.sun_color, [1.0, 0.7, 0.7]);
        assert_eq!(a.sky_color, [0.2, 0.25, 0.05]);
        assert_eq!(a.sky_dir, [0.0, 0.0, -1.0]);
        assert!((a.cloud_density - 0.25).abs() < 1e-6);
        assert_eq!(a.cloud_color, [0.95, 0.85, 0.75]);
        assert_eq!(a.skybox, "cleardesert.dds");
    }

    #[test]
    fn parses_resources_detail_tex() {
        let lua = r#"
resources = {
    detailTex = "detailtexblurred.bmp",
    splatDistrTex = "splat.dds",
},
"#;
        let mut settings = MapSettings::default();
        apply_mapinfo_overrides(lua, &mut settings);
        assert_eq!(settings.resources.detail_tex, "detailtexblurred.bmp");
        assert_eq!(settings.resources.splat_distr_tex, "splat.dds");
    }

    #[test]
    fn parses_aurelia_splat_detail_normal() {
        // Lifted from Aurelia's mapinfo.lua; tests both the four
        // splatDetailNormalTexN keys and the sibling `splats` table.
        let lua = r#"
resources = {
    detailTex = "detailtexblurred.bmp",
    splatDistrTex = "splat.dds",
    splatDetailNormalDiffuseAlpha = 1,
    splatDetailNormalTex1 = "cracks_2_dnts_.dds",
    splatDetailNormalTex2 = "dirt_267_highpass_dnts.dds",
    splatDetailNormalTex3 = "rugged_rock.dds",
    splatDetailNormalTex4 = "torturedrock.dds",
},
splats = {
    texScales = {0.0032, 0.0063, 0.0044, 0.0055},
    texMults  = {0.31, 0.22, 1, 0.73},
},
"#;
        let mut settings = MapSettings::default();
        apply_mapinfo_overrides(lua, &mut settings);
        let r = &settings.resources;
        assert!(r.splat_detail_normal_diffuse_alpha);
        assert_eq!(r.splat_detail_normal_tex_1, "cracks_2_dnts_.dds");
        assert_eq!(r.splat_detail_normal_tex_2, "dirt_267_highpass_dnts.dds");
        assert_eq!(r.splat_detail_normal_tex_3, "rugged_rock.dds");
        assert_eq!(r.splat_detail_normal_tex_4, "torturedrock.dds");
        assert!((r.splat_tex_scales[0] - 0.0032).abs() < 1e-6);
        assert!((r.splat_tex_scales[3] - 0.0055).abs() < 1e-6);
        assert!((r.splat_tex_mults[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn custom_fog_literal_height() {
        let lua = r#"
custom = {
    fog = {
        color = { 0.1, 0.2, 0.3 },
        height = 50,
        fogatten = 0.01,
    },
}
"#;
        let mut settings = MapSettings::default();
        apply_mapinfo_overrides(lua, &mut settings);
        assert!(settings.custom_fog.enabled);
        assert_eq!(settings.custom_fog.height_elmos, 50.0);
    }
}

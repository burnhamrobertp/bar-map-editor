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
    // SMF heights live inside the `smf = { minheight = ..., maxheight = ... }`
    // sub-table. Use the dedicated parser so we walk into the
    // sub-table rather than catching unrelated top-level keys.
    if let Some((min_h, max_h)) = parse_mapinfo_smf_heights(lua) {
        settings.min_height = Some(min_h);
        settings.max_height = Some(max_h);
    }

    // Top-level scalars on `MapSettings`. All Option-valued: a field
    // is only `Some` when the source mapinfo actually had it; the
    // emitter then writes only those, and the renderer falls through
    // to engine defaults via `MapSettings::resolved()`.
    settings.gravity = parse_mapinfo_number(lua, "gravity");
    settings.tidal_strength = parse_mapinfo_number(lua, "tidalStrength");
    settings.max_metal = parse_mapinfo_number(lua, "maxMetal");
    settings.extractor_radius = parse_mapinfo_number(lua, "extractorRadius");
    settings.map_hardness = parse_mapinfo_number(lua, "mapHardness").map(|v| v as u32);
    settings.void_water = parse_mapinfo_bool(lua, "voidWater");
    settings.void_ground = parse_mapinfo_bool(lua, "voidGround");
    settings.auto_show_metal = parse_mapinfo_bool(lua, "autoShowMetal");
    // Engine convention: mapinfo has `notDeformable` (inverse flag) so
    // omission means deformable=true. Invert when reading so the
    // recipe carries the positive `deformable` form throughout.
    settings.deformable = parse_mapinfo_bool(lua, "notDeformable").map(|v| !v);

    // replace = { ... }: parse just enough to know whether the
    // source declared the key (so we can re-emit it). Real maps
    // almost always have an empty replace table; non-empty cases
    // (string-keyed archive overrides) are rare and parsed as a
    // best-effort flat key/value list.
    if let Some(body) = extract_table_body(lua, "replace") {
        settings.replace = crate::recipe::ReplaceTable {
            declared: true,
            entries: parse_replace_entries(&body),
        };
    }

    // terrainTypes = { [N] = { name, hardness, receiveTracks,
    // moveSpeeds = { tank, kbot, hover, ship } } }
    if let Some(entries) = parse_terrain_types(lua) {
        settings.terrain_types = entries;
    }

    // Water table.
    let water = &mut settings.water;
    water.base_color = parse_mapinfo_vec3(lua, "basecolor");
    water.absorb = parse_mapinfo_vec3(lua, "absorb");
    water.min_color = parse_mapinfo_vec3(lua, "mincolor");
    water.damage = parse_mapinfo_number(lua, "damage");
    water.surface_color = parse_mapinfo_vec3(lua, "surfaceColor");
    water.surface_alpha = parse_mapinfo_number(lua, "surfaceAlpha");
    water.diffuse_color = parse_mapinfo_vec3(lua, "diffuseColor");
    water.specular_color = parse_mapinfo_vec3(lua, "specularColor");
    water.ambient_factor = parse_mapinfo_number(lua, "ambientFactor");
    water.diffuse_factor = parse_mapinfo_number(lua, "diffuseFactor");
    water.specular_factor = parse_mapinfo_number(lua, "specularFactor");
    water.specular_power = parse_mapinfo_number(lua, "specularPower");
    water.fresnel_min = parse_mapinfo_number(lua, "fresnelMin");
    water.fresnel_max = parse_mapinfo_number(lua, "fresnelMax");
    water.fresnel_power = parse_mapinfo_number(lua, "fresnelPower");
    water.reflection_distortion = parse_mapinfo_number(lua, "reflectionDistortion");
    water.perlin_amplitude = parse_mapinfo_number(lua, "perlinAmplitude");
    water.blur_base = parse_mapinfo_number(lua, "blurBase");
    water.blur_exponent = parse_mapinfo_number(lua, "blurExponent");
    water.caustics_resolution = parse_mapinfo_number(lua, "causticsResolution");
    water.caustics_strength = parse_mapinfo_number(lua, "causticsStrength");
    water.wave_offset_factor = parse_mapinfo_number(lua, "waveOffsetFactor");
    water.wave_foam_distortion = parse_mapinfo_number(lua, "waveFoamDistortion");
    water.wave_foam_intensity = parse_mapinfo_number(lua, "waveFoamIntensity");
    water.wave_length = parse_mapinfo_number(lua, "waveLength");
    water.force_rendering = parse_mapinfo_bool(lua, "forceRendering");
    water.has_water_plane = parse_mapinfo_bool(lua, "hasWaterPlane");
    water.num_tiles = parse_mapinfo_number(lua, "numTiles").map(|v| v.max(1.0) as u32);
    water.perlin_start_freq = parse_mapinfo_number(lua, "perlinStartFreq");
    water.perlin_lacunarity = parse_mapinfo_number(lua, "perlinLacunarity");
    water.plane_color = parse_mapinfo_vec3(lua, "planeColor");
    water.repeat_x = parse_mapinfo_number(lua, "repeatX");
    water.repeat_y = parse_mapinfo_number(lua, "repeatY");
    water.shore_waves = parse_mapinfo_bool(lua, "shoreWaves");
    water.normal_texture = parse_mapinfo_string(lua, "normalTexture");

    // Lighting table.
    let lighting = &mut settings.lighting;
    if let Some(v) = parse_mapinfo_vec4(lua, "sunDir") {
        lighting.sun_dir = Some([v[0], v[1], v[2]]);
        lighting.sun_intensity = Some(v[3]);
    } else if let Some(v) = parse_mapinfo_vec3(lua, "sunDir") {
        lighting.sun_dir = Some(v);
    }
    lighting.ground_ambient = parse_mapinfo_vec3(lua, "groundAmbientColor");
    lighting.ground_diffuse = parse_mapinfo_vec3(lua, "groundDiffuseColor");
    lighting.ground_specular = parse_mapinfo_vec3(lua, "groundSpecularColor");
    lighting.spec_exponent = parse_mapinfo_number(lua, "specularExponent");
    lighting.ground_shadow_density =
        parse_mapinfo_number(lua, "groundShadowDensity").map(|v| v.clamp(0.0, 1.0));
    lighting.unit_shadow_density =
        parse_mapinfo_number(lua, "unitShadowDensity").map(|v| v.clamp(0.0, 1.0));
    lighting.unit_ambient = parse_mapinfo_vec3(lua, "unitAmbientColor");
    lighting.unit_diffuse = parse_mapinfo_vec3(lua, "unitDiffuseColor");
    lighting.unit_specular = parse_mapinfo_vec3(lua, "unitSpecularColor");

    // Atmosphere table.
    let atm = &mut settings.atmosphere;
    atm.min_wind = parse_mapinfo_number(lua, "minWind");
    atm.max_wind = parse_mapinfo_number(lua, "maxWind");
    atm.fog_start = parse_mapinfo_number(lua, "fogStart");
    atm.fog_end = parse_mapinfo_number(lua, "fogEnd");
    atm.fog_color = parse_mapinfo_vec3(lua, "fogColor");
    atm.sun_color = parse_mapinfo_vec3(lua, "sunColor");
    atm.sky_color = parse_mapinfo_vec3(lua, "skyColor");
    atm.sky_dir = parse_mapinfo_vec3(lua, "skyDir");
    atm.cloud_density = parse_mapinfo_number(lua, "cloudDensity");
    atm.cloud_color = parse_mapinfo_vec3(lua, "cloudColor");
    atm.skybox = parse_mapinfo_string(lua, "skyBox");

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
        if let Some(s) = parse_mapinfo_string(&body, "lightEmissionTex") {
            settings.resources.light_emission_tex = s;
        }
        if let Some(s) = parse_mapinfo_string(&body, "detailNormalTex") {
            settings.resources.detail_normal_tex = s;
        }
        if let Some(s) = parse_mapinfo_string(&body, "splatDetailTex") {
            settings.resources.splat_detail_tex = s;
        }
    }
    // `splats = { texScales = {...}, texMults = {...} }`. Note this is
    // a SIBLING of `resources`, not nested inside it.
    if let Some(body) = extract_table_body(lua, "splats") {
        settings.resources.splat_tex_scales = parse_mapinfo_vec4(&body, "texScales");
        settings.resources.splat_tex_mults = parse_mapinfo_vec4(&body, "texMults");
    }

    // Height-based custom fog (`custom = { fog = { ... } }` in mapinfo).
    // Not engine-stock; in-game it's a widget that tints fragments below
    // `height` by `color`. We bake the same behaviour into our terrain /
    // water shaders so previews match what the player sees.
    // `custom.fog.height` can be either an absolute value or a
    // percentage of the map's max height. We need the resolved max
    // height here so percentage strings ("40%") can be turned into
    // absolute elmos before storing.
    let max_h = settings
        .max_height
        .unwrap_or(crate::engine_defaults::MAP_MAX_HEIGHT);
    if let Some(fog) = parse_custom_fog(lua, max_h) {
        settings.custom_fog = fog;
    }

    // Grass widget config (`custom = { grassConfig = { ... } }`). BAR's
    // `map_grass_gl4` LuaUI widget reads this block to spawn animated
    // grass blades from a distribution mask. Same in-scope-because-
    // mapinfo-authored line as `custom.fog`.
    if let Some(grass) = parse_custom_grass(lua) {
        settings.custom_grass = grass;
    }

    // Volumetric clouds widget config (`custom = { clouds = { ... } }`).
    if let Some(clouds) = parse_custom_clouds(lua) {
        settings.custom_clouds = clouds;
    }

    // Sound / EFX preset (top-level `sound = { preset, passfilter,
    // reverb }`).
    if let Some(sound) = parse_sound(lua) {
        settings.sound = sound;
    }

    // Team start positions (`teams = { [N] = { startPos = { x, z } } }`).
    // Without this, BME's emitter falls through to corner defaults
    // and commanders spawn in the wrong place on re-bundled maps.
    if let Some(spawns) = parse_team_start_positions(lua) {
        settings.start_positions = spawns;
    }
}

/// Parse `replace = { ["key"] = "value", ... }` entries. Returns
/// an empty Vec when the body is empty (the engine's empty-`{}`
/// case). Quote style is normalised to double-quoted on emit.
fn parse_replace_entries(body: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    // Lines of the form `["foo"] = "bar",` or `foo = "bar",`.
    for line in body.lines() {
        let line = line.split("--").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        // Strip `[ ... ]` wrappers around the key, if present.
        let eq = match line.find('=') {
            Some(i) => i,
            None => continue,
        };
        let raw_key = line[..eq]
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']');
        let key = raw_key.trim().trim_matches(|c| c == '"' || c == '\'');
        let raw_val = line[eq + 1..]
            .trim()
            .trim_end_matches(',')
            .trim_end_matches(';')
            .trim();
        let val = raw_val.trim_matches(|c| c == '"' || c == '\'');
        if !key.is_empty() {
            entries.push((key.to_string(), val.to_string()));
        }
    }
    entries
}

/// Walk a `terrainTypes = { [N] = { ... } }` block.
fn parse_terrain_types(lua: &str) -> Option<Vec<crate::recipe::TerrainTypeEntry>> {
    let body = extract_table_body(lua, "terrainTypes")?;
    let bytes = body.as_bytes();
    let mut i = 0usize;
    let mut out: Vec<crate::recipe::TerrainTypeEntry> = Vec::new();
    while i < bytes.len() {
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        let key_start = i + 1;
        let mut j = key_start;
        while j < bytes.len() && bytes[j] != b']' {
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
        let idx_str = body[key_start..j].trim();
        let idx: u32 = match idx_str.parse() {
            Ok(v) => v,
            Err(_) => {
                i = j + 1;
                continue;
            }
        };
        i = j + 1;
        while i < bytes.len() && bytes[i] != b'{' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let entry_start = i + 1;
        let mut depth: i32 = 1;
        let mut k = entry_start;
        while k < bytes.len() && depth > 0 {
            match bytes[k] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            k += 1;
        }
        let entry_body = &body[entry_start..k.saturating_sub(1)];
        // Inside an entry we'll have `name = "..."`, `hardness = N`,
        // `receiveTracks = bool`, plus `moveSpeeds = { ... }`. The
        // last is a sub-table that we extract separately.
        let move_speeds_body = extract_table_body(entry_body, "moveSpeeds");
        let mut move_speeds: Vec<(String, f32)> = Vec::new();
        if let Some(ms) = move_speeds_body {
            let normalised = ms.replace(',', "\n");
            for line in normalised.lines() {
                let l = line.split("--").next().unwrap_or("").trim();
                if l.is_empty() {
                    continue;
                }
                let eq = match l.find('=') {
                    Some(p) => p,
                    None => continue,
                };
                let key = l[..eq].trim().to_string();
                let val_str = l[eq + 1..]
                    .trim()
                    .trim_end_matches(',')
                    .trim_end_matches(';')
                    .trim();
                if let Ok(v) = val_str.parse::<f32>() {
                    move_speeds.push((key, v));
                }
            }
        }
        out.push(crate::recipe::TerrainTypeEntry {
            index: idx,
            name: parse_mapinfo_string(entry_body, "name"),
            hardness: parse_mapinfo_number(entry_body, "hardness"),
            receive_tracks: parse_mapinfo_bool(entry_body, "receiveTracks"),
            move_speeds,
        });
        i = k;
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Parse `teams = { [N] = { startPos = { x = ..., z = ... } } }`
/// out of mapinfo.lua. Returns one `[x, z]` per team index found,
/// in order. Returns `None` when the block is absent.
///
/// Engine spawn coordinates are spring world units (elmos); the
/// recipe stores them verbatim so a re-bundle round-trips the
/// author's authored positions.
pub fn parse_team_start_positions(lua: &str) -> Option<Vec<[u32; 2]>> {
    let teams_body = extract_table_body(lua, "teams")?;
    // Each entry: `[N] = { startPos = { x = X, z = Z } }` (or `Z` /
    // y-as-z variants). Walk the body byte by byte, find `[N]` keys,
    // then extract that team's startPos sub-table.
    let bytes = teams_body.as_bytes();
    let mut i = 0usize;
    let mut out: Vec<(u32, [u32; 2])> = Vec::new();
    while i < bytes.len() {
        // Find next `[` at this depth (depth 0).
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        // Parse the integer index inside the brackets.
        let key_start = i + 1;
        let mut j = key_start;
        while j < bytes.len() && bytes[j] != b']' {
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
        let idx_str = &teams_body[key_start..j].trim();
        let idx: u32 = match idx_str.parse() {
            Ok(v) => v,
            Err(_) => {
                i = j + 1;
                continue;
            }
        };
        // After `]`, expect ` = { ... }`. Scan to the next `{`.
        i = j + 1;
        while i < bytes.len() && bytes[i] != b'{' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        // Walk to matching `}` at depth 1.
        let team_body_start = i + 1;
        let mut depth: i32 = 1;
        let mut k = team_body_start;
        while k < bytes.len() && depth > 0 {
            match bytes[k] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            k += 1;
        }
        let team_body = &teams_body[team_body_start..k.saturating_sub(1)];
        // startPos = { x = X, z = Z } -- usually on a single line, so
        // normalise commas to newlines first so the line-oriented
        // `parse_mapinfo_number` can find each named entry.
        if let Some(sp_body) = extract_table_body(team_body, "startPos") {
            let normalised = sp_body.replace(',', "\n");
            let x = parse_mapinfo_number(&normalised, "x");
            // Engine accepts both `z` and `y` for the second axis on
            // some maps; prefer `z` (the modern form).
            let z = parse_mapinfo_number(&normalised, "z")
                .or_else(|| parse_mapinfo_number(&normalised, "y"));
            if let (Some(xv), Some(zv)) = (x, z) {
                out.push((idx, [xv.max(0.0) as u32, zv.max(0.0) as u32]));
            }
        }
        i = k;
    }
    if out.is_empty() {
        return None;
    }
    out.sort_by_key(|(idx, _)| *idx);
    Some(out.into_iter().map(|(_, pos)| pos).collect())
}

/// Pull the `custom.clouds = { ... }` block out of mapinfo.lua.
/// Returns `None` when absent; returns `Some` with every present
/// sub-field set to `Some(v)` so round-trip preserves the source.
pub fn parse_custom_clouds(lua: &str) -> Option<crate::recipe::CustomCloudsSettings> {
    let custom_body = extract_table_body(lua, "custom")?;
    let clouds_body = extract_table_body(&custom_body, "clouds")?;
    Some(crate::recipe::CustomCloudsSettings {
        speed: parse_mapinfo_number(&clouds_body, "speed"),
        color: parse_mapinfo_vec3(&clouds_body, "color"),
        height: parse_mapinfo_number(&clouds_body, "height"),
        bottom: parse_mapinfo_number(&clouds_body, "bottom"),
        fade_alt: parse_mapinfo_number(&clouds_body, "fade_alt"),
        scale: parse_mapinfo_number(&clouds_body, "scale"),
        opacity: parse_mapinfo_number(&clouds_body, "opacity"),
        clamp_to_map: parse_mapinfo_bool(&clouds_body, "clamp_to_map"),
        sun_penetration: parse_mapinfo_number(&clouds_body, "sun_penetration"),
    })
}

/// Pull the top-level `sound = { ... }` block out of mapinfo.lua.
/// Returns `None` when absent. Models only `preset` + `passfilter`
/// (reverb is unused on real maps).
pub fn parse_sound(lua: &str) -> Option<crate::recipe::SoundSettings> {
    let sound_body = extract_table_body(lua, "sound")?;
    let preset = parse_mapinfo_string(&sound_body, "preset");
    let (passfilter_gainlf, passfilter_gainhf) =
        if let Some(pf) = extract_table_body(&sound_body, "passfilter") {
            (
                parse_mapinfo_number(&pf, "gainlf"),
                parse_mapinfo_number(&pf, "gainhf"),
            )
        } else {
            (None, None)
        };
    if preset.is_none() && passfilter_gainlf.is_none() && passfilter_gainhf.is_none() {
        return None;
    }
    Some(crate::recipe::SoundSettings {
        preset,
        passfilter_gainlf,
        passfilter_gainhf,
    })
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

/// Parse a Lua-table-of-strings field: `key = { "foo", "bar" }`. Used
/// for `depend = { "Map Helper v1" }` and friends. Returns `None` if
/// the key isn't found; returns `Some(vec![])` if the table is empty.
/// Items can be quoted with `"` or `'`; whitespace and newlines
/// between items are tolerated. Comments (`--` to end of line) inside
/// the table are stripped.
pub fn parse_mapinfo_string_list(lua: &str, key: &str) -> Option<Vec<String>> {
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
        if idx >= lua.len() || bytes[idx] != b'{' {
            continue;
        }
        idx += 1;
        let close = lua[idx..].find('}')?;
        let body = &lua[idx..idx + close];
        // Strip line comments before extracting strings.
        let cleaned: String = body
            .lines()
            .map(|line| match line.find("--") {
                Some(p) => &line[..p],
                None => line,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut out = Vec::new();
        let mut rest = cleaned.as_str();
        while let Some(q_pos) = rest.find(['"', '\'']) {
            let quote = rest.as_bytes()[q_pos] as char;
            let after_open = q_pos + 1;
            let close_off = rest[after_open..].find(quote)?;
            out.push(rest[after_open..after_open + close_off].to_string());
            rest = &rest[after_open + close_off + 1..];
        }
        return Some(out);
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

/// Pull the `custom.grassConfig = { ... }` block out of `mapinfo.lua`.
/// Returns `None` when the block is absent or has no `grassDistTGA`
/// (without that the widget can't pick patch positions). Matches
/// BAR's `map_grass_gl4` widget config-read logic
/// (`bar-game/luaui/Widgets/map_grass_gl4.lua:87-110` for defaults
/// and `~:160-200` for per-map override pattern).
pub fn parse_custom_grass(lua: &str) -> Option<crate::recipe::CustomGrassSettings> {
    let custom_body = extract_table_body(lua, "custom")?;
    let grass_body = extract_table_body(&custom_body, "grassConfig")?;

    let mut settings = crate::recipe::CustomGrassSettings {
        dist_tga: parse_mapinfo_string(&grass_body, "grassDistTGA"),
        ..Default::default()
    };
    if settings
        .dist_tga
        .as_deref()
        .map(str::is_empty)
        .unwrap_or(true)
    {
        // No distribution mask -> widget would generate zero blades.
        // Treat the block as if it weren't present so the renderer
        // gate stays off.
        return None;
    }
    settings.blade_color_tex = parse_mapinfo_string(&grass_body, "grassBladeColorTex");
    settings.max_size = parse_mapinfo_number(&grass_body, "grassMaxSize");
    settings.min_size = parse_mapinfo_number(&grass_body, "grassMinSize");
    settings.patch_resolution =
        parse_mapinfo_number(&grass_body, "patchResolution").map(|v| v.max(1.0) as u32);
    settings.patch_placement_jitter =
        parse_mapinfo_number(&grass_body, "patchPlacementJitter").map(|v| v.clamp(0.0, 1.0));

    // Grass-shader scalars live in a nested `grassShaderParams =
    // { ... }` sub-table. Each key mirrors the BAR widget verbatim
    // (`map_grass_gl4.lua:93-110`); values stay in engine units
    // (elmos for distances, dimensionless for factors).
    if let Some(shader_body) = extract_table_body(&grass_body, "grassShaderParams") {
        settings.map_color_factor = parse_mapinfo_number(&shader_body, "MAPCOLORFACTOR");
        settings.map_color_base = parse_mapinfo_number(&shader_body, "MAPCOLORBASE");
        settings.alpha_threshold = parse_mapinfo_number(&shader_body, "ALPHATHRESHOLD");
        settings.shadow_factor = parse_mapinfo_number(&shader_body, "SHADOWFACTOR");
        settings.grass_brightness = parse_mapinfo_number(&shader_body, "GRASSBRIGHTNESS");
        settings.fade_start = parse_mapinfo_number(&shader_body, "FADESTART");
        settings.fade_end = parse_mapinfo_number(&shader_body, "FADEEND");
        settings.wind_strength = parse_mapinfo_number(&shader_body, "WINDSTRENGTH");
        settings.wind_scale = parse_mapinfo_number(&shader_body, "WINDSCALE");
        settings.wind_sample_scale = parse_mapinfo_number(&shader_body, "WINDSAMPLESCALE");
    }
    settings.grass_wind_mult = parse_mapinfo_number(&grass_body, "grassWindMult");

    Some(settings)
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
/// Parse a Lua boolean field: `key = true` / `key = false`. Case-
/// insensitive on the key, returns `None` when the key is missing or
/// the value isn't a recognisable boolean literal.
pub fn parse_mapinfo_bool(lua: &str, key: &str) -> Option<bool> {
    let key_lower = key.to_ascii_lowercase();
    for line in lua.lines() {
        let no_comment = match line.find("--") {
            Some(i) => &line[..i],
            None => line,
        };
        let trimmed = no_comment.trim().to_ascii_lowercase();
        let stripped = trimmed
            .strip_prefix(&key_lower)
            .map(|rest| rest.trim_start());
        let value_str = match stripped {
            Some(r) if r.starts_with('=') => &r[1..],
            _ => continue,
        };
        let value = value_str
            .trim()
            .trim_end_matches(',')
            .trim_end_matches(';')
            .trim();
        match value {
            "true" => return Some(true),
            "false" => return Some(false),
            _ => {}
        }
    }
    None
}

pub fn parse_mapinfo_number(lua: &str, key: &str) -> Option<f32> {
    // Case-insensitive key match: BAR's Lua mapinfo loader lowercases
    // every key on read, so authors freely mix `mapHardness` /
    // `maphardness` / `MapHardness`. Without case folding here, the
    // emit-then-parse round-trip can drop fields whose emitter and
    // parser disagree on casing.
    let key_lower = key.to_ascii_lowercase();
    let pat = format!("{key_lower}=");
    for line in lua.lines() {
        let no_comment = match line.find("--") {
            Some(i) => &line[..i],
            None => line,
        };
        let trimmed_orig = no_comment.trim();
        let trimmed = trimmed_orig.to_ascii_lowercase();
        let stripped = trimmed
            .strip_prefix(&key_lower)
            .map(|rest| rest.trim_start());
        let value_str_lower = match stripped {
            Some(r) if r.starts_with('=') => &r[1..],
            _ => {
                if !trimmed.starts_with(&pat) {
                    continue;
                }
                &trimmed[pat.len()..]
            }
        };
        let value = value_str_lower
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
        assert_eq!(settings.gravity, Some(130.0));
        assert_eq!(settings.tidal_strength, Some(18.0));
        assert!((settings.water.fresnel_min.unwrap() - 0.1).abs() < 1e-6);
        assert!((settings.water.fresnel_max.unwrap() - 0.5).abs() < 1e-6);
        assert_eq!(settings.water.base_color, Some([0.05, 0.7, 0.6]));
        assert_eq!(settings.lighting.sun_dir, Some([-0.64, 0.66, -0.57]));
        // 3-element sunDir leaves intensity unset (None -> resolved to 1.0).
        assert_eq!(settings.lighting.sun_intensity, None);
    }

    #[test]
    fn ground_shadow_density_parses_and_clamps() {
        let lua = "groundShadowDensity = 0.65,";
        let mut s = MapSettings::default();
        apply_mapinfo_overrides(lua, &mut s);
        assert!((s.lighting.ground_shadow_density.unwrap() - 0.65).abs() < 1e-6);

        let lua_high = "groundShadowDensity = 1.4,";
        let mut s_high = MapSettings::default();
        apply_mapinfo_overrides(lua_high, &mut s_high);
        assert_eq!(s_high.lighting.ground_shadow_density, Some(1.0));

        let lua_low = "groundShadowDensity = -0.1,";
        let mut s_low = MapSettings::default();
        apply_mapinfo_overrides(lua_low, &mut s_low);
        assert_eq!(s_low.lighting.ground_shadow_density, Some(0.0));

        // Missing key leaves the field as None (resolves to engine
        // default 0.8 at read time).
        let mut s_default = MapSettings::default();
        apply_mapinfo_overrides("", &mut s_default);
        assert_eq!(s_default.lighting.ground_shadow_density, None);
    }

    #[test]
    fn four_element_sun_dir_picks_up_intensity() {
        let lua = "sunDir = { -0.64, 0.66, -0.57, 0.75 },";
        let mut settings = MapSettings::default();
        apply_mapinfo_overrides(lua, &mut settings);
        assert_eq!(settings.lighting.sun_dir, Some([-0.64, 0.66, -0.57]));
        assert!((settings.lighting.sun_intensity.unwrap() - 0.75).abs() < 1e-6);
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
            max_height: Some(261.0),
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
    fn parses_onyx_cauldron_custom_grass_block() {
        // Format taken from Onyx Cauldron 2.2.2's mapinfo.lua. The
        // grassConfig sub-table sits inside `custom = { ... }` along
        // with `fog`; we should pick up the grass keys without
        // interfering with the fog parser.
        let lua = r#"
custom = {
    fog = {
        color = {0.71, 0.71, 0.86},
        height = "40%",
        fogatten = 0.0075,
    },
    grassConfig = {
        grassDistTGA = "maps/Onyx Cauldron 2.0_grassDist.tga",
        grassMaxSize = 2.0,
        grassBladeColorTex = "maps/grass_field_mixed.dds.cached.dds",
        grassShaderParams = {
            MAPCOLORFACTOR = 0.2,
            MAPCOLORBASE = 0.6,
        },
    },
}
"#;
        let mut settings = MapSettings {
            max_height: Some(1000.0),
            ..MapSettings::default()
        };
        apply_mapinfo_overrides(lua, &mut settings);
        assert_eq!(
            settings.custom_grass.dist_tga.as_deref(),
            Some("maps/Onyx Cauldron 2.0_grassDist.tga")
        );
        assert_eq!(
            settings.custom_grass.blade_color_tex.as_deref(),
            Some("maps/grass_field_mixed.dds.cached.dds")
        );
        assert!((settings.custom_grass.max_size.unwrap() - 2.0).abs() < 1e-6);
        assert!((settings.custom_grass.map_color_factor.unwrap() - 0.2).abs() < 1e-6);
        assert!((settings.custom_grass.map_color_base.unwrap() - 0.6).abs() < 1e-6);
        // The neighbouring fog block must still parse correctly.
        assert!(settings.custom_fog.enabled);
    }

    #[test]
    fn debug_parses_real_onyx_block() {
        // Verbatim from Onyx Cauldron 2.2.2 mapinfo.lua (custom block
        // only). Catches issues like extract_table_body losing the
        // sub-table when a sibling table (`fog`, `clouds`) sits
        // between or after grassConfig.
        let lua = r#"
custom = {
    grassConfig= {
        grassDistTGA = "maps/Onyx Cauldron 2.0_grassDist.tga",
        grassMaxSize = 2.0,
        grassBladeColorTex = "maps/grass_field_mixed.dds.cached.dds", -- rgb + alpha transp
        grassShaderParams = { -- allcaps because thats how i know
            MAPCOLORFACTOR = 0.2, -- how much effect the minimapcolor has
            MAPCOLORBASE = 0.6,     --how much more to blend the bottom of the grass patches into map color
        },
    },
    fog = {
        color    = {0.71, 0.71, 0.86},
        height   = "40%",
        fogatten = 0.0075,
    },
    clouds = {
        speed = 0.05,
        color    = {0.9, 0.9, 0.9},
        height   = 1100,
        bottom = 300,
    },
},
"#;
        let mut settings = MapSettings {
            max_height: Some(980.0),
            ..MapSettings::default()
        };
        apply_mapinfo_overrides(lua, &mut settings);
        assert_eq!(
            settings.custom_grass.dist_tga.as_deref(),
            Some("maps/Onyx Cauldron 2.0_grassDist.tga")
        );
        assert_eq!(
            settings.custom_grass.blade_color_tex.as_deref(),
            Some("maps/grass_field_mixed.dds.cached.dds")
        );
        assert!(settings.custom_fog.enabled);
    }

    #[test]
    fn parses_onyx_style_team_start_positions() {
        // Source format Onyx Cauldron uses: numeric-indexed teams
        // table, each entry has a `startPos = { x, z }` sub-table.
        let lua = r#"
teams = {
    [0] = {startPos = {x = 1778, z = 1695}},
    [1] = {startPos = {x = 6482, z = 6628}},
},
"#;
        let mut settings = MapSettings::default();
        apply_mapinfo_overrides(lua, &mut settings);
        assert_eq!(settings.start_positions.len(), 2);
        assert_eq!(settings.start_positions[0], [1778, 1695]);
        assert_eq!(settings.start_positions[1], [6482, 6628]);
    }

    #[test]
    fn teams_block_absent_leaves_start_positions_empty() {
        let lua = "name = \"Test\"";
        let mut settings = MapSettings::default();
        apply_mapinfo_overrides(lua, &mut settings);
        assert!(settings.start_positions.is_empty());
    }

    #[test]
    fn custom_grass_without_dist_tga_disabled() {
        let lua = r#"
custom = {
    grassConfig = {
        grassMaxSize = 2.0,
        -- no grassDistTGA -> widget has no patches to seed from
    },
}
"#;
        let mut settings = MapSettings::default();
        apply_mapinfo_overrides(lua, &mut settings);
        assert!(settings
            .custom_grass
            .dist_tga
            .as_deref()
            .map(str::is_empty)
            .unwrap_or(true));
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
        assert_eq!(a.min_wind, Some(3.0));
        assert_eq!(a.max_wind, Some(16.0));
        assert!((a.fog_start.unwrap() - 0.8).abs() < 1e-6);
        assert_eq!(a.fog_color, Some([0.8, 0.6, 0.5]));
        assert_eq!(a.sun_color, Some([1.0, 0.7, 0.7]));
        assert_eq!(a.sky_color, Some([0.2, 0.25, 0.05]));
        assert_eq!(a.sky_dir, Some([0.0, 0.0, -1.0]));
        assert!((a.cloud_density.unwrap() - 0.25).abs() < 1e-6);
        assert_eq!(a.cloud_color, Some([0.95, 0.85, 0.75]));
        assert_eq!(a.skybox.as_deref(), Some("cleardesert.dds"));
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
        let scales = r.splat_tex_scales.unwrap();
        assert!((scales[0] - 0.0032).abs() < 1e-6);
        assert!((scales[3] - 0.0055).abs() < 1e-6);
        let mults = r.splat_tex_mults.unwrap();
        assert!((mults[2] - 1.0).abs() < 1e-6);
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

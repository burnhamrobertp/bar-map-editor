//! Pull `mapinfo.lua` out of a `.sd7` (or read a `.lua` directly), apply it
//! over `MapSettings::default()`, and print the result as JSON. Unset fields
//! stay `None`, so the JSON is exactly what the archive explicitly declared --
//! the ground truth for the "is this field set?" corpus analysis.
//!
//! Usage: mapinfo_dump <map.sd7 | mapinfo.lua>

use std::cell::RefCell;
use std::path::Path;

use bar_project::{apply_mapinfo_overrides, MapSettings};

fn extract_mapinfo(sd7: &Path) -> Option<String> {
    let found: RefCell<Option<String>> = RefCell::new(None);
    let dest = std::env::temp_dir();
    let res = sevenz_rust::decompress_file_with_extract_fn(sd7, &dest, |entry, reader, _p| {
        let name = entry.name().to_ascii_lowercase().replace('\\', "/");
        if name == "mapinfo.lua" || name.ends_with("/mapinfo.lua") {
            let mut s = String::new();
            let _ = reader.read_to_string(&mut s);
            *found.borrow_mut() = Some(s);
            // Stop once we have it -- avoids decompressing the (huge) rest.
            return Ok(false);
        }
        Ok(true)
    });
    if let Err(e) = res {
        eprintln!("extract note: {e}");
    }
    found.into_inner()
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: mapinfo_dump <map.sd7 | mapinfo.lua>");
    let p = Path::new(&path);
    let is_lua = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("lua"))
        .unwrap_or(false);

    let lua = if is_lua {
        std::fs::read_to_string(p).ok()
    } else {
        extract_mapinfo(p)
    };

    let Some(lua) = lua else {
        eprintln!("no mapinfo.lua found in {path}");
        std::process::exit(2);
    };

    let mut settings = MapSettings::default();
    apply_mapinfo_overrides(&lua, &mut settings);
    println!("{}", serde_json::to_string(&settings).unwrap());
}

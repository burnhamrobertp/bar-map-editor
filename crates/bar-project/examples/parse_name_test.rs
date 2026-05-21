fn main() {
    let lua = std::fs::read_to_string(r"C:\Users\Robert\AppData\Local\BarEditor\BarEditor\cache\work\onyx_cauldron_2.2.2_331385913f532381\mapinfo.lua").unwrap();
    let parsed = bar_project::parse_mapinfo_string(&lua, "name");
    println!("parsed name = {:?}", parsed);
    println!(
        "parsed shortname = {:?}",
        bar_project::parse_mapinfo_string(&lua, "shortname")
    );
    println!(
        "parsed author = {:?}",
        bar_project::parse_mapinfo_string(&lua, "author")
    );
}

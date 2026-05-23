//! DDS decode inspector. Loads a DDS file through the same
//! `load_dds_2d_with_mips` path the grass widget uses, prints sample
//! pixel values from the base mip, and writes the decoded RGBA bytes
//! to a sibling PNG so the result can be opened in any image viewer
//! for byte-level comparison against the original.
//!
//! Usage:
//!   cargo run -p bar-data --example inspect_dds -- <path/to/file.dds>
//!
//! Output:
//!   1. Format + dimension summary to stdout.
//!   2. Sample pixel RGBA at five fixed coordinates (centre and the
//!      four corner quadrants).
//!   3. `<path>.bme_decoded.png` next to the source file.
//!
//! What to do with this:
//!   - Open the original DDS in paint.net (built-in DDS support on
//!     Windows) and pick the same pixel coords with the colour-picker
//!     tool. Compare the RGB triplet against this tool's stdout.
//!   - If paint.net and BME match: the BC3 decode is correct. The
//!     remaining brightness gap is downstream (sampler, modulator,
//!     gamma pass).
//!   - If paint.net and BME differ: the BC3 endpoint interpolation
//!     in `crates/bar-data/src/smt.rs::decode_dxt1_block` is the
//!     culprit -- swap it for the `image` crate's reference decoder
//!     to fix.

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: inspect_dds <path/to/file.dds>");
        std::process::exit(2);
    }
    let src = Path::new(&args[1]);
    if !src.is_file() {
        eprintln!("not a file: {}", src.display());
        std::process::exit(2);
    }

    let mips = bar_data::load_dds_2d_with_mips(src).unwrap_or_else(|e| {
        eprintln!("decode failed: {e}");
        std::process::exit(1);
    });

    let base = &mips[0];
    println!("file:        {}", src.display());
    println!("mip count:   {}", mips.len());
    println!("base size:   {} x {}", base.width, base.height);
    println!("base bytes:  {}", base.rgba.len());
    println!();
    println!("--- sample pixels (x, y) = (R, G, B, A) ---");
    let samples: &[(u32, u32)] = &[
        (base.width / 2, base.height / 2),         // centre
        (base.width / 4, base.height / 4),         // top-left quadrant
        (3 * base.width / 4, base.height / 4),     // top-right quadrant
        (base.width / 4, 3 * base.height / 4),     // bottom-left quadrant
        (3 * base.width / 4, 3 * base.height / 4), // bottom-right quadrant
    ];
    for &(x, y) in samples {
        let idx = ((y * base.width + x) * 4) as usize;
        let r = base.rgba[idx];
        let g = base.rgba[idx + 1];
        let b = base.rgba[idx + 2];
        let a = base.rgba[idx + 3];
        println!(
            "  ({:>4}, {:>4}) = ({:>3}, {:>3}, {:>3}, {:>3})  #{:02X}{:02X}{:02X}{:02X}",
            x, y, r, g, b, a, r, g, b, a
        );
    }

    // Save the decoded base mip as PNG for visual cross-check.
    let png_path = src.with_extension("bme_decoded.png");
    let img = image::RgbaImage::from_raw(base.width, base.height, base.rgba.clone())
        .expect("rgba buffer matches w*h*4");
    if let Err(e) = img.save(&png_path) {
        eprintln!("png save failed: {e}");
        std::process::exit(1);
    }
    println!();
    println!("wrote PNG:   {}", png_path.display());
}

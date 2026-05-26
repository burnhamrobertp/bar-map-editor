//! Build a set of demonstration `.barproj` directories showcasing the
//! symmetric-layout features:
//!
//!   1. Layout with `symmetry = none`         (baseline)
//!   2. Layout with `symmetry = mirror_x`     (1v1 mirror)
//!   3. Layout with `symmetry = mirror_xy`    (4-quadrant)
//!   4. Layout with `symmetry = rotate_90`    (4-corner radial)
//!   5. Mirror with `mode = mirror_x`                  (canonical half wins)
//!   6. Mirror with `mode = average_x`                 (both halves blended)
//!
//! All projects share the same three off-centre ellipse placements so
//! the visual differences come purely from the symmetry / mirror mode.
//!
//! Run with: `cargo run -p bar-project --example build_symmetric_demos -- <out-dir>`
//!
//! The output directory will contain one subdirectory per demo, each a
//! valid `.barproj` that BME can open via File -> Open Project.

use std::collections::HashMap;
use std::path::PathBuf;

use bar_graph::{NodeType, ParamValue};
use bar_project::{
    project::Project,
    recipe::{MapSettings, OutputConfig, Recipe, RecipeConnection, RecipeNode},
};

fn float(v: f32) -> ParamValue {
    ParamValue::Float(v)
}

fn uint(v: u32) -> ParamValue {
    ParamValue::UInt(v)
}

fn string(s: &str) -> ParamValue {
    ParamValue::String(s.to_string())
}

/// Three off-centre ellipse placements shared across the Layout
/// demos. Off-centre on purpose so the symmetry transforms have visible
/// effects -- a centre-of-canvas shape is its own mirror.
fn layout_shapes_params(symmetry: &str) -> HashMap<String, ParamValue> {
    let mut m = HashMap::new();
    m.insert("item_count".to_string(), uint(3));
    m.insert("symmetry".to_string(), string(symmetry));

    let shapes: &[(f32, f32, f32, f32, f32, &str)] = &[
        // (x, y, rx, ry, height, type)
        (0.25, 0.50, 0.12, 0.12, 0.80, "ellipse"),
        (0.30, 0.30, 0.08, 0.08, 0.55, "ellipse"),
        (0.40, 0.65, 0.10, 0.06, 0.65, "ellipse"),
    ];
    for (i, &(x, y, rx, ry, h, t)) in shapes.iter().enumerate() {
        m.insert(format!("type_{i}"), string(t));
        m.insert(format!("x_{i}"), float(x));
        m.insert(format!("y_{i}"), float(y));
        m.insert(format!("rx_{i}"), float(rx));
        m.insert(format!("ry_{i}"), float(ry));
        m.insert(format!("angle_{i}"), float(0.0));
        m.insert(format!("height_{i}"), float(h));
        m.insert(format!("falloff_{i}"), float(0.5));
    }
    // Remaining slots (3..8) keep default zero-height entries so they
    // contribute nothing to the composite.
    for i in 3..8usize {
        m.insert(format!("type_{i}"), string("ellipse"));
        m.insert(format!("x_{i}"), float(0.5));
        m.insert(format!("y_{i}"), float(0.5));
        m.insert(format!("rx_{i}"), float(0.2));
        m.insert(format!("ry_{i}"), float(0.2));
        m.insert(format!("angle_{i}"), float(0.0));
        m.insert(format!("height_{i}"), float(0.0));
        m.insert(format!("falloff_{i}"), float(0.5));
    }
    m
}

fn output_config() -> OutputConfig {
    OutputConfig {
        // 257 = 256 squares + 1, matching BME's default heightmap shape
        // for a 2x2 chunk map (the smallest convenient demo size).
        width: 257,
        height: 257,
        map_settings: MapSettings::default(),
    }
}

/// Layout -> FinalComposition with a given symmetry mode.
fn layout_demo(name: &str, description: &str, symmetry: &str) -> Recipe {
    Recipe {
        name: name.to_string(),
        shortname: None,
        description: description.to_string(),
        author: Some("bar-editor symmetric-layout demo".to_string()),
        version: Some("0.1".to_string()),
        tip: None,
        depend: vec!["Map Helper v1".to_string()],
        nodes: vec![
            RecipeNode {
                key: "layout".to_string(),
                node_type: NodeType::Layout,
                label: format!("Layout (symmetry={symmetry})"),
                params: layout_shapes_params(symmetry),
            },
            RecipeNode {
                key: "output".to_string(),
                node_type: NodeType::FinalComposition,
                label: "Export".to_string(),
                params: HashMap::new(),
            },
        ],
        connections: vec![RecipeConnection {
            from: "layout.output".to_string(),
            to: "output.heightmap".to_string(),
        }],
        output: output_config(),
        features: Vec::new(),
    }
}

/// RidgedNoise -> Mirror -> FinalComposition. The same noise seed feeds
/// every Mirror demo so the only difference is the `mode` param.
fn mirror_demo(name: &str, description: &str, mode: &str) -> Recipe {
    Recipe {
        name: name.to_string(),
        shortname: None,
        description: description.to_string(),
        author: Some("bar-editor symmetric-layout demo".to_string()),
        version: Some("0.1".to_string()),
        tip: None,
        depend: vec!["Map Helper v1".to_string()],
        nodes: vec![
            RecipeNode {
                key: "noise".to_string(),
                node_type: NodeType::RidgedNoise,
                label: "Asymmetric noise".to_string(),
                params: HashMap::from([
                    ("frequency".to_string(), float(2.5)),
                    ("octaves".to_string(), uint(5)),
                    ("lacunarity".to_string(), float(2.0)),
                    ("persistence".to_string(), float(0.5)),
                    ("seed".to_string(), uint(42)),
                ]),
            },
            RecipeNode {
                key: "mirror".to_string(),
                node_type: NodeType::Mirror,
                label: format!("Mirror (mode={mode})"),
                params: HashMap::from([("mode".to_string(), string(mode))]),
            },
            RecipeNode {
                key: "output".to_string(),
                node_type: NodeType::FinalComposition,
                label: "Export".to_string(),
                params: HashMap::new(),
            },
        ],
        connections: vec![
            RecipeConnection {
                from: "noise.output".to_string(),
                to: "mirror.input".to_string(),
            },
            RecipeConnection {
                from: "mirror.output".to_string(),
                to: "output.heightmap".to_string(),
            },
        ],
        output: output_config(),
        features: Vec::new(),
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let out_root = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        // Default location: workspace `examples/symmetric-layout-demos/`.
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // crates/bar-project
        manifest_dir
            .parent() // crates/
            .and_then(|p| p.parent()) // workspace root
            .map(|p| p.join("examples").join("symmetric-layout-demos"))
            .unwrap_or_else(|| PathBuf::from("symmetric-layout-demos"))
    };

    println!("Writing demos to: {}", out_root.display());
    std::fs::create_dir_all(&out_root)?;

    let demos: &[(&str, Recipe)] = &[
        (
            "1-layout-symmetry-none.barproj",
            layout_demo(
                "Layout: symmetry=none",
                "Three off-centre ellipses with no symmetry. Compare to the other layout demos to see how each symmetry mode duplicates these shapes.",
                "none",
            ),
        ),
        (
            "2-layout-symmetry-mirror-x.barproj",
            layout_demo(
                "Layout: symmetry=mirror_x",
                "Same three off-centre ellipses with symmetry=mirror_x. Each shape gets a partner reflected across the vertical centre line -- the canonical 1v1 BAR symmetry.",
                "mirror_x",
            ),
        ),
        (
            "3-layout-symmetry-mirror-xy.barproj",
            layout_demo(
                "Layout: symmetry=mirror_xy",
                "Same shapes with 4-quadrant symmetry. Each shape produces 4 copies (original + 3 reflections). Good for 2v2 corner-symmetric maps.",
                "mirror_xy",
            ),
        ),
        (
            "4-layout-symmetry-rotate-90.barproj",
            layout_demo(
                "Layout: symmetry=rotate_90",
                "Same shapes with 90-degree radial symmetry. Each shape produces 4 rotated copies about the centre. Good for 4-player ffa / radial maps.",
                "rotate_90",
            ),
        ),
        (
            "5-mirror-replace-x.barproj",
            mirror_demo(
                "Mirror: mode=mirror_x (replace)",
                "RidgedNoise fed into Mirror with the original replace-style mode. The right half is discarded and replaced with a copy of the left.",
                "mirror_x",
            ),
        ),
        (
            "6-mirror-average-x.barproj",
            mirror_demo(
                "Mirror: mode=average_x (blend)",
                "Same RidgedNoise input, but Mirror now uses average_x. Both halves contribute -- each output pixel is the mean of the matching pair. Detail from both halves survives.",
                "average_x",
            ),
        ),
    ];

    for (name, recipe) in demos {
        let dir = out_root.join(name);
        Project::from_recipe(recipe.clone()).save(&dir)?;
        println!("  wrote {}", dir.display());
    }

    println!();
    println!("Done. Open any of the demos in BME via File -> Open Project,");
    println!("or run `bar-cli run <demo>/recipe.json --target spring-smf -o <out>`");
    println!("to export a heightmap PNG directly.");
    Ok(())
}

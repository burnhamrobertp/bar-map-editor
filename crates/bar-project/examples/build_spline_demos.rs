//! Build demonstration `.barproj` directories for the SplineLayout
//! node and the shared canvas editor.
//!
//! Each demo carries a hand-picked control-point sequence so the
//! visual output is meaningful out-of-the-box; once opened in BME the
//! author can drag points around to see live updates.
//!
//! Run with: `cargo run -p bar-project --example build_spline_demos -- <out-dir>`

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

fn string(s: &str) -> ParamValue {
    ParamValue::String(s.to_string())
}

fn boolean(v: bool) -> ParamValue {
    ParamValue::Bool(v)
}

fn spline(points: Vec<[f32; 2]>) -> ParamValue {
    ParamValue::Spline(points)
}

fn output_config() -> OutputConfig {
    OutputConfig {
        width: 257,
        height: 257,
        map_settings: MapSettings::default(),
    }
}

/// SplineLayout -> FinalComposition. Standalone demo with no noise mix.
fn spline_demo(
    name: &str,
    description: &str,
    points: Vec<[f32; 2]>,
    mode: &str,
    amplitude: f32,
    width: f32,
    closed: bool,
    symmetry: &str,
) -> Recipe {
    let mut params = HashMap::new();
    params.insert("points".to_string(), spline(points));
    params.insert("mode".to_string(), string(mode));
    params.insert("amplitude".to_string(), float(amplitude));
    params.insert("width".to_string(), float(width));
    params.insert("falloff".to_string(), float(0.5));
    params.insert("closed".to_string(), boolean(closed));
    params.insert("symmetry".to_string(), string(symmetry));

    Recipe {
        name: name.to_string(),
        shortname: None,
        description: description.to_string(),
        author: Some("bar-editor spline-layout demo".to_string()),
        version: Some("0.1".to_string()),
        tip: None,
        depend: vec!["Map Helper v1".to_string()],
        nodes: vec![
            RecipeNode {
                key: "spline".to_string(),
                node_type: NodeType::SplineLayout,
                label: format!("SplineLayout ({mode}, symmetry={symmetry})"),
                params,
            },
            RecipeNode {
                key: "output".to_string(),
                node_type: NodeType::FinalComposition,
                label: "Export".to_string(),
                params: HashMap::new(),
            },
        ],
        connections: vec![RecipeConnection {
            from: "spline.output".to_string(),
            to: "output.heightmap".to_string(),
        }],
        output: output_config(),
        features: Vec::new(),
    }
}

/// RidgedNoise + SplineLayout (valley) added together via `Add` to
/// show how a carved river fits into noise terrain.
fn river_in_noise_demo() -> Recipe {
    let mut spline_params = HashMap::new();
    spline_params.insert(
        "points".to_string(),
        spline(vec![
            [0.05, 0.35],
            [0.30, 0.55],
            [0.55, 0.30],
            [0.80, 0.60],
            [0.95, 0.45],
        ]),
    );
    spline_params.insert("mode".to_string(), string("valley"));
    spline_params.insert("amplitude".to_string(), float(0.4));
    spline_params.insert("width".to_string(), float(0.04));
    spline_params.insert("falloff".to_string(), float(0.5));
    spline_params.insert("closed".to_string(), boolean(false));
    spline_params.insert("symmetry".to_string(), string("none"));

    Recipe {
        name: "Spline valley cut into noise".to_string(),
        shortname: None,
        description:
            "Ridged noise base with a SplineLayout valley running across it. Demonstrates how the valley mode composites with a base heightmap via Multiply, carving a river-like channel through the underlying terrain."
                .to_string(),
        author: Some("bar-editor spline-layout demo".to_string()),
        version: Some("0.1".to_string()),
        tip: None,
        depend: vec!["Map Helper v1".to_string()],
        nodes: vec![
            RecipeNode {
                key: "noise".to_string(),
                node_type: NodeType::RidgedNoise,
                label: "Ridged terrain".to_string(),
                params: HashMap::from([
                    ("frequency".to_string(), float(3.0)),
                    ("octaves".to_string(), ParamValue::UInt(5)),
                    ("lacunarity".to_string(), float(2.0)),
                    ("persistence".to_string(), float(0.5)),
                    ("seed".to_string(), ParamValue::UInt(7)),
                ]),
            },
            RecipeNode {
                key: "spline".to_string(),
                node_type: NodeType::SplineLayout,
                label: "River valley".to_string(),
                params: spline_params,
            },
            RecipeNode {
                key: "carve".to_string(),
                node_type: NodeType::Multiply,
                label: "Multiply (carve)".to_string(),
                params: HashMap::new(),
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
                to: "carve.a".to_string(),
            },
            RecipeConnection {
                from: "spline.output".to_string(),
                to: "carve.b".to_string(),
            },
            RecipeConnection {
                from: "carve.output".to_string(),
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
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // crates/bar-project
        manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("examples").join("spline-layout-demos"))
            .unwrap_or_else(|| PathBuf::from("spline-layout-demos"))
    };

    println!("Writing demos to: {}", out_root.display());
    std::fs::create_dir_all(&out_root)?;

    let demos: &[(&str, Recipe)] = &[
        (
            "1-ridge-curved.barproj",
            spline_demo(
                "Ridge: curved line",
                "Four control points forming an S-curve. Ridge mode raises a thin elevation band along the Catmull-Rom curve through the points.",
                vec![[0.15, 0.70], [0.40, 0.40], [0.60, 0.65], [0.85, 0.30]],
                "ridge",
                0.7,
                0.05,
                false,
                "none",
            ),
        ),
        (
            "2-valley-river.barproj",
            spline_demo(
                "Valley: river cut",
                "Same S-curve but in valley mode. Output reads as a carved trough rather than a raised ridge. Baseline elevation is amplitude (0.4); the channel pixels sit at zero.",
                vec![[0.10, 0.30], [0.35, 0.55], [0.65, 0.40], [0.90, 0.65]],
                "valley",
                0.4,
                0.04,
                false,
                "none",
            ),
        ),
        (
            "3-mask-corridor.barproj",
            spline_demo(
                "Mask: corridor",
                "Mask mode emits a 0..1 weight along the spline; pixels inside the falloff radius are bright, the rest dark. Useful as input to TextureWeightmap or as a selector for downstream operations.",
                vec![[0.1, 0.5], [0.5, 0.5], [0.9, 0.5]],
                "mask",
                1.0,
                0.08,
                false,
                "none",
            ),
        ),
        (
            "4-closed-atoll.barproj",
            spline_demo(
                "Closed loop: atoll",
                "Five control points wrapped in a closed loop -- the curve completes back to the first point. Output is a continuous ridge ring (atoll / crater rim).",
                vec![
                    [0.30, 0.30],
                    [0.70, 0.30],
                    [0.85, 0.55],
                    [0.55, 0.80],
                    [0.20, 0.60],
                ],
                "ridge",
                0.8,
                0.05,
                true,
                "none",
            ),
        ),
        (
            "5-mirror-rivers.barproj",
            spline_demo(
                "Symmetry: mirror_x river pair",
                "A single river spline authored in the left half plus mirror_x symmetry produces two mirrored rivers -- the natural shape for 1v1 BAR maps where each side gets the same waterway.",
                vec![[0.10, 0.20], [0.25, 0.45], [0.20, 0.70], [0.40, 0.90]],
                "valley",
                0.5,
                0.05,
                false,
                "mirror_x",
            ),
        ),
        (
            "6-rotate-radial.barproj",
            spline_demo(
                "Symmetry: rotate_90 radial channels",
                "Single short ridge with rotate_90 symmetry. Produces four channels radiating from the centre at 90 degrees -- useful for 4-player FFA layouts.",
                vec![[0.5, 0.5], [0.6, 0.45], [0.75, 0.45], [0.9, 0.5]],
                "ridge",
                0.7,
                0.04,
                false,
                "rotate_90",
            ),
        ),
        ("7-river-in-noise.barproj", river_in_noise_demo()),
    ];

    for (name, recipe) in demos {
        let dir = out_root.join(name);
        Project::from_recipe(recipe.clone()).save(&dir)?;
        println!("  wrote {}", dir.display());
    }

    println!();
    println!("Done. Open any demo via File -> Open Project, or render");
    println!("a heightmap PNG with `bar-cli run <demo>/recipe.json --target raw-layers -o <out>`.");
    Ok(())
}

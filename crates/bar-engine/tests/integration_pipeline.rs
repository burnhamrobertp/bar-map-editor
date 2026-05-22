//! Integration test: full CLI pipeline via the bar-engine library.
//! This exercises the same path as `om run` without spawning a subprocess.

use std::fs;

use bar_engine::recipe::{MapSettings, Recipe};
use bar_engine::{export_heightmap_png, export_smf, CpuExecutor};

#[test]
fn test_sample_recipe_full_pipeline() {
    let recipe = Recipe::sample();
    let graph = recipe.build_graph().unwrap();
    let executor = CpuExecutor;

    let dir = std::env::temp_dir().join("om_integration_test");
    fs::create_dir_all(&dir).unwrap();

    let smf_path = dir.join("test.smf");
    let png_path = dir.join("test.png");

    // Export SMF
    export_smf(
        &graph,
        &executor,
        recipe.output.width,
        recipe.output.height,
        &smf_path,
    )
    .unwrap();

    // Export PNG
    export_heightmap_png(
        &graph,
        &executor,
        recipe.output.width,
        recipe.output.height,
        &png_path,
    )
    .unwrap();

    // Verify SMF
    let smf_meta = fs::metadata(&smf_path).unwrap();
    assert!(smf_meta.len() > 0, "SMF file should not be empty");

    // Verify PNG
    let img = image::open(&png_path).unwrap();
    assert_eq!(img.width(), recipe.output.width);
    assert_eq!(img.height(), recipe.output.height);

    // Cleanup
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_recipe_json_roundtrip() {
    let recipe = Recipe::sample();
    let json = recipe.to_json().unwrap();

    // Verify it's valid JSON that parses back
    let parsed = Recipe::from_json(&json).unwrap();
    assert_eq!(parsed.name, recipe.name);
    assert_eq!(parsed.nodes.len(), recipe.nodes.len());
    assert_eq!(parsed.connections.len(), recipe.connections.len());
    assert_eq!(parsed.output.width, recipe.output.width);
}

#[test]
fn test_recipe_with_override_dimensions() {
    let recipe = Recipe::sample();
    let graph = recipe.build_graph().unwrap();
    let executor = CpuExecutor;

    // Override to smaller size for speed
    let w = 65;
    let h = 65;

    let dir = std::env::temp_dir().join("om_integration_override");
    fs::create_dir_all(&dir).unwrap();

    let png_path = dir.join("override.png");
    export_heightmap_png(&graph, &executor, w, h, &png_path).unwrap();

    let img = image::open(&png_path).unwrap();
    assert_eq!(img.width(), w);
    assert_eq!(img.height(), h);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_complex_recipe_with_combiners() {
    // Build a more complex recipe programmatically
    use bar_engine::recipe::{OutputConfig, RecipeConnection, RecipeNode};
    use bar_engine::{NodeType, ParamValue};
    use std::collections::HashMap;

    let recipe = Recipe {
        name: "Complex Test".to_string(),
        shortname: None,
        description: String::new(),
        author: None,
        version: None,
        tip: None,
        depend: vec!["Map Helper v1".to_string()],
        nodes: vec![
            RecipeNode {
                key: "perlin".to_string(),
                node_type: NodeType::PerlinNoise,
                label: "Perlin".to_string(),
                params: HashMap::from([
                    ("frequency".to_string(), ParamValue::Float(4.0)),
                    ("seed".to_string(), ParamValue::UInt(1)),
                ]),
            },
            RecipeNode {
                key: "simplex".to_string(),
                node_type: NodeType::SimplexNoise,
                label: "Simplex".to_string(),
                params: HashMap::from([
                    ("frequency".to_string(), ParamValue::Float(6.0)),
                    ("seed".to_string(), ParamValue::UInt(2)),
                ]),
            },
            RecipeNode {
                key: "add".to_string(),
                node_type: NodeType::Add,
                label: "Add".to_string(),
                params: HashMap::new(),
            },
            RecipeNode {
                key: "clamp".to_string(),
                node_type: NodeType::Clamp,
                label: "Clamp".to_string(),
                params: HashMap::from([
                    ("min".to_string(), ParamValue::Float(0.1)),
                    ("max".to_string(), ParamValue::Float(0.9)),
                ]),
            },
            RecipeNode {
                key: "out".to_string(),
                node_type: NodeType::FinalComposition,
                label: "Export".to_string(),
                params: HashMap::new(),
            },
        ],
        connections: vec![
            RecipeConnection {
                from: "perlin.output".to_string(),
                to: "add.a".to_string(),
            },
            RecipeConnection {
                from: "simplex.output".to_string(),
                to: "add.b".to_string(),
            },
            RecipeConnection {
                from: "add.output".to_string(),
                to: "clamp.input".to_string(),
            },
            RecipeConnection {
                from: "clamp.output".to_string(),
                to: "out.heightmap".to_string(),
            },
        ],
        output: OutputConfig {
            width: 64,
            height: 64,
            map_settings: MapSettings::default(),
        },
        features: Vec::new(),
    };

    recipe.validate().unwrap();
    let graph = recipe.build_graph().unwrap();
    let executor = CpuExecutor;

    let dir = std::env::temp_dir().join("om_integration_complex");
    fs::create_dir_all(&dir).unwrap();
    let png_path = dir.join("complex.png");

    export_heightmap_png(&graph, &executor, 64, 64, &png_path).unwrap();

    let img = image::open(&png_path).unwrap();
    assert_eq!(img.width(), 64);
    assert_eq!(img.height(), 64);

    // Verify clamping worked: all pixel values should be between 0.1*65535 and 0.9*65535
    let gray = img.to_luma16();
    let min_expected = (0.1 * 65535.0) as u16;
    let max_expected = (0.9 * 65535.0) as u16;
    for pixel in gray.pixels() {
        let v = pixel.0[0];
        assert!(
            v >= min_expected.saturating_sub(1) && v <= max_expected + 1,
            "Pixel value {} outside clamped range [{}, {}]",
            v,
            min_expected,
            max_expected
        );
    }

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_recipe_with_erosion_pipeline() {
    // Build a recipe that chains hydraulic then thermal erosion
    use bar_engine::recipe::{OutputConfig, RecipeConnection, RecipeNode};
    use bar_engine::{NodeType, ParamValue};
    use std::collections::HashMap;

    let recipe = Recipe {
        name: "Erosion Test".to_string(),
        shortname: None,
        description: "Tests hydraulic erosion in pipeline".to_string(),
        author: None,
        version: None,
        tip: None,
        depend: vec!["Map Helper v1".to_string()],
        nodes: vec![
            RecipeNode {
                key: "noise".to_string(),
                node_type: NodeType::PerlinNoise,
                label: "Base Terrain".to_string(),
                params: HashMap::from([
                    ("frequency".to_string(), ParamValue::Float(3.0)),
                    ("octaves".to_string(), ParamValue::UInt(6)),
                    ("seed".to_string(), ParamValue::UInt(42)),
                ]),
            },
            RecipeNode {
                key: "erode".to_string(),
                node_type: NodeType::HydraulicErosion,
                label: "Hydraulic Erosion".to_string(),
                params: HashMap::from([
                    ("iterations".to_string(), ParamValue::UInt(2000)),
                    ("erosion_rate".to_string(), ParamValue::Float(0.3)),
                    ("deposition_rate".to_string(), ParamValue::Float(0.3)),
                    ("max_lifetime".to_string(), ParamValue::UInt(25)),
                ]),
            },
            RecipeNode {
                key: "thermal".to_string(),
                node_type: NodeType::ThermalErosion,
                label: "Thermal Erosion".to_string(),
                params: HashMap::from([
                    ("iterations".to_string(), ParamValue::UInt(30)),
                    ("talus_angle".to_string(), ParamValue::Float(0.005)),
                    ("erosion_rate".to_string(), ParamValue::Float(0.5)),
                ]),
            },
            RecipeNode {
                key: "out".to_string(),
                node_type: NodeType::FinalComposition,
                label: "Export".to_string(),
                params: HashMap::new(),
            },
        ],
        connections: vec![
            RecipeConnection {
                from: "noise.output".to_string(),
                to: "erode.input".to_string(),
            },
            RecipeConnection {
                from: "erode.output".to_string(),
                to: "thermal.input".to_string(),
            },
            RecipeConnection {
                from: "thermal.output".to_string(),
                to: "out.heightmap".to_string(),
            },
        ],
        output: OutputConfig {
            width: 64,
            height: 64,
            map_settings: MapSettings::default(),
        },
        features: Vec::new(),
    };

    recipe.validate().unwrap();
    let graph = recipe.build_graph().unwrap();
    let executor = CpuExecutor;

    let dir = std::env::temp_dir().join("om_integration_erosion");
    fs::create_dir_all(&dir).unwrap();

    let png_path = dir.join("eroded.png");
    export_heightmap_png(&graph, &executor, 64, 64, &png_path).unwrap();

    let img = image::open(&png_path).unwrap();
    assert_eq!(img.width(), 64);
    assert_eq!(img.height(), 64);

    // Verify erosion produced visible terrain variation (not flat)
    let gray = img.to_luma16();
    let values: Vec<u16> = gray.pixels().map(|p| p.0[0]).collect();
    let min_v = *values.iter().min().unwrap();
    let max_v = *values.iter().max().unwrap();
    assert!(
        max_v - min_v > 5000,
        "Eroded terrain should have significant variation: range={}",
        max_v - min_v
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_mask_operations_pipeline() {
    use bar_engine::recipe::{OutputConfig, RecipeConnection, RecipeNode};
    use bar_engine::{NodeType, ParamValue};
    use std::collections::HashMap;

    let recipe = Recipe {
        name: "Mask Test".to_string(),
        shortname: None,
        description: "Tests mask threshold, invert, blur, and apply".to_string(),
        author: None,
        version: None,
        tip: None,
        depend: vec!["Map Helper v1".to_string()],
        nodes: vec![
            RecipeNode {
                key: "terrain".to_string(),
                node_type: NodeType::PerlinNoise,
                label: "Terrain".to_string(),
                params: HashMap::from([
                    ("frequency".to_string(), ParamValue::Float(3.0)),
                    ("octaves".to_string(), ParamValue::UInt(4)),
                    ("seed".to_string(), ParamValue::UInt(1)),
                ]),
            },
            RecipeNode {
                key: "flat".to_string(),
                node_type: NodeType::SimplexNoise,
                label: "Flat".to_string(),
                params: HashMap::from([
                    ("frequency".to_string(), ParamValue::Float(1.0)),
                    ("octaves".to_string(), ParamValue::UInt(2)),
                    ("seed".to_string(), ParamValue::UInt(99)),
                ]),
            },
            RecipeNode {
                key: "threshold".to_string(),
                node_type: NodeType::MaskThreshold,
                label: "Threshold".to_string(),
                params: HashMap::from([
                    ("threshold".to_string(), ParamValue::Float(0.5)),
                    ("smoothness".to_string(), ParamValue::Float(0.1)),
                ]),
            },
            RecipeNode {
                key: "blur".to_string(),
                node_type: NodeType::MaskBlur,
                label: "Blur Mask".to_string(),
                params: HashMap::from([("radius".to_string(), ParamValue::Float(3.0))]),
            },
            RecipeNode {
                key: "apply".to_string(),
                node_type: NodeType::MaskApply,
                label: "Masked Blend".to_string(),
                params: HashMap::new(),
            },
            RecipeNode {
                key: "out".to_string(),
                node_type: NodeType::FinalComposition,
                label: "Export".to_string(),
                params: HashMap::new(),
            },
        ],
        connections: vec![
            RecipeConnection {
                from: "terrain.output".to_string(),
                to: "threshold.input".to_string(),
            },
            RecipeConnection {
                from: "threshold.output".to_string(),
                to: "blur.input".to_string(),
            },
            RecipeConnection {
                from: "terrain.output".to_string(),
                to: "apply.input".to_string(),
            },
            RecipeConnection {
                from: "blur.output".to_string(),
                to: "apply.mask".to_string(),
            },
            RecipeConnection {
                from: "flat.output".to_string(),
                to: "apply.background".to_string(),
            },
            RecipeConnection {
                from: "apply.output".to_string(),
                to: "out.heightmap".to_string(),
            },
        ],
        output: OutputConfig {
            width: 64,
            height: 64,
            map_settings: MapSettings::default(),
        },
        features: Vec::new(),
    };

    recipe.validate().unwrap();
    let graph = recipe.build_graph().unwrap();
    let executor = CpuExecutor;

    let dir = std::env::temp_dir().join("om_integration_masks");
    fs::create_dir_all(&dir).unwrap();

    let png_path = dir.join("masked.png");
    export_heightmap_png(&graph, &executor, 64, 64, &png_path).unwrap();

    let img = image::open(&png_path).unwrap();
    assert_eq!(img.width(), 64);
    assert_eq!(img.height(), 64);

    let gray = img.to_luma16();
    let values: Vec<u16> = gray.pixels().map(|p| p.0[0]).collect();
    let min_v = *values.iter().min().unwrap();
    let max_v = *values.iter().max().unwrap();
    assert!(
        max_v - min_v > 1000,
        "Masked terrain should have variation: range={}",
        max_v - min_v
    );

    fs::remove_dir_all(&dir).ok();
}

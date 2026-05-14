//! `om preview-macro` — headless macro renderer for value-iteration.
//!
//! Used as a value-tuning toolchain: drop a macro name and (optional)
//! knob overrides on the command line, get a grayscale heightmap PNG
//! out. Lets a developer (or AI assistant) iterate on macro defaults
//! without launching the GUI.
//!
//! The macro JSON shape mirrors what `bar-gui::macros::MacroTemplate`
//! parses, but is duplicated here so `bar-cli` doesn't pull in
//! `bar-gui`'s egui dependency.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

use bar_engine::recipe::{
    MapSettings, OutputConfig, Recipe, RecipeConnection, RecipeNode, RECIPE_SCHEMA_VERSION,
};
use bar_engine::CpuExecutor;
use bar_graph::{evaluate_graph, NodeType, ParamValue, PortValue};

#[derive(Debug, Deserialize)]
struct MacroTemplate {
    #[allow(dead_code)]
    name: String,
    nodes: Vec<MacroNode>,
    #[serde(default)]
    connections: Vec<MacroConnection>,
    subgraph: MacroSubgraph,
    #[serde(default)]
    macro_params: Vec<MacroParamSpec>,
}

#[derive(Debug, Deserialize)]
struct MacroNode {
    key: String,
    #[serde(rename = "type")]
    node_type: NodeType,
    #[serde(default)]
    label: String,
    #[serde(default)]
    params: HashMap<String, ParamValue>,
}

#[derive(Debug, Deserialize)]
struct MacroConnection {
    from: String,
    to: String,
}

#[derive(Debug, Deserialize)]
struct MacroSubgraph {
    #[serde(default)]
    outputs: Vec<MacroPort>,
}

#[derive(Debug, Deserialize)]
struct MacroPort {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    label: String,
    #[allow(dead_code)]
    kind: String,
    binding: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MacroParamSpec {
    name: String,
    kind: String,
    /// `node_key:param_key`
    binding: String,
}

/// Render a macro as a heightmap PNG. Optional knob overrides written
/// through the macro's `macro_params` bindings; optional `--seed`
/// rewrites every UInt-typed `seed` param across inner nodes.
pub fn run(
    macro_arg: &str,
    knobs: &[String],
    seed: Option<u32>,
    width: u32,
    height: u32,
    out_dir: &Path,
) -> Result<()> {
    let template =
        load_macro(macro_arg).with_context(|| format!("Failed to load macro: {macro_arg}"))?;

    // Parse `--knob name=value` pairs into a map.
    let knob_map = parse_knobs(knobs)?;

    // Build a Recipe from the macro: every inner node, plus a Bundler
    // wired to the macro's first heightmap-typed output. This is the
    // smallest evaluable graph the macro represents.
    let recipe = build_recipe(&template, &knob_map, seed, width, height)?;

    // Evaluate.
    let graph = recipe
        .build_graph()
        .context("Recipe graph construction failed")?;
    let executor = CpuExecutor;
    let outputs = evaluate_graph(
        &graph,
        &executor,
        width,
        height,
        (width - 1) * 8,
        (height - 1) * 8,
    )
    .map_err(|e| anyhow!("Graph evaluation failed: {e:?}"))?;

    // Pull the heightmap out of the bundler input.
    let bundler_id = bar_engine::find_bundler_nodes(&graph)
        .first()
        .copied()
        .ok_or_else(|| anyhow!("No bundler in built recipe (internal error)"))?;
    let heightmap = bar_graph::get_bundler_node_heightmap(&graph, &outputs, bundler_id)
        .ok_or_else(|| anyhow!("No heightmap available — macro's output binding may be wrong"))?;

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("Cannot create output dir: {}", out_dir.display()))?;
    let height_path = out_dir.join("heightmap.png");
    bar_engine::export::write_heightmap_png(&heightmap, &height_path)
        .with_context(|| format!("Failed to write {}", height_path.display()))?;

    // Diagnostic: data range so I can see whether the macro is
    // producing flat output (bug) vs. legitimately gentle terrain.
    let data = heightmap.data();
    let (min, max) = data
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    let mean: f32 = data.iter().sum::<f32>() / data.len() as f32;

    println!(
        "Wrote {} ({width}x{height})\n\
         Range: [{min:.3}, {max:.3}], mean {mean:.3}",
        height_path.display()
    );

    // Try to also produce an AutoTexture preview by tacking one onto
    // the graph if the macro doesn't already have a texture binding.
    // Skipped for now to keep the toolchain heightmap-focused.

    Ok(())
}

fn load_macro(arg: &str) -> Result<MacroTemplate> {
    // If `arg` looks like a path that exists, read from disk; else
    // resolve against `assets/macros/<arg>.json` from the current
    // working directory (the CWD when running cargo from the repo
    // root will hit this).
    let candidate = if Path::new(arg).is_file() {
        PathBuf::from(arg)
    } else {
        let stem = arg.trim_end_matches(".json");
        PathBuf::from("assets/macros").join(format!("{stem}.json"))
    };
    let text = std::fs::read_to_string(&candidate)
        .with_context(|| format!("read {}", candidate.display()))?;
    let tpl: MacroTemplate = serde_json::from_str(&text)
        .with_context(|| format!("parse macro JSON {}", candidate.display()))?;
    Ok(tpl)
}

fn parse_knobs(knobs: &[String]) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    for k in knobs {
        let (name, val) = k
            .split_once('=')
            .ok_or_else(|| anyhow!("--knob expects name=value, got '{k}'"))?;
        map.insert(name.to_string(), val.to_string());
    }
    Ok(map)
}

fn build_recipe(
    template: &MacroTemplate,
    knobs: &HashMap<String, String>,
    seed: Option<u32>,
    width: u32,
    height: u32,
) -> Result<Recipe> {
    // Find the macro's first heightmap-typed output binding so we
    // know which inner port becomes the bundler's heightmap input.
    let height_output = template
        .subgraph
        .outputs
        .iter()
        .find(|p| p.binding.is_some())
        .ok_or_else(|| anyhow!("Macro has no output port with a binding"))?;
    let height_binding = height_output
        .binding
        .as_ref()
        .ok_or_else(|| anyhow!("Macro's output port has no binding"))?;

    let mut nodes: Vec<RecipeNode> = template
        .nodes
        .iter()
        .map(|n| RecipeNode {
            key: n.key.clone(),
            node_type: n.node_type.clone(),
            label: n.label.clone(),
            params: n.params.clone(),
        })
        .collect();

    // Apply knob overrides. Each macro_param has a `binding` like
    // `node_key:param_name`; find that node, write the value.
    for (knob_name, knob_val) in knobs {
        let spec = template
            .macro_params
            .iter()
            .find(|p| p.name == *knob_name)
            .ok_or_else(|| anyhow!("Unknown knob '{knob_name}' for macro '{}'", template.name))?;
        let (node_key, param_key) = spec
            .binding
            .split_once(':')
            .ok_or_else(|| anyhow!("Bad binding '{}' for knob '{knob_name}'", spec.binding))?;
        let typed = parse_param_value(&spec.kind, knob_val).with_context(|| {
            format!(
                "Knob '{knob_name}': can't parse '{knob_val}' as {}",
                spec.kind
            )
        })?;
        let node = nodes
            .iter_mut()
            .find(|n| n.key == node_key)
            .ok_or_else(|| anyhow!("Knob '{knob_name}' references unknown node '{node_key}'"))?;
        node.params.insert(param_key.to_string(), typed);
    }

    // Apply seed override across all UInt-typed `seed` params. Same
    // rule as `macros::instantiate` so the result matches what the
    // GUI would do when the user drops the macro with that seed.
    if let Some(s) = seed {
        for n in nodes.iter_mut() {
            if let Some(ParamValue::UInt(_)) = n.params.get("seed") {
                n.params.insert("seed".into(), ParamValue::UInt(s));
            }
        }
    }

    // Mirror the GUI's character→FBM side-effect: setting a noise
    // node's `character` param also writes the four FBM defaults for
    // that character. Any FBM param explicitly --knob'd by the user
    // wins over the character default. Without this, `--knob
    // character=rugged` only sets the string and the noise renders
    // with the type's default frequency/octaves/etc.
    let user_overrides: std::collections::HashSet<&str> = knobs
        .keys()
        .filter_map(|name| {
            // Strip prefixes for macro_params bound to specific noise
            // nodes — we just need the inner-node param name.
            template
                .macro_params
                .iter()
                .find(|p| p.name == *name)
                .and_then(|p| p.binding.split_once(':').map(|(_, k)| k))
        })
        .collect();
    for n in nodes.iter_mut() {
        if !matches!(
            n.node_type,
            NodeType::PerlinNoise
                | NodeType::SimplexNoise
                | NodeType::WorleyNoise
                | NodeType::RidgedNoise
        ) {
            continue;
        }
        let ch = match n.params.get("character") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => continue,
        };
        let cd = bar_graph::character_defaults(&n.node_type, &ch);
        let writes: [(&str, ParamValue); 4] = [
            ("frequency", ParamValue::Float(cd.frequency)),
            ("octaves", ParamValue::UInt(cd.octaves)),
            ("lacunarity", ParamValue::Float(cd.lacunarity)),
            ("persistence", ParamValue::Float(cd.persistence)),
        ];
        for (k, v) in writes {
            if !user_overrides.contains(k) {
                n.params.insert(k.to_string(), v);
            }
        }
    }

    // Add the Bundler.
    nodes.push(RecipeNode {
        key: "_bundler".to_string(),
        node_type: NodeType::Bundler,
        label: "Bundler".to_string(),
        params: HashMap::new(),
    });

    // Translate the macro's inner connections, then add macro-output
    // → bundler.heightmap.
    let mut connections: Vec<RecipeConnection> = template
        .connections
        .iter()
        .map(|c| RecipeConnection {
            from: c.from.clone(),
            to: c.to.clone(),
        })
        .collect();
    let (out_node, out_port) = height_binding.split_once(':').ok_or_else(|| {
        anyhow!(
            "Macro output binding '{}' must be node:port",
            height_binding
        )
    })?;
    connections.push(RecipeConnection {
        from: format!("{out_node}.{out_port}"),
        to: "_bundler.heightmap".to_string(),
    });

    Ok(Recipe {
        schema_version: RECIPE_SCHEMA_VERSION,
        name: format!("preview-{}", template.name),
        shortname: None,
        description: String::new(),
        author: None,
        version: None,
        nodes,
        connections,
        output: OutputConfig {
            width,
            height,
            map_settings: MapSettings::default(),
        },
        features: Vec::new(),
    })
}

fn parse_param_value(kind: &str, val: &str) -> Result<ParamValue> {
    match kind {
        "Float" => Ok(ParamValue::Float(val.parse()?)),
        "UInt" => Ok(ParamValue::UInt(val.parse()?)),
        "Int" => Ok(ParamValue::Int(val.parse()?)),
        "Bool" => Ok(ParamValue::Bool(val.parse()?)),
        "String" => Ok(ParamValue::String(val.to_string())),
        other => bail!("Unknown param kind: {other}"),
    }
}

// Squash unused-warning for the import — used inside the function
// signature above but rust-analyzer's static check doesn't always
// see it through the `bar_graph::` re-export path.
#[allow(dead_code)]
fn _force_uses(_: PortValue) {}

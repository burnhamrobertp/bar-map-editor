use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};
use evalexpr::{
    build_operator_tree, ContextWithMutableVariables, DefaultNumericTypes, HashMapContext, Node,
    Value,
};
use rayon::prelude::*;

use crate::exec::ExecCtx;
use crate::exec::shared::get_optional_heightmap;

type Tree = Node<DefaultNumericTypes>;

/// Sample an optional input at (x, y), treating an absent input as 0.0. Out-of
/// bounds reads (a smaller upstream map) also fall back to 0.0.
fn sample(hm: &Option<Heightmap>, x: u32, y: u32) -> f64 {
    hm.as_ref().and_then(|h| h.get(x, y)).unwrap_or(0.0) as f64
}

fn eval_pixel(tree: &Tree, ctx: &mut HashMapContext<DefaultNumericTypes>, vars: [f64; 6]) -> f32 {
    let [a, b, c, d, nx, ny] = vars;
    // set_value only fails on a name/type collision, which can't happen here:
    // these keys are always plain float variables. Ignore the result.
    let _ = ctx.set_value("a".into(), Value::from_float(a));
    let _ = ctx.set_value("b".into(), Value::from_float(b));
    let _ = ctx.set_value("c".into(), Value::from_float(c));
    let _ = ctx.set_value("d".into(), Value::from_float(d));
    let _ = ctx.set_value("h".into(), Value::from_float(a));
    let _ = ctx.set_value("x".into(), Value::from_float(nx));
    let _ = ctx.set_value("y".into(), Value::from_float(ny));

    // A formula that references an undefined variable, divides by zero, etc.
    // fails per-pixel; fall back to 0.0 rather than aborting the whole node.
    tree.eval_float_with_context_mut(ctx).map(|v| v as f32).unwrap_or(0.0)
}

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let formula = match ctx.params.get("formula") {
        Some(bar_graph::ParamValue::String(s)) => s.as_str(),
        _ => "a",
    };

    let tree: Tree = build_operator_tree(formula)
        .map_err(|e| EvalError::Compute(format!("invalid formula: {e}")))?;

    let a = get_optional_heightmap(ctx.inputs, "a");
    let b = get_optional_heightmap(ctx.inputs, "b");
    let c = get_optional_heightmap(ctx.inputs, "c");
    let d = get_optional_heightmap(ctx.inputs, "d");

    let w = ctx.hm_w;
    let h = ctx.hm_h;
    let xden = (w.saturating_sub(1)).max(1) as f64;
    let yden = (h.saturating_sub(1)).max(1) as f64;

    let mut data = vec![0.0f32; (w as usize) * (h as usize)];
    data.par_chunks_mut(w as usize)
        .enumerate()
        .for_each(|(row, out)| {
            // One reused context per row -- only the variable values change
            // per pixel, so the map keeps its slots across the scanline.
            let mut cx = HashMapContext::<DefaultNumericTypes>::new();
            let y = row as u32;
            let ny = y as f64 / yden;
            for (x, px) in out.iter_mut().enumerate() {
                let xu = x as u32;
                let nx = xu as f64 / xden;
                let vars = [
                    sample(&a, xu, y),
                    sample(&b, xu, y),
                    sample(&c, xu, y),
                    sample(&d, xu, y),
                    nx,
                    ny,
                ];
                *px = eval_pixel(&tree, &mut cx, vars).clamp(0.0, 1.0);
            }
        });

    let hm = Heightmap::frbar_data(w, h, data).map_err(|e| EvalError::Compute(e.to_string()))?;

    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::ParamValue;

    fn run(formula: &str, inputs: &HashMap<String, PortValue>, w: u32, h: u32) -> Heightmap {
        let mut params = HashMap::new();
        params.insert("formula".to_string(), ParamValue::String(formula.to_string()));
        let ctx = ExecCtx { params: &params, inputs, hm_w: w, hm_h: h, tex_w: w, tex_h: h };
        match super::exec(&ctx).unwrap().remove("output").unwrap() {
            PortValue::Heightmap(hm) => hm,
            _ => panic!("expected heightmap output"),
        }
    }

    fn hm(w: u32, h: u32, data: Vec<f32>) -> Heightmap {
        Heightmap::frbar_data(w, h, data).unwrap()
    }

    #[test]
    fn formula_a_is_identity_of_input_a() {
        let data = vec![0.1, 0.4, 0.7, 0.9];
        let mut inputs = HashMap::new();
        inputs.insert("a".to_string(), PortValue::Heightmap(hm(2, 2, data.clone())));

        let out = run("a", &inputs, 2, 2);
        assert_eq!(out.width(), 2);
        assert_eq!(out.height(), 2);
        assert_eq!(out.get(0, 0).unwrap(), 0.1);
        assert_eq!(out.get(1, 0).unwrap(), 0.4);
        assert_eq!(out.get(0, 1).unwrap(), 0.7);
        assert_eq!(out.get(1, 1).unwrap(), 0.9);
    }

    #[test]
    fn averages_two_inputs() {
        let mut inputs = HashMap::new();
        inputs.insert("a".to_string(), PortValue::Heightmap(hm(2, 1, vec![0.2, 0.8])));
        inputs.insert("b".to_string(), PortValue::Heightmap(hm(2, 1, vec![0.4, 0.4])));

        let out = run("(a + b) / 2", &inputs, 2, 1);
        assert!((out.get(0, 0).unwrap() - 0.3).abs() < 1e-6);
        assert!((out.get(1, 0).unwrap() - 0.6).abs() < 1e-6);
    }

    #[test]
    fn x_yields_horizontal_gradient() {
        let inputs = HashMap::new();
        let out = run("x", &inputs, 5, 1);
        assert!(out.get(0, 0).unwrap() < 1e-6, "left edge ~0");
        assert!((out.get(4, 0).unwrap() - 1.0).abs() < 1e-6, "right edge ~1");
        // Monotonic increase left to right.
        assert!(out.get(1, 0).unwrap() < out.get(3, 0).unwrap());
    }

    #[test]
    fn unconnected_inputs_are_zero() {
        // No inputs wired: a..d all sample 0, so "a + b + c + d" is 0 everywhere.
        let inputs = HashMap::new();
        let out = run("a + b + c + d", &inputs, 3, 3);
        for &v in out.data() {
            assert_eq!(v, 0.0);
        }
    }

    #[test]
    fn result_is_clamped_to_unit_range() {
        let mut inputs = HashMap::new();
        inputs.insert("a".to_string(), PortValue::Heightmap(hm(2, 1, vec![0.5, 0.5])));
        let out = run("a * 10", &inputs, 2, 1);
        assert_eq!(out.get(0, 0).unwrap(), 1.0);
        let neg = run("a - 10", &inputs, 2, 1);
        assert_eq!(neg.get(0, 0).unwrap(), 0.0);
    }

    #[test]
    fn parse_error_yields_failure_no_panic() {
        let inputs = HashMap::new();
        let mut params = HashMap::new();
        params.insert("formula".to_string(), ParamValue::String("(a + b".to_string()));
        let ctx = ExecCtx { params: &params, inputs: &inputs, hm_w: 4, hm_h: 4, tex_w: 4, tex_h: 4 };
        let result = super::exec(&ctx);
        assert!(result.is_err(), "malformed formula must fail the node");
    }
}

use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    super::run_binop(ctx, |va, vb| (va + vb).min(1.0))
}

#[cfg(test)]
mod tests {
    use bar_data::Heightmap;
    use bar_graph::{NodeExecutor, NodeType, ParamValue};
    use std::collections::HashMap;

    use bar_graph::PortValue;

    fn const_hm(w: u32, h: u32, v: f32) -> Heightmap {
        Heightmap::frbar_data(w, h, vec![v; (w as usize) * (h as usize)]).unwrap()
    }

    #[test]
    fn add_node_honours_mask() {
        // Add with mask=0 should leave `a` untouched everywhere.
        let executor = crate::CpuExecutor;
        let a = const_hm(2, 2, 0.3);
        let b = const_hm(2, 2, 0.4);
        let mask = const_hm(2, 2, 0.0);
        let inputs = HashMap::from([
            ("a".to_string(), PortValue::Heightmap(a)),
            ("b".to_string(), PortValue::Heightmap(b)),
            ("mask".to_string(), PortValue::Mask(mask)),
        ]);
        let result = executor
            .execute(&NodeType::Add, &HashMap::<String, ParamValue>::new(), &inputs, 2, 2, 2, 2)
            .unwrap();
        let PortValue::Heightmap(hm) = result.get("output").unwrap() else {
            panic!("expected heightmap")
        };
        for &v in hm.data() {
            assert!((v - 0.3).abs() < 1e-6, "mask=0 should keep `a`, got {v}");
        }
    }
}

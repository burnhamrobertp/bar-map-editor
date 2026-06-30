use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::selectors::shared::apply_morphology;
use crate::exec::shared::{get_float, get_input_heightmap};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let radius = get_float(ctx.params, "radius", 4.0).max(0.5);
    let hm = apply_morphology(&input, radius, true);

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

#[cfg(test)]
mod tests {
    use bar_data::Heightmap;
    use bar_graph::{NodeExecutor, NodeType, ParamValue, PortValue};
    use std::collections::HashMap;

    #[test]
    fn mask_expand_dilates() {
        let executor = crate::CpuExecutor;
        // Single bright pixel in the centre of a dark field.
        let mut data = vec![0.0f32; 8 * 8];
        data[4 * 8 + 4] = 1.0;
        let hm = Heightmap::frbar_data(8, 8, data).unwrap();
        let params = HashMap::from([("radius".to_string(), ParamValue::Float(1.5))]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let result = executor
            .execute(&NodeType::MaskExpand, &params, &inputs, 8, 8, 8, 8)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // Centre + direct neighbours should be 1.
        assert!(out.get(4, 4).unwrap() > 0.99);
        assert!(
            out.get(3, 4).unwrap() > 0.99,
            "left neighbour should be expanded"
        );
        assert!(
            out.get(5, 4).unwrap() > 0.99,
            "right neighbour should be expanded"
        );
        // Far corner should still be 0.
        assert!(
            out.get(0, 0).unwrap() < 0.01,
            "corner should not be expanded"
        );
    }
}

use std::collections::HashMap;

use bar_compute::NoiseType;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    super::run_noise(NoiseType::Perlin, ctx)
}

#[cfg(test)]
mod tests {
    use bar_graph::{NodeExecutor, NodeType, ParamValue};
    use std::collections::HashMap;

    use bar_graph::PortValue;

    #[test]
    fn test_cpu_executor_noise() {
        let executor = crate::CpuExecutor;
        let params = HashMap::from([
            ("frequency".to_string(), ParamValue::Float(4.0)),
            ("octaves".to_string(), ParamValue::UInt(4)),
            ("seed".to_string(), ParamValue::UInt(42)),
        ]);
        let inputs = HashMap::new();
        let result = executor
            .execute(&NodeType::PerlinNoise, &params, &inputs, 64, 64, 64, 64)
            .unwrap();

        let output = result.get("output").unwrap();
        match output {
            PortValue::Heightmap(hm) => {
                assert_eq!(hm.width(), 64);
                assert_eq!(hm.height(), 64);
                assert!(hm.data().iter().any(|&v| v > 0.1));
            }
            _ => panic!("Expected heightmap output"),
        }
    }
}

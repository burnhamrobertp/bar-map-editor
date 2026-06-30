//! Executors for the channel split/merge nodes.

use std::collections::HashMap;

use bar_graph::NodeType;

use super::ExecFn;

pub mod channel_merge;
pub mod channel_split;

pub fn register(m: &mut HashMap<NodeType, ExecFn>) {
    m.insert(NodeType::ChannelSplit, channel_split::exec);
    m.insert(NodeType::ChannelMerge, channel_merge::exec);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use bar_data::ColorBuffer;
    use bar_graph::PortValue;

    use crate::exec::ExecCtx;

    fn ctx<'a>(
        params: &'a HashMap<String, bar_graph::ParamValue>,
        inputs: &'a HashMap<String, PortValue>,
    ) -> ExecCtx<'a> {
        ExecCtx {
            params,
            inputs,
            hm_w: 4,
            hm_h: 4,
            tex_w: 4,
            tex_h: 4,
        }
    }

    #[test]
    fn split_yields_per_channel_heightmaps_and_merge_inverts() {
        let mut buf = ColorBuffer::new(2, 2).unwrap();
        buf.set(0, 0, [0.1, 0.2, 0.3, 0.4]);
        buf.set(1, 0, [0.5, 0.6, 0.7, 0.8]);
        buf.set(0, 1, [0.9, 0.8, 0.7, 0.6]);
        buf.set(1, 1, [0.4, 0.3, 0.2, 0.1]);

        let params = HashMap::new();
        let mut inputs = HashMap::new();
        inputs.insert("color".to_string(), PortValue::Color(buf.clone()));

        let split = super::channel_split::exec(&ctx(&params, &inputs)).unwrap();

        let r = match &split["r"] {
            PortValue::Heightmap(h) => h.clone(),
            _ => panic!("r is not a heightmap"),
        };
        assert_eq!(r.get(0, 0).unwrap(), 0.1);
        assert_eq!(r.get(1, 1).unwrap(), 0.4);

        let mut merge_inputs = HashMap::new();
        for ch in ["r", "g", "b", "a"] {
            merge_inputs.insert(ch.to_string(), split[ch].clone());
        }
        let merged = super::channel_merge::exec(&ctx(&params, &merge_inputs)).unwrap();
        let out = match &merged["color"] {
            PortValue::Color(c) => c,
            _ => panic!("color output missing"),
        };

        for (orig, got) in buf.data().iter().zip(out.data()) {
            assert!((orig - got).abs() < 1e-6, "{orig} != {got}");
        }
    }

    #[test]
    fn merge_without_alpha_is_opaque() {
        let r = bar_data::Heightmap::frbar_data(2, 1, vec![0.2, 0.4]).unwrap();
        let g = bar_data::Heightmap::frbar_data(2, 1, vec![0.0, 0.0]).unwrap();
        let b = bar_data::Heightmap::frbar_data(2, 1, vec![0.0, 0.0]).unwrap();

        let params = HashMap::new();
        let mut inputs = HashMap::new();
        inputs.insert("r".to_string(), PortValue::Heightmap(r));
        inputs.insert("g".to_string(), PortValue::Heightmap(g));
        inputs.insert("b".to_string(), PortValue::Heightmap(b));

        let merged = super::channel_merge::exec(&ctx(&params, &inputs)).unwrap();
        let out = match &merged["color"] {
            PortValue::Color(c) => c,
            _ => panic!("color output missing"),
        };

        assert_eq!(out.get(0, 0).unwrap()[3], 1.0);
        assert_eq!(out.get(1, 0).unwrap()[3], 1.0);
    }
}

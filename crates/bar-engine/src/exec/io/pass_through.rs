use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::shared::get_string;
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let files_str = get_string(ctx.params, "files", "");
    let file_list: Vec<bar_graph::FileRef> = files_str
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '|');
            let path = parts.next()?.trim().to_string();
            // Bundle paths must use forward slashes; self-heal any
            // legacy backslashed entries from older saved projects
            // so the bundler validator doesn't reject them.
            let bundle_path = parts.next()?.trim().replace('\\', "/");
            if path.is_empty() {
                None
            } else {
                Some(bar_graph::FileRef { path, bundle_path })
            }
        })
        .collect();

    Ok(HashMap::from([(
        "files".to_string(),
        PortValue::FileList(file_list),
    )]))
}

#[cfg(test)]
mod tests {
    use bar_graph::{NodeExecutor, NodeType, ParamValue, PortValue};
    use std::collections::HashMap;

    #[test]
    fn test_passthrough_normalises_backslash_bundle_paths() {
        // Regression: the GUI's SD7 import path used to write file lists with
        // native (Windows) separators, which the bundler validator rejects.
        // The executor now normalises bundle paths to forward slashes.
        let executor = crate::CpuExecutor;
        let params = HashMap::from([(
            "files".to_string(),
            ParamValue::String(
                "C:\\src\\unittextures\\rock.dds|unittextures\\rock.dds\n\
                 C:\\src\\maps\\info.lua|maps\\info.lua"
                    .to_string(),
            ),
        )]);
        let inputs = HashMap::new();
        let result = executor
            .execute(&NodeType::PassThrough, &params, &inputs, 1, 1, 1, 1)
            .unwrap();
        let PortValue::FileList(list) = result.get("files").unwrap() else {
            panic!("Expected FileList output");
        };
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].bundle_path, "unittextures/rock.dds");
        assert_eq!(list[1].bundle_path, "maps/info.lua");
    }
}

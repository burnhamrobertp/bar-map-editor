use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{get_string, get_uint};
use crate::exec::paint::shared::{read_painted_heightmap_asset, GrayscaleSampling};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    // `resolution` is a legacy single-dim fallback used when
    // the asset file is missing; if width / height params are
    // present (new rectangular recipes) they take precedence.
    let res_fallback = get_uint(ctx.params, "resolution", 256).max(1);
    let fallback_w = get_uint(ctx.params, "width", res_fallback).max(1);
    let fallback_h = get_uint(ctx.params, "height", res_fallback).max(1);
    let asset_path = get_string(ctx.params, "asset_path", "");
    let sampling = match get_string(ctx.params, "sampling", "smooth") {
        "nearest" => GrayscaleSampling::Nearest,
        _ => GrayscaleSampling::Bilinear,
    };
    let hm = read_painted_heightmap_asset(
        asset_path, fallback_w, fallback_h, ctx.hm_w, ctx.hm_h, sampling,
    );

    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

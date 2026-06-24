use std::collections::HashMap;

use bar_compute::{bake_lightmap_cpu, LightmapParams};
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{get_float, get_input_heightmap, get_uint};
use crate::exec::texture::shared::resize_color_to_tex;

/// Unit vector toward the sun from azimuth (compass degrees) + elevation
/// (degrees above the horizon). +Z is up; azimuth 0 = +Y, increasing clockwise.
fn sun_dir_from_angles(azimuth_deg: f32, elevation_deg: f32) -> [f32; 3] {
    let az = azimuth_deg.to_radians();
    let el = elevation_deg.to_radians();
    let horiz = el.cos();

    [horiz * az.sin(), horiz * az.cos(), el.sin()]
}

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "heightmap")?;

    let sun_dir = sun_dir_from_angles(
        get_float(ctx.params, "sun_azimuth", 315.0),
        get_float(ctx.params, "sun_elevation", 45.0),
    );

    let params = LightmapParams {
        width: input.width(),
        height: input.height(),
        ao_strength: get_float(ctx.params, "ao_strength", 1.0),
        ao_radius: get_float(ctx.params, "ao_radius", 0.1),
        num_directions: get_uint(ctx.params, "num_directions", 16),
        max_steps: get_uint(ctx.params, "max_steps", 24),
        sun_dir,
        sun_softness: get_float(ctx.params, "sun_softness", 0.2),
    };

    let lightmap = bake_lightmap_cpu(&input, &params);
    let lightmap = resize_color_to_tex(lightmap, ctx.tex_w, ctx.tex_h);

    Ok(HashMap::from([(
        "lightmap".to_string(),
        PortValue::Color(lightmap),
    )]))
}

#[cfg(test)]
mod tests {
    use super::sun_dir_from_angles;

    #[test]
    fn sun_dir_is_unit_length() {
        for (az, el) in [(0.0, 0.0), (315.0, 45.0), (90.0, 30.0), (180.0, 90.0)] {
            let d = sun_dir_from_angles(az, el);
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-5, "az={az} el={el} len={len}");
        }
    }

    #[test]
    fn elevation_90_points_straight_up() {
        let d = sun_dir_from_angles(123.0, 90.0);
        assert!(d[2] > 0.999, "z should be ~1 at zenith: {}", d[2]);
        assert!(d[0].abs() < 1e-4 && d[1].abs() < 1e-4, "horizontal ~0 at zenith");
    }

    #[test]
    fn azimuth_zero_faces_positive_y() {
        let d = sun_dir_from_angles(0.0, 0.0);
        assert!(d[1] > 0.999, "az=0 should point +Y: {:?}", d);
    }
}

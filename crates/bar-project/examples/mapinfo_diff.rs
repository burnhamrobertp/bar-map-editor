use bar_project::{apply_mapinfo_overrides, MapSettings};

fn main() {
    let src = std::fs::read_to_string(r"C:\Users\Robert\AppData\Local\BarEditor\BarEditor\cache\work\onyx_cauldron_2.2.2_331385913f532381\mapinfo.lua").unwrap();
    let dst = std::fs::read_to_string(r"C:\Users\Robert\AppData\Local\Programs\Beyond-All-Reason\data\maps\onyx_cauldron.sdd\mapinfo.lua").unwrap();

    let mut a = MapSettings::default();
    apply_mapinfo_overrides(&src, &mut a);
    let mut b = MapSettings::default();
    apply_mapinfo_overrides(&dst, &mut b);

    println!("Source -> BME bundle (None/Some) per modelled field:\n");

    macro_rules! cmp {
        ($field:expr, $src:expr, $dst:expr) => {
            if $src != $dst {
                println!("  DIFFER  {:32} src={:?}  bme={:?}", $field, $src, $dst);
            } else if $src.is_some() {
                println!("  match   {:32} = {:?}", $field, $src);
            } else {
                println!("  none    {:32}", $field);
            }
        };
    }

    cmp!("min_height", a.min_height, b.min_height);
    cmp!("max_height", a.max_height, b.max_height);
    cmp!("map_hardness", a.map_hardness, b.map_hardness);
    cmp!("gravity", a.gravity, b.gravity);
    cmp!("tidal_strength", a.tidal_strength, b.tidal_strength);
    cmp!("max_metal", a.max_metal, b.max_metal);
    cmp!("extractor_radius", a.extractor_radius, b.extractor_radius);
    cmp!("void_water", a.void_water, b.void_water);
    cmp!("void_ground", a.void_ground, b.void_ground);
    cmp!("deformable", a.deformable, b.deformable);

    println!("\n-- atmosphere --");
    cmp!("min_wind", a.atmosphere.min_wind, b.atmosphere.min_wind);
    cmp!("max_wind", a.atmosphere.max_wind, b.atmosphere.max_wind);
    cmp!("fog_start", a.atmosphere.fog_start, b.atmosphere.fog_start);
    cmp!("fog_end", a.atmosphere.fog_end, b.atmosphere.fog_end);
    cmp!("fog_color", a.atmosphere.fog_color, b.atmosphere.fog_color);
    cmp!("sun_color", a.atmosphere.sun_color, b.atmosphere.sun_color);
    cmp!("sky_color", a.atmosphere.sky_color, b.atmosphere.sky_color);
    cmp!("sky_dir", a.atmosphere.sky_dir, b.atmosphere.sky_dir);
    cmp!(
        "cloud_density",
        a.atmosphere.cloud_density,
        b.atmosphere.cloud_density
    );
    cmp!(
        "cloud_color",
        a.atmosphere.cloud_color,
        b.atmosphere.cloud_color
    );
    cmp!("skybox", a.atmosphere.skybox, b.atmosphere.skybox);

    println!("\n-- lighting --");
    cmp!("sun_dir", a.lighting.sun_dir, b.lighting.sun_dir);
    cmp!(
        "sun_intensity",
        a.lighting.sun_intensity,
        b.lighting.sun_intensity
    );
    cmp!(
        "ground_ambient",
        a.lighting.ground_ambient,
        b.lighting.ground_ambient
    );
    cmp!(
        "ground_diffuse",
        a.lighting.ground_diffuse,
        b.lighting.ground_diffuse
    );
    cmp!(
        "ground_specular",
        a.lighting.ground_specular,
        b.lighting.ground_specular
    );
    cmp!(
        "spec_exponent",
        a.lighting.spec_exponent,
        b.lighting.spec_exponent
    );
    cmp!(
        "ground_shadow_density",
        a.lighting.ground_shadow_density,
        b.lighting.ground_shadow_density
    );

    println!("\n-- water --");
    cmp!("damage", a.water.damage, b.water.damage);
    cmp!("absorb", a.water.absorb, b.water.absorb);
    cmp!("base_color", a.water.base_color, b.water.base_color);
    cmp!("min_color", a.water.min_color, b.water.min_color);
    cmp!(
        "surface_color",
        a.water.surface_color,
        b.water.surface_color
    );
    cmp!(
        "surface_alpha",
        a.water.surface_alpha,
        b.water.surface_alpha
    );
    cmp!(
        "specular_color",
        a.water.specular_color,
        b.water.specular_color
    );
    cmp!(
        "ambient_factor",
        a.water.ambient_factor,
        b.water.ambient_factor
    );
    cmp!(
        "diffuse_factor",
        a.water.diffuse_factor,
        b.water.diffuse_factor
    );
    cmp!(
        "specular_factor",
        a.water.specular_factor,
        b.water.specular_factor
    );
    cmp!(
        "specular_power",
        a.water.specular_power,
        b.water.specular_power
    );
    cmp!("fresnel_min", a.water.fresnel_min, b.water.fresnel_min);
    cmp!("fresnel_max", a.water.fresnel_max, b.water.fresnel_max);
    cmp!(
        "fresnel_power",
        a.water.fresnel_power,
        b.water.fresnel_power
    );
    cmp!(
        "reflection_distortion",
        a.water.reflection_distortion,
        b.water.reflection_distortion
    );
    cmp!(
        "perlin_amplitude",
        a.water.perlin_amplitude,
        b.water.perlin_amplitude
    );
    cmp!("blur_base", a.water.blur_base, b.water.blur_base);
    cmp!(
        "blur_exponent",
        a.water.blur_exponent,
        b.water.blur_exponent
    );
    cmp!(
        "caustics_resolution",
        a.water.caustics_resolution,
        b.water.caustics_resolution
    );
    cmp!(
        "caustics_strength",
        a.water.caustics_strength,
        b.water.caustics_strength
    );
    cmp!(
        "wave_offset_factor",
        a.water.wave_offset_factor,
        b.water.wave_offset_factor
    );
    cmp!(
        "wave_foam_distortion",
        a.water.wave_foam_distortion,
        b.water.wave_foam_distortion
    );
    cmp!(
        "wave_foam_intensity",
        a.water.wave_foam_intensity,
        b.water.wave_foam_intensity
    );
    cmp!("wave_length", a.water.wave_length, b.water.wave_length);

    println!("\n-- custom_grass --");
    cmp!("dist_tga", a.custom_grass.dist_tga, b.custom_grass.dist_tga);
    cmp!(
        "blade_color_tex",
        a.custom_grass.blade_color_tex,
        b.custom_grass.blade_color_tex
    );
    cmp!("max_size", a.custom_grass.max_size, b.custom_grass.max_size);
    cmp!("min_size", a.custom_grass.min_size, b.custom_grass.min_size);
    cmp!(
        "patch_resolution",
        a.custom_grass.patch_resolution,
        b.custom_grass.patch_resolution
    );
    cmp!(
        "patch_placement_jitter",
        a.custom_grass.patch_placement_jitter,
        b.custom_grass.patch_placement_jitter
    );
    cmp!(
        "map_color_factor",
        a.custom_grass.map_color_factor,
        b.custom_grass.map_color_factor
    );
    cmp!(
        "map_color_base",
        a.custom_grass.map_color_base,
        b.custom_grass.map_color_base
    );
    cmp!(
        "alpha_threshold",
        a.custom_grass.alpha_threshold,
        b.custom_grass.alpha_threshold
    );
    cmp!(
        "shadow_factor",
        a.custom_grass.shadow_factor,
        b.custom_grass.shadow_factor
    );
    cmp!(
        "grass_brightness",
        a.custom_grass.grass_brightness,
        b.custom_grass.grass_brightness
    );
    cmp!(
        "fade_start",
        a.custom_grass.fade_start,
        b.custom_grass.fade_start
    );
    cmp!("fade_end", a.custom_grass.fade_end, b.custom_grass.fade_end);
    cmp!(
        "wind_strength",
        a.custom_grass.wind_strength,
        b.custom_grass.wind_strength
    );
    cmp!(
        "wind_scale",
        a.custom_grass.wind_scale,
        b.custom_grass.wind_scale
    );
    cmp!(
        "wind_sample_scale",
        a.custom_grass.wind_sample_scale,
        b.custom_grass.wind_sample_scale
    );
    cmp!(
        "grass_wind_mult",
        a.custom_grass.grass_wind_mult,
        b.custom_grass.grass_wind_mult
    );
}

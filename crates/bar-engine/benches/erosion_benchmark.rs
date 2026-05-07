//! Benchmarks comparing CPU vs GPU erosion at various resolutions.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

use bar_compute::{
    hydraulic_erosion, thermal_erosion, FlowErosionParams, GpuContext, GpuErosionPipeline,
    HydraulicErosionParams, ThermalErosionParams, NoiseParams, NoiseType, generate_noise_cpu,
};

const RESOLUTIONS: &[u32] = &[256, 512, 1024];

/// Build a simple heightmap filled with FBM noise at the given size.
fn make_heightmap(size: u32) -> bar_data::Heightmap {
    let params = NoiseParams {
        width: size,
        height: size,
        noise_type: NoiseType::Perlin,
        frequency: 2.0,
        octaves: 4,
        persistence: 0.5,
        lacunarity: 2.0,
        seed: 1,
        offset_x: 0.0,
        offset_y: 0.0,
    };
    generate_noise_cpu(&params).unwrap()
}

fn bench_cpu_hydraulic(c: &mut Criterion) {
    let mut group = c.benchmark_group("cpu_hydraulic_erosion");
    group.sample_size(10);

    for &size in RESOLUTIONS {
        let hm = make_heightmap(size);
        let params = HydraulicErosionParams {
            num_droplets: 5_000,
            ..Default::default()
        };

        group.bench_with_input(BenchmarkId::new("droplets_5k", size), &size, |b, _| {
            b.iter(|| hydraulic_erosion(&hm, &params));
        });
    }
    group.finish();
}

fn bench_cpu_thermal(c: &mut Criterion) {
    let mut group = c.benchmark_group("cpu_thermal_erosion");
    group.sample_size(10);

    for &size in RESOLUTIONS {
        let hm = make_heightmap(size);
        let params = ThermalErosionParams {
            iterations: 50,
            talus_angle: 0.004,
            erosion_rate: 0.5,
        };

        group.bench_with_input(BenchmarkId::new("iters_50", size), &size, |b, _| {
            b.iter(|| thermal_erosion(&hm, &params));
        });
    }
    group.finish();
}

fn bench_gpu_hydraulic(c: &mut Criterion) {
    let gpu_context = match pollster::block_on(async { GpuContext::new_standalone().await }) {
        Ok(ctx) => ctx,
        Err(_) => {
            eprintln!("No GPU available, skipping GPU erosion benchmarks");
            return;
        }
    };

    let pipeline = GpuErosionPipeline::new(&gpu_context.device);

    let mut group = c.benchmark_group("gpu_hydraulic_erosion");
    group.sample_size(10);

    for &size in RESOLUTIONS {
        let hm = make_heightmap(size);
        let params = FlowErosionParams {
            iterations: 20,
            rain_rate: 0.012,
            evaporation_rate: 0.015,
            sediment_capacity: 1.0,
            erosion_rate: 0.3,
            deposition_rate: 0.3,
            min_tilt: 0.01,
            gravity: 9.8,
            dt: 0.02,
            pipe_length: 1.0,
        };

        group.bench_with_input(BenchmarkId::new("flow_iters_20", size), &size, |b, _| {
            b.iter(|| pipeline.hydraulic_flow_erode(&gpu_context, &hm, &params).unwrap());
        });
    }
    group.finish();
}

fn bench_gpu_thermal(c: &mut Criterion) {
    let gpu_context = match pollster::block_on(async { GpuContext::new_standalone().await }) {
        Ok(ctx) => ctx,
        Err(_) => {
            eprintln!("No GPU available, skipping GPU thermal benchmarks");
            return;
        }
    };

    let pipeline = GpuErosionPipeline::new(&gpu_context.device);

    let mut group = c.benchmark_group("gpu_thermal_erosion");
    group.sample_size(10);

    for &size in RESOLUTIONS {
        let hm = make_heightmap(size);
        let params = ThermalErosionParams {
            iterations: 50,
            talus_angle: 0.004,
            erosion_rate: 0.5,
        };

        group.bench_with_input(BenchmarkId::new("iters_50", size), &size, |b, _| {
            b.iter(|| pipeline.thermal_erode(&gpu_context, &hm, &params).unwrap());
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_cpu_hydraulic,
    bench_cpu_thermal,
    bench_gpu_hydraulic,
    bench_gpu_thermal
);
criterion_main!(benches);

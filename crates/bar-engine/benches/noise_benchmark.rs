//! Benchmarks comparing CPU vs GPU noise generation at various resolutions.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::collections::HashMap;

use bar_compute::{generate_noise_cpu, GpuContext, GpuNoisePipeline, NoiseParams, NoiseType};
use bar_engine::CpuExecutor;
use bar_graph::{NodeExecutor, NodeType, ParamValue};

const RESOLUTIONS: &[u32] = &[128, 256, 512, 1024, 2048];

fn noise_params(size: u32) -> NoiseParams {
    NoiseParams {
        width: size,
        height: size,
        noise_type: NoiseType::Perlin,
        frequency: 2.0,
        octaves: 6,
        persistence: 0.5,
        lacunarity: 2.0,
        seed: 42,
        offset_x: 0.0,
        offset_y: 0.0,
    }
}

fn executor_params() -> HashMap<String, ParamValue> {
    let mut params = HashMap::new();
    params.insert("frequency".to_string(), ParamValue::Float(2.0));
    params.insert("octaves".to_string(), ParamValue::Float(6.0));
    params.insert("persistence".to_string(), ParamValue::Float(0.5));
    params.insert("lacunarity".to_string(), ParamValue::Float(2.0));
    params.insert("seed".to_string(), ParamValue::Float(42.0));
    params
}

fn bench_cpu_noise(c: &mut Criterion) {
    let mut group = c.benchmark_group("cpu_noise");
    group.sample_size(10);

    for &size in RESOLUTIONS {
        group.bench_with_input(BenchmarkId::new("perlin", size), &size, |b, &size| {
            let params = noise_params(size);
            b.iter(|| generate_noise_cpu(&params));
        });
    }
    group.finish();
}

fn bench_cpu_executor_noise(c: &mut Criterion) {
    let mut group = c.benchmark_group("cpu_executor_noise");
    group.sample_size(10);
    let executor = CpuExecutor;
    let params = executor_params();
    let inputs = HashMap::new();

    for &size in RESOLUTIONS {
        group.bench_with_input(BenchmarkId::new("perlin", size), &size, |b, &size| {
            b.iter(|| {
                executor
                    .execute(&NodeType::PerlinNoise, &params, &inputs, size, size)
                    .unwrap()
            });
        });
    }
    group.finish();
}

fn bench_gpu_noise(c: &mut Criterion) {
    // Try to create a standalone GPU context
    let gpu_context = match pollster::block_on(async {
        GpuContext::new_standalone().await
    }) {
        Ok(ctx) => ctx,
        Err(_) => {
            eprintln!("No GPU available, skipping GPU benchmarks");
            return;
        }
    };

    let pipeline = GpuNoisePipeline::new(&gpu_context.device);

    let mut group = c.benchmark_group("gpu_noise");
    group.sample_size(10);

    for &size in RESOLUTIONS {
        group.bench_with_input(BenchmarkId::new("perlin", size), &size, |b, &size| {
            let params = noise_params(size);
            b.iter(|| pipeline.generate(&gpu_context, &params, NoiseType::Perlin).unwrap());
        });
    }
    group.finish();
}

criterion_group!(benches, bench_cpu_noise, bench_cpu_executor_noise, bench_gpu_noise);
criterion_main!(benches);

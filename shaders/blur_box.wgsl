// Box Blur Compute Shader (single direction)
// Dispatched in alternating horizontal/vertical passes.
// 3 full passes (H+V each) approximates a Gaussian blur.

struct BlurParams {
    width: u32,
    height: u32,
    radius: u32,
    horizontal: u32,  // 1 = horizontal pass, 0 = vertical pass
}

@group(0) @binding(0) var<uniform> params: BlurParams;
@group(0) @binding(1) var<storage, read> input_data: array<f32>;
@group(0) @binding(2) var<storage, read_write> output_data: array<f32>;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = gid.y;

    if x >= params.width || y >= params.height {
        return;
    }

    let idx = y * params.width + x;
    var sum: f32 = 0.0;
    var count: f32 = 0.0;
    let r = i32(params.radius);

    if params.horizontal == 1u {
        // Horizontal box blur
        let x_start = max(i32(x) - r, 0);
        let x_end = min(i32(x) + r + 1, i32(params.width));
        for (var xx = x_start; xx < x_end; xx = xx + 1) {
            sum = sum + input_data[y * params.width + u32(xx)];
            count = count + 1.0;
        }
    } else {
        // Vertical box blur
        let y_start = max(i32(y) - r, 0);
        let y_end = min(i32(y) + r + 1, i32(params.height));
        for (var yy = y_start; yy < y_end; yy = yy + 1) {
            sum = sum + input_data[u32(yy) * params.width + x];
            count = count + 1.0;
        }
    }

    output_data[idx] = sum / count;
}

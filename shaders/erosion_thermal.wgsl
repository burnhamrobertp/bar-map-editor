// Thermal Erosion (Weathering) Compute Shader
//
// Simulates material slumping: if the height difference between neighbours
// exceeds the talus angle threshold, material is transferred downhill.
//
// Fixed: each cell computes both its LOSS to lower neighbours AND its GAIN
// from higher neighbours in one pass, preserving mass.  Each thread writes
// only to its own output index, so there are no data races.
//
// Cost: O(64) heightmap reads per cell (8 neighbours × 8 inner neighbours)
// vs the previous O(8). Fully correct and deterministic.

struct ThermalParams {
    width:       u32,
    height:      u32,
    talus_angle: f32,
    erosion_rate: f32,
}

@group(0) @binding(0) var<uniform>             params:        ThermalParams;
@group(0) @binding(1) var<storage, read_write> heightmap_in:  array<f32>;
@group(0) @binding(2) var<storage, read_write> heightmap_out: array<f32>;

fn sample_in(x: i32, y: i32) -> f32 {
    let cx = clamp(x, 0, i32(params.width)  - 1);
    let cy = clamp(y, 0, i32(params.height) - 1);
    return heightmap_in[u32(cy) * params.width + u32(cx)];
}

// Neighbour offset table indexed 0..7:
//   0=(-1,-1)  1=(0,-1)  2=(1,-1)
//   3=(-1, 0)            4=(1, 0)
//   5=(-1, 1)  6=(0, 1)  7=(1, 1)
fn nbr_dx(k: i32) -> i32 {
    switch k {
        case 0, 3, 5: { return -1; }
        case 1, 6:    { return  0; }
        default:      { return  1; }
    }
}
fn nbr_dy(k: i32) -> i32 {
    switch k {
        case 0, 1, 2: { return -1; }
        case 3, 4:    { return  0; }
        default:      { return  1; }
    }
}
fn nbr_dist(k: i32) -> f32 {
    switch k {
        case 0, 2, 5, 7: { return 1.41421356; }
        default:         { return 1.0; }
    }
}

// Compute (max_slope_excess, total_slope_excess) for cell (cx,cy) over all
// its lower neighbours that exceed the talus angle.
fn cell_loss_stats(cx: i32, cy: i32) -> vec2<f32> {
    let h_c       = sample_in(cx, cy);
    var max_diff  = 0.0;
    var total_exc = 0.0;
    for (var k: i32 = 0; k < 8; k++) {
        let nx = cx + nbr_dx(k);
        let ny = cy + nbr_dy(k);
        if nx < 0 || nx >= i32(params.width) || ny < 0 || ny >= i32(params.height) { continue; }
        let diff = (h_c - sample_in(nx, ny)) / nbr_dist(k);
        if diff > params.talus_angle {
            total_exc += diff - params.talus_angle;
            max_diff   = max(max_diff, diff);
        }
    }
    return vec2<f32>(max_diff, total_exc);
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = gid.y;
    if x >= params.width || y >= params.height { return; }

    let idx      = y * params.width + x;
    let center_h = heightmap_in[idx];
    let ix       = i32(x);
    let iy       = i32(y);
    var delta    = 0.0;

    // ── LOSS: material flowing out to steeper lower neighbours ────────────────
    let stats = cell_loss_stats(ix, iy);
    if stats.y > 0.0 {
        delta -= (stats.x - params.talus_angle) * params.erosion_rate * 0.5;
    }

    // ── GAIN: material flowing in from steeper higher neighbours ──────────────
    // For each higher neighbour N, compute the proportion of N's outbound
    // transfer that is directed toward the current cell C.
    for (var k: i32 = 0; k < 8; k++) {
        let nx        = ix + nbr_dx(k);
        let ny        = iy + nbr_dy(k);
        let dist_to_n = nbr_dist(k);
        if nx < 0 || nx >= i32(params.width) || ny < 0 || ny >= i32(params.height) { continue; }

        let h_n         = sample_in(nx, ny);
        let diff_n_to_c = (h_n - center_h) / dist_to_n;
        if diff_n_to_c <= params.talus_angle { continue; }

        let n_stats = cell_loss_stats(nx, ny);
        if n_stats.y <= 0.0 { continue; }

        let n_transfer = (n_stats.x - params.talus_angle) * params.erosion_rate * 0.5;
        let proportion = (diff_n_to_c - params.talus_angle) / n_stats.y;
        delta         += n_transfer * proportion;
    }

    heightmap_out[idx] = center_h + delta;
}

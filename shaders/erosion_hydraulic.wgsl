// Hydraulic Erosion Compute Shader
// Particle-based erosion simulation — each invocation traces one water droplet.
//
// Algorithm:
// 1. Drop a particle at a random position
// 2. Compute gradient (steepest descent direction)
// 3. Move particle along gradient
// 4. Erode sediment from current cell (based on slope, speed, capacity)
// 5. Deposit sediment when capacity exceeded or speed too low
// 6. Repeat until lifetime expires or particle exits bounds

struct ErosionParams {
    width: u32,
    height: u32,
    num_droplets: u32,
    seed: u32,
    inertia: f32,
    capacity_factor: f32,
    min_capacity: f32,
    deposition_rate: f32,
    erosion_rate: f32,
    evaporation_rate: f32,
    gravity: f32,
    max_lifetime: u32,
    erosion_radius: u32,
    _padding: u32,
}

@group(0) @binding(0) var<uniform> params: ErosionParams;
@group(0) @binding(1) var<storage, read_write> heightmap: array<f32>;

// PCG hash for random number generation
fn pcg_hash(input: u32) -> u32 {
    var state = input * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn get_height(x: i32, y: i32) -> f32 {
    let cx = clamp(x, 0, i32(params.width) - 1);
    let cy = clamp(y, 0, i32(params.height) - 1);
    return heightmap[u32(cy) * params.width + u32(cx)];
}

// Bilinear interpolation of height
fn sample_height(px: f32, py: f32) -> f32 {
    let x0 = i32(floor(px));
    let y0 = i32(floor(py));
    let fx = px - f32(x0);
    let fy = py - f32(y0);

    let h00 = get_height(x0, y0);
    let h10 = get_height(x0 + 1, y0);
    let h01 = get_height(x0, y0 + 1);
    let h11 = get_height(x0 + 1, y0 + 1);

    let h0 = h00 * (1.0 - fx) + h10 * fx;
    let h1 = h01 * (1.0 - fx) + h11 * fx;
    return h0 * (1.0 - fy) + h1 * fy;
}

// Compute gradient at position (bilinear)
fn compute_gradient(px: f32, py: f32) -> vec2<f32> {
    let x0 = i32(floor(px));
    let y0 = i32(floor(py));
    let fx = px - f32(x0);
    let fy = py - f32(y0);

    let h00 = get_height(x0, y0);
    let h10 = get_height(x0 + 1, y0);
    let h01 = get_height(x0, y0 + 1);
    let h11 = get_height(x0 + 1, y0 + 1);

    let gx = (h10 - h00) * (1.0 - fy) + (h11 - h01) * fy;
    let gy = (h01 - h00) * (1.0 - fx) + (h11 - h10) * fx;
    return vec2<f32>(gx, gy);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let droplet_idx = gid.x;
    if droplet_idx >= params.num_droplets {
        return;
    }

    // Initialize random seed from droplet index and global seed
    var rng_state = pcg_hash(droplet_idx ^ params.seed);

    // Random starting position
    rng_state = pcg_hash(rng_state);
    let start_x = f32(rng_state) / 4294967295.0 * f32(params.width - 1u);
    rng_state = pcg_hash(rng_state);
    let start_y = f32(rng_state) / 4294967295.0 * f32(params.height - 1u);

    var pos = vec2<f32>(start_x, start_y);
    var dir = vec2<f32>(0.0, 0.0);
    var speed: f32 = 1.0;
    var water: f32 = 1.0;
    var sediment: f32 = 0.0;

    for (var lifetime = 0u; lifetime < params.max_lifetime; lifetime = lifetime + 1u) {
        let ix = i32(floor(pos.x));
        let iy = i32(floor(pos.y));

        // Check bounds
        if ix < 0 || ix >= i32(params.width) - 1 || iy < 0 || iy >= i32(params.height) - 1 {
            break;
        }

        // Compute gradient and height at current position
        let gradient = compute_gradient(pos.x, pos.y);
        let old_height = sample_height(pos.x, pos.y);

        // Update direction with inertia
        dir = dir * params.inertia - gradient * (1.0 - params.inertia);
        let dir_len = length(dir);
        if dir_len < 0.0001 {
            // Random direction if stuck in flat area
            rng_state = pcg_hash(rng_state);
            let angle = f32(rng_state) / 4294967295.0 * 6.283185;
            dir = vec2<f32>(cos(angle), sin(angle));
        } else {
            dir = dir / dir_len;
        }

        // Move particle
        let new_pos = pos + dir;

        // Check new position bounds
        if new_pos.x < 0.0 || new_pos.x >= f32(params.width) - 1.0 ||
           new_pos.y < 0.0 || new_pos.y >= f32(params.height) - 1.0 {
            break;
        }

        let new_height = sample_height(new_pos.x, new_pos.y);
        let height_diff = new_height - old_height;

        // Calculate sediment capacity
        let capacity = max(
            -height_diff * speed * water * params.capacity_factor,
            params.min_capacity
        );

        if sediment > capacity || height_diff > 0.0 {
            // Deposit sediment
            var deposit_amount: f32;
            if height_diff > 0.0 {
                deposit_amount = min(sediment, height_diff);
            } else {
                deposit_amount = (sediment - capacity) * params.deposition_rate;
            }
            sediment = sediment - deposit_amount;

            // Deposit at integer cell (atomic-free — races are acceptable)
            let cx = u32(ix);
            let cy = u32(iy);
            if cx < params.width && cy < params.height {
                let didx = cy * params.width + cx;
                heightmap[didx] = heightmap[didx] + deposit_amount;
            }
        } else {
            // Erode terrain (simplified: single-cell erosion for GPU safety)
            let erode_amount = min(
                (capacity - sediment) * params.erosion_rate,
                -height_diff
            );

            let cx = u32(ix);
            let cy = u32(iy);
            if cx < params.width && cy < params.height {
                let eidx = cy * params.width + cx;
                heightmap[eidx] = heightmap[eidx] - erode_amount;
            }

            sediment = sediment + erode_amount;
        }

        // Update speed and water
        speed = sqrt(max(speed * speed + height_diff * params.gravity, 0.0));
        water = water * (1.0 - params.evaporation_rate);

        pos = new_pos;

        // Stop if water evaporated
        if water < 0.001 {
            break;
        }
    }
}

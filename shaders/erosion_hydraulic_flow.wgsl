// Hydraulic Erosion — Virtual Pipe Model (Mei et al. 2007)
//
// Replaces the particle-based shader with a physically correct shallow-water
// flow simulation that has zero data races.
//
// Each iteration consists of 4 serial compute passes dispatched from a single
// Rust command encoder (wgpu guarantees sequential execution + barriers):
//
//   pass_flux       — update outflow flux from height differences
//   pass_water_vel  — update water depth and velocity field from flux
//   pass_erosion    — compute per-cell erosion/deposition delta
//   pass_apply      — apply terrain change + semi-Lagrangian sediment advect
//
// After pass_apply the Rust host copies sediment_out → sediment before the
// next iteration, implementing a ping-pong that eliminates the one remaining
// read/write ambiguity on the sediment buffer.
//
// Race analysis (all passes write only to own cell index unless noted):
//   pass_flux       reads terrain, water; writes flux[own]            — no race
//   pass_water_vel  reads flux (read-only after pass 1); writes water, velocity[own] — no race
//   pass_erosion    reads velocity, water, sediment; writes scratch[own] — no race
//   pass_apply      reads scratch, velocity, sediment; writes terrain, water, sediment_out[own] — no race

struct FlowParams {
    width:             u32,
    height:            u32,
    dt:                f32,
    pipe_length:       f32,
    gravity:           f32,
    rain_rate:         f32,
    evaporation_rate:  f32,
    sediment_capacity: f32,
    erosion_rate:      f32,
    deposition_rate:   f32,
    min_tilt:          f32,
    padding:           u32,
}

@group(0) @binding(0) var<uniform>             params:       FlowParams;
@group(0) @binding(1) var<storage, read_write> terrain:      array<f32>;
@group(0) @binding(2) var<storage, read_write> water:        array<f32>;
@group(0) @binding(3) var<storage, read_write> sediment:     array<f32>;
@group(0) @binding(4) var<storage, read_write> flux:         array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> velocity:     array<vec2<f32>>;
@group(0) @binding(6) var<storage, read_write> scratch:      array<f32>;
@group(0) @binding(7) var<storage, read_write> sediment_out: array<f32>;

fn cell_idx(x: u32, y: u32) -> u32 {
    return y * params.width + x;
}

// Read terrain+water surface height with closed-boundary clamping.
fn surface_height(x: i32, y: i32) -> f32 {
    let cx = u32(clamp(x, 0, i32(params.width)  - 1));
    let cy = u32(clamp(y, 0, i32(params.height) - 1));
    let i  = cy * params.width + cx;
    return terrain[i] + water[i];
}

// ─── Pass 1: update outflow flux ─────────────────────────────────────────────
@compute @workgroup_size(16, 16)
fn pass_flux(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = gid.y;
    if x >= params.width || y >= params.height { return; }

    let i  = cell_idx(x, y);
    let l  = params.pipe_length;
    let dt = params.dt;
    let A  = l * l;   // virtual pipe cross-sectional area

    let ix = i32(x);
    let iy = i32(y);
    let h0 = surface_height(ix, iy);

    // Height differences toward each neighbour (positive → we are higher)
    // flux layout: x=LEFT, y=RIGHT, z=UP(y-1), w=DOWN(y+1)
    let dh_L = h0 - surface_height(ix - 1, iy);
    let dh_R = h0 - surface_height(ix + 1, iy);
    let dh_U = h0 - surface_height(ix,     iy - 1);
    let dh_D = h0 - surface_height(ix,     iy + 1);

    let old  = flux[i];
    var f_L  = max(0.0, old.x + dt * A * params.gravity * dh_L / l);
    var f_R  = max(0.0, old.y + dt * A * params.gravity * dh_R / l);
    var f_U  = max(0.0, old.z + dt * A * params.gravity * dh_U / l);
    var f_D  = max(0.0, old.w + dt * A * params.gravity * dh_D / l);

    // Closed boundary: zero outflow past map edges
    if x == 0u               { f_L = 0.0; }
    if x == params.width - 1u  { f_R = 0.0; }
    if y == 0u               { f_U = 0.0; }
    if y == params.height - 1u { f_D = 0.0; }

    // Conservation: cap total outflow to available water volume
    let sum_f = f_L + f_R + f_U + f_D;
    if sum_f > 0.0 {
        let K = min(1.0, water[i] * A / (dt * sum_f));
        f_L *= K;  f_R *= K;  f_U *= K;  f_D *= K;
    }

    flux[i] = vec4<f32>(f_L, f_R, f_U, f_D);
}

// ─── Pass 2: update water depth and velocity ──────────────────────────────────
@compute @workgroup_size(16, 16)
fn pass_water_vel(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = gid.y;
    if x >= params.width || y >= params.height { return; }

    let i  = cell_idx(x, y);
    let l  = params.pipe_length;
    let dt = params.dt;

    // Outflow from this cell (written by pass_flux)
    let out     = flux[i];
    let out_sum = out.x + out.y + out.z + out.w;

    // Inflow from the four cardinal neighbours
    //   in_L = left  neighbour's rightward outflow  = flux[left ].y
    //   in_R = right neighbour's leftward  outflow  = flux[right].x
    //   in_U = top   neighbour's downward  outflow  = flux[top  ].w
    //   in_D = bot   neighbour's upward    outflow  = flux[bot  ].z
    var in_L = 0.0;
    var in_R = 0.0;
    var in_U = 0.0;
    var in_D = 0.0;
    if x > 0u                { in_L = flux[cell_idx(x - 1u, y     )].y; }
    if x < params.width  - 1u { in_R = flux[cell_idx(x + 1u, y     )].x; }
    if y > 0u                { in_U = flux[cell_idx(x,      y - 1u)].w; }
    if y < params.height - 1u { in_D = flux[cell_idx(x,      y + 1u)].z; }

    let in_sum = in_L + in_R + in_U + in_D;

    // Water depth update + uniform rainfall
    let old_w = water[i];
    let new_w = max(0.0, old_w + dt * (in_sum - out_sum) / (l * l))
                + params.rain_rate * dt;
    water[i]  = new_w;

    // Velocity: average net flux at each pair of cell boundaries.
    // Positive u = rightward (+x), positive v = downward (+y in image space)
    let avg_w  = max(0.001, 0.5 * (old_w + new_w));
    let denom  = 2.0 * avg_w * l;
    let u      = (in_L - out.x + out.y - in_R) / denom;
    let v      = (in_U - out.z + out.w - in_D) / denom;
    velocity[i] = vec2<f32>(u, v);
}

// ─── Pass 3: compute erosion/deposition delta ─────────────────────────────────
@compute @workgroup_size(16, 16)
fn pass_erosion(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = gid.y;
    if x >= params.width || y >= params.height { return; }

    let i     = cell_idx(x, y);
    let vel   = velocity[i];
    let speed = length(vel);
    let depth = water[i];

    // Sediment transport capacity: proportional to flow speed and water depth
    let capacity = params.sediment_capacity
                 * max(params.min_tilt, speed)
                 * depth;

    let s = sediment[i];
    var delta: f32;
    if s < capacity {
        // Erosion: remove material from terrain, suspend it in water
        delta = params.erosion_rate * (capacity - s);
        // Cap: cannot erode more than available terrain
        delta = min(delta, terrain[i]);
    } else {
        // Deposition: suspended sediment settles onto terrain
        delta = -params.deposition_rate * (s - capacity);
    }

    scratch[i] = delta;
}

// Bilinear sample from the sediment buffer (read-only within pass_apply)
fn sample_sediment(sx: f32, sy: f32) -> f32 {
    let w  = i32(params.width);
    let h  = i32(params.height);
    let x0 = i32(floor(sx));
    let y0 = i32(floor(sy));
    let fx = sx - f32(x0);
    let fy = sy - f32(y0);
    let cx0 = u32(clamp(x0,     0, w - 1));
    let cy0 = u32(clamp(y0,     0, h - 1));
    let cx1 = u32(clamp(x0 + 1, 0, w - 1));
    let cy1 = u32(clamp(y0 + 1, 0, h - 1));
    let s00 = sediment[cy0 * params.width + cx0];
    let s10 = sediment[cy0 * params.width + cx1];
    let s01 = sediment[cy1 * params.width + cx0];
    let s11 = sediment[cy1 * params.width + cx1];
    return mix(mix(s00, s10, fx), mix(s01, s11, fx), fy);
}

// ─── Pass 4: apply changes + advect sediment ──────────────────────────────────
@compute @workgroup_size(16, 16)
fn pass_apply(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = gid.y;
    if x >= params.width || y >= params.height { return; }

    let i     = cell_idx(x, y);
    let delta = scratch[i];

    // Positive delta = erosion (terrain loses), negative = deposition (terrain gains)
    terrain[i] = clamp(terrain[i] - delta, 0.0, 1.0);

    // Evaporation
    water[i] = max(0.0, water[i] * (1.0 - params.evaporation_rate * params.dt));

    // Semi-Lagrangian sediment advection: backtrack from current position along
    // the velocity field to find where this cell's water parcel originated.
    let vel      = velocity[i];
    let src_x    = f32(x) - vel.x * params.dt;
    let src_y    = f32(y) - vel.y * params.dt;
    let advected = sample_sediment(src_x, src_y);

    // Write to ping-pong output; Rust copies sediment_out → sediment after this pass.
    sediment_out[i] = max(0.0, advected + delta);
}

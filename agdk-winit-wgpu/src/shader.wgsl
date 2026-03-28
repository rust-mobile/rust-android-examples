struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

struct Uniforms {
    rotation: f32,
    position_x: f32,
    position_y: f32,
    _padding: f32,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) in_vertex_index: u32) -> VertexOutput {
    // Original triangle vertices
    let x = f32(i32(in_vertex_index) - 1);
    let y = f32(i32(in_vertex_index & 1u) * 2 - 1);
    let z = 0.0;

    // Apply Y-axis rotation
    let cos_r = cos(uniforms.rotation);
    let sin_r = sin(uniforms.rotation);
    let rotated_x = x * cos_r + z * sin_r;
    let rotated_z = -x * sin_r + z * cos_r;
    let rotated_y = y;

    // Scale down to fit in middle third (scale by 1/3)
    let scaled_x = rotated_x / 3.0;
    let scaled_y = rotated_y / 3.0;

    // Apply position offset
    let final_x = scaled_x + uniforms.position_x;
    let final_y = scaled_y + uniforms.position_y;

    var out: VertexOutput;
    out.position = vec4<f32>(final_x, final_y, 0.0, 1.0);

    // Assign colors: red, green, blue for the three vertices
    if (in_vertex_index == 0u) {
        out.color = vec4<f32>(1.0, 0.0, 0.0, 1.0); // Red
    } else if (in_vertex_index == 1u) {
        out.color = vec4<f32>(0.0, 1.0, 0.0, 1.0); // Green
    } else {
        out.color = vec4<f32>(0.0, 0.0, 1.0, 1.0); // Blue
    }

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}

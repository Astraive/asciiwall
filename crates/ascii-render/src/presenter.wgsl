// Engine-owned presenter shader, revision ascii-presenter-1. No package WGSL is accepted.
struct Cell {
    @align(16) glyph: u32,
    foreground: u32,
    background: u32,
    flags: u32,
}
struct Parameters {
    columns: u32,
    rows: u32,
    atlas_columns: u32,
    cell_width: u32,
    cell_height: u32,
    reserved0: u32,
    reserved1: u32,
    reserved2: u32,
}
@group(0) @binding(0) var<uniform> params: Parameters;
@group(0) @binding(1) var<storage, read> cells: array<Cell>;
@group(0) @binding(2) var masks: texture_2d<f32>;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) cell_index: u32,
    @location(1) local: vec2<f32>,
}

@vertex
fn vertex_main(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> VertexOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0)
    );
    let local = corners[vertex];
    let column = instance % params.columns;
    let row = instance / params.columns;
    let uv = (vec2<f32>(f32(column), f32(row)) + local) /
        vec2<f32>(f32(params.columns), f32(params.rows));
    var result: VertexOut;
    result.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    result.cell_index = instance;
    result.local = local;
    return result;
}

fn linear_channel(value: f32) -> f32 {
    if value <= 0.04045 { return value / 12.92; }
    return pow((value + 0.055) / 1.055, 2.4);
}

// Input words contain byte-order-independent RGBA components in bits 0..31.
fn premultiplied_color(word: u32, coverage: f32) -> vec4<f32> {
    let red = f32(word & 255u) / 255.0;
    let green = f32((word >> 8u) & 255u) / 255.0;
    let blue = f32((word >> 16u) & 255u) / 255.0;
    let alpha = f32((word >> 24u) & 255u) / 255.0 * coverage;
    return vec4<f32>(vec3<f32>(linear_channel(red), linear_channel(green), linear_channel(blue)) * alpha, alpha);
}

@fragment
fn background_main(input: VertexOut) -> @location(0) vec4<f32> {
    return premultiplied_color(cells[input.cell_index].background, 1.0);
}

@fragment
fn glyph_main(input: VertexOut) -> @location(0) vec4<f32> {
    let glyph = cells[input.cell_index].glyph;
    let glyph_origin = vec2<u32>((glyph % params.atlas_columns) * params.cell_width,
                                 (glyph / params.atlas_columns) * params.cell_height);
    let extent = vec2<u32>(params.cell_width, params.cell_height);
    let offset = min(vec2<u32>(max(input.local, vec2<f32>(0.0)) * vec2<f32>(extent)), extent - vec2<u32>(1u));
    let coverage = textureLoad(masks, vec2<i32>(glyph_origin + offset), 0).r;
    return premultiplied_color(cells[input.cell_index].foreground, coverage);
}

// Layer compositing: one quad per layer, premultiplied-alpha "over" blending.
// Canvas space: pixels, origin at the canvas centre, +y down.

struct Globals {
    resolution: vec2<f32>,
    _pad: vec2<f32>,
};

struct LayerData {
    axes: vec4<f32>,      // world x_axis.xy, y_axis.xy
    origin_size: vec4<f32>, // world translation.xy, content size.xy
    color: vec4<f32>,     // linear RGBA, straight alpha (rectangles)
    params: vec4<f32>,    // corner radius, opacity, kind (0 rect, 1 image, 2 missing), AA margin
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var<uniform> layer: LayerData;
@group(1) @binding(0) var image_tex: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, -0.5), vec2<f32>(0.5, 0.5),
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, 0.5), vec2<f32>(-0.5, 0.5),
    );
    let size = layer.origin_size.zw;
    let margin = layer.params.w;
    let local = corners[vi] * (size + vec2<f32>(2.0 * margin));
    let world = layer.axes.xy * local.x + layer.axes.zw * local.y + layer.origin_size.xy;
    let half_res = globals.resolution * 0.5;

    var out: VsOut;
    out.pos = vec4<f32>(world.x / half_res.x, -world.y / half_res.y, 0.0, 1.0);
    out.local = local;
    return out;
}

fn sd_round_box(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let size = layer.origin_size.zw;
    let half_size = size * 0.5;
    let radius = min(layer.params.x, min(half_size.x, half_size.y));

    // Signed distance in content units; fwidth converts to screen pixels.
    let d = sd_round_box(in.local, half_size, radius);
    let aa = max(fwidth(d), 1e-4);
    let coverage = clamp(0.5 - d / aa, 0.0, 1.0);

    // Sample unconditionally (uniform control flow); rectangles bind 1x1 white.
    let uv = clamp(in.local / size + vec2<f32>(0.5), vec2<f32>(0.0), vec2<f32>(1.0));
    let sampled = textureSample(image_tex, image_sampler, uv);

    let kind = layer.params.z;
    var color = layer.color;
    if (kind > 0.5 && kind < 1.5) {
        color = sampled;
    } else if (kind >= 1.5) {
        // Missing image: magenta checkerboard.
        let cell = floor(in.local / 24.0);
        let odd = ((i32(cell.x) + i32(cell.y)) & 1) == 1;
        color = select(vec4<f32>(0.8, 0.0, 0.8, 1.0), vec4<f32>(0.08, 0.08, 0.08, 1.0), odd);
    }

    let a = color.a * coverage * layer.params.y;
    return vec4<f32>(color.rgb * a, a);
}

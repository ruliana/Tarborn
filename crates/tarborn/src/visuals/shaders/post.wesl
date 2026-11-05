// Full-screen finish: optional ordered dither to a 4-color palette, then
// vignette and film grain.
import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;

struct Post {
    p0: vec4<f32>,
    p1: vec4<f32>,
    p2: vec4<f32>,
    p3: vec4<f32>,
    danger: vec4<f32>,
    time: f32,
    grain: f32,
    grain_fps: f32,
    vignette: f32,
    vignette_start: f32,
    vignette_end: f32,
    dither: f32,
    dither_pixel: f32,
    dither_spread: f32,
    red_keep: f32,
}

@group(0) @binding(2) var<uniform> post: Post;

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn bayer4(x: u32, y: u32) -> f32 {
    var m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return m[(x % 4u) + (y % 4u) * 4u] / 16.0;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(screen_texture));
    var color = textureSampleLevel(screen_texture, texture_sampler, in.uv, 0.0).rgb;

    if post.dither > 0.5 {
        let px = max(post.dither_pixel, 1.0);
        let cell = floor(in.uv * dims / px);
        let c = textureSampleLevel(screen_texture, texture_sampler, (cell + 0.5) * px / dims, 0.0).rgb;
        if c.r > post.red_keep && c.g < c.r * 0.5 && c.b < c.r * 0.5 {
            color = post.danger.rgb;
        } else {
            // Perceptual lightness from linear color.
            let l = pow(dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)), 1.0 / 2.2);
            let v = l * 3.0 + (bayer4(u32(cell.x), u32(cell.y)) - 0.47) * post.dither_spread;
            let i = clamp(round(v), 0.0, 3.0);
            if i < 0.5 {
                color = post.p0.rgb;
            } else if i < 1.5 {
                color = post.p1.rgb;
            } else if i < 2.5 {
                color = post.p2.rgb;
            } else {
                color = post.p3.rgb;
            }
        }
    }

    let d = distance(in.uv, vec2<f32>(0.5)) * 1.41421;
    color = color * (1.0 - smoothstep(post.vignette_start, post.vignette_end, d) * post.vignette);

    let frame = floor(post.time * post.grain_fps);
    let g = hash(floor(in.uv * dims) + vec2<f32>(frame * 17.0, frame * 31.0)) - 0.5;
    color = max(color + g * post.grain, vec3<f32>(0.0));
    return vec4<f32>(color, 1.0);
}

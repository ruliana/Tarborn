// The body of every solid, colored by how deep inside it a point is: a
// ragged trim band along exposed edges, a darker seam, then panelled fill
// that darkens with depth, all under a fine grain.
import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;

struct Body {
    // xy = raster origin, zw = raster size (world px).
    area: vec4<f32>,
    fill: vec4<f32>,
    trim: vec4<f32>,
    seam: vec4<f32>,
    // trim depth, seam width, wobble, wobble scale.
    band: vec4<f32>,
    // panel width, panel height, panel variation, grain.
    panel: vec4<f32>,
    // grain scale, depth darken, depth range, depth max.
    shade: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> body: Body;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var depth_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var depth_sampler: sampler;

fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(i);
    let b = hash(i + vec2<f32>(1.0, 0.0));
    let c = hash(i + vec2<f32>(0.0, 1.0));
    let d = hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xy;
    var uv = (p - body.area.xy) / body.area.zw;
    uv.y = 1.0 - uv.y;
    let depth = textureSample(depth_texture, depth_sampler, uv).r * body.shade.w;

    let trim_depth = body.band.x;
    let seam_width = body.band.y;
    let wobble = (noise(p / max(body.band.w, 1.0)) - 0.5) * 2.0 * body.band.z;
    let d = depth + wobble;

    let grain = (hash(floor(p / max(body.shade.x, 0.5))) - 0.5) * body.panel.w;

    var color: vec3<f32>;
    if d < trim_depth {
        color = body.trim.rgb;
    } else if d < trim_depth + seam_width {
        color = body.seam.rgb;
    } else {
        let panel = hash(floor(p / max(body.panel.xy, vec2<f32>(1.0))) + 17.0);
        let vary = 1.0 + (panel - 0.5) * 2.0 * body.panel.z;
        let deep = smoothstep(0.0, max(body.shade.z, 1.0), d - trim_depth - seam_width);
        color = body.fill.rgb * vary * (1.0 - body.shade.y * deep);
    }
    return vec4<f32>(max(color * (1.0 + grain), vec3<f32>(0.0)), 1.0);
}

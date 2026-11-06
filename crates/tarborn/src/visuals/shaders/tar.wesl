// The tar blob: the sim's outline polygon, rounded outwards, shaded as a
// glossy bead. A colored rim shows the blob's state.
import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;

struct Tar {
    // Outline points in world pixels, two per vec4 (xy, zw).
    points: array<vec4<f32>, 16>,
    count: u32,
    round: f32,
    bevel: f32,
    aa: f32,
    body: vec4<f32>,
    highlight: vec4<f32>,
    // State color in rgb; a = how strongly it shows.
    tint: vec4<f32>,
    // xyz = light direction, w = specular power.
    light: vec4<f32>,
    // x = specular strength, y = rim strength, z = rim power, w = body tint.
    strength: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> tar: Tar;

fn point(i: u32) -> vec2<f32> {
    let v = tar.points[i / 2u];
    if (i % 2u) == 0u {
        return v.xy;
    }
    return v.zw;
}

// Signed distance to the outline polygon (negative inside), after Inigo Quilez.
fn sd(p: vec2<f32>) -> f32 {
    let n = tar.count;
    let first = p - point(0u);
    var d = dot(first, first);
    var s = 1.0;
    var j = n - 1u;
    for (var i = 0u; i < n; i++) {
        let vi = point(i);
        let vj = point(j);
        let e = vj - vi;
        let w = p - vi;
        let b = w - e * clamp(dot(w, e) / max(dot(e, e), 1e-6), 0.0, 1.0);
        d = min(d, dot(b, b));
        let c1 = p.y >= vi.y;
        let c2 = p.y < vj.y;
        let c3 = e.x * w.y > e.y * w.x;
        if (c1 && c2 && c3) || (!c1 && !c2 && !c3) {
            s = -s;
        }
        j = i;
    }
    return s * sqrt(d) - tar.round;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xy;
    let d = sd(p);
    let alpha = 1.0 - smoothstep(-tar.aa, tar.aa, d);
    if alpha <= 0.0 {
        discard;
    }
    let grad = vec2<f32>(
        sd(p + vec2<f32>(1.0, 0.0)) - sd(p - vec2<f32>(1.0, 0.0)),
        sd(p + vec2<f32>(0.0, 1.0)) - sd(p - vec2<f32>(0.0, 1.0)),
    );
    let n2 = grad / max(length(grad), 1e-5);
    // Height: 0 at the edge, 1 once `bevel` pixels inside.
    let h = clamp(-d / max(tar.bevel, 1e-3), 0.0, 1.0);
    let nz = sin(h * 1.5707964);
    let n = normalize(vec3<f32>(n2 * sqrt(max(1.0 - nz * nz, 0.0)), nz));
    let l = normalize(tar.light.xyz);
    let r = reflect(-l, n);
    let spec = pow(max(r.z, 0.0), tar.light.w) * tar.strength.x;
    let rim = pow(1.0 - h, tar.strength.z) * tar.strength.y * tar.tint.a;
    var color = mix(tar.body.rgb, tar.tint.rgb, tar.tint.a * tar.strength.w);
    color = color + tar.highlight.rgb * spec + tar.tint.rgb * rim;
    return vec4<f32>(color, alpha);
}

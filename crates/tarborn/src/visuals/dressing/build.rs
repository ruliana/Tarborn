//! Passes 2 to 5: lay pieces from a [`DressingSet`] onto a read [`Shape`].
//! Everything here is geometry pushed into [`Fill`] batches; y points up.

use bevy::prelude::*;

use super::set::*;
use super::shape::{CornerKind, EdgeClass, Run, Shape};
use crate::visuals::fill::Fill;
use crate::visuals::tuning::{Rgb, srgb};
use crate::visuals::wfc::{self, Rng};

fn rgba(c: Rgba) -> Color {
    Color::srgba(c.0, c.1, c.2, c.3)
}

fn range(rng: &mut Rng, (lo, hi): (f32, f32)) -> f32 {
    lo + (hi - lo) * rng.next_f32()
}

fn pick<'a, T>(rng: &mut Rng, items: &'a [T]) -> Option<&'a T> {
    if items.is_empty() {
        return None;
    }
    let i = ((rng.next_f32() * items.len() as f32) as usize).min(items.len() - 1);
    Some(&items[i])
}

fn weighted<'a, T>(rng: &mut Rng, items: &'a [(T, f32)]) -> Option<&'a T> {
    let total: f32 = items.iter().map(|(_, w)| w.max(0.0)).sum();
    if total <= 0.0 {
        return None;
    }
    let mut r = rng.next_f32() * total;
    for (item, w) in items {
        r -= w.max(0.0);
        if r <= 0.0 {
            return Some(item);
        }
    }
    items.last().map(|(item, _)| item)
}

/// Along-edge direction for an outward normal.
fn along(n: Vec2) -> Vec2 {
    Vec2::new(n.y, -n.x)
}

pub fn dot(fill: &mut Fill, at: Vec2, radius: f32, angle: f32, d: &Dot) {
    let radii = Vec2::new(radius * d.squash.max(0.1), radius);
    if d.shadow.3 > 0.0 {
        fill.ellipse(
            at + Vec2::new(0.35, -0.45) * radius,
            radii,
            angle,
            12,
            rgba(d.shadow),
        );
        fill.ellipse(at, radii * 0.8, angle, 12, srgb(d.color));
    } else {
        fill.ellipse(at, radii, angle, 12, srgb(d.color));
    }
}

fn blade(fill: &mut Fill, base: Vec2, up: Vec2, length: f32, lean: f32, width: f32, color: Color) {
    let side = along(up);
    let tip = base + up * length + side * lean;
    let control = base + up * length * 0.6 + side * lean * 0.3;
    fill.tapered(
        &Fill::curve(base, control, tip, 5),
        width,
        width * 0.2,
        color,
    );
}

fn mushroom(fill: &mut Fill, base: Vec2, up: Vec2, size: f32, stem: Rgb, cap: Rgb, spot: Rgb) {
    let side = along(up);
    let top = base + up * size * 1.2;
    fill.polygon(
        &[
            base - side * size * 0.25,
            base + side * size * 0.25,
            top + side * size * 0.25,
            top - side * size * 0.25,
        ],
        srgb(stem),
    );
    let half: Vec<Vec2> = (0..=10)
        .map(|k| {
            let a = k as f32 / 10.0 * std::f32::consts::PI;
            top + side * a.cos() * size + up * a.sin() * size * 0.6
        })
        .collect();
    fill.polygon(&half, srgb(cap));
    fill.circle(
        top - side * size * 0.35 + up * size * 0.3,
        size * 0.15,
        6,
        srgb(spot),
    );
}

// ------------------------------------------------------------------ interior

pub fn interior(shape: &Shape, set: &DressingSet, rng: &mut Rng, fill: &mut Fill) {
    let it = &set.interior;
    let cell = it.cell.max(4.0);
    let min_depth = set.body.trim_depth + set.body.seam_width + it.margin;
    // Cover the whole raster, not just the open space.
    let lo = shape.origin;
    let hi = shape.origin + Vec2::new(shape.width as f32, shape.height as f32) * shape.cell;
    let width = ((hi.x - lo.x) / cell).floor() as usize;
    let height = ((hi.y - lo.y) / cell).floor() as usize;
    let center = |x: usize, y: usize| lo + (Vec2::new(x as f32, y as f32) + 0.5) * cell;
    let mut inside = vec![false; width * height];
    for y in 0..height {
        for x in 0..width {
            let c = center(x, y);
            let h = cell * 0.5;
            inside[y * width + x] = [
                Vec2::ZERO,
                Vec2::new(-h, -h),
                Vec2::new(h, -h),
                Vec2::new(h, h),
                Vec2::new(-h, h),
            ]
            .iter()
            .all(|d| {
                let depth = shape.depth_at(c + *d);
                depth >= min_depth && depth < 1e5
            });
        }
    }
    let grid = wfc::Grid {
        width,
        height,
        inside,
    };
    let tiles = wfc::collapse(&grid, &it.weights, rng);
    let color = srgb(it.color);
    let dirs = [
        (wfc::N, Vec2::Y),
        (wfc::E, Vec2::X),
        (wfc::S, Vec2::NEG_Y),
        (wfc::W, Vec2::NEG_X),
    ];
    for y in 0..height {
        for x in 0..width {
            let Some(tile) = tiles[y * width + x] else {
                continue;
            };
            let c = center(x, y);
            for (bit, dir) in dirs {
                if tile & bit == 0 {
                    continue;
                }
                let w = range(rng, it.width);
                match it.style {
                    InteriorStyle::Girders => {
                        fill.segment(c - dir * w * 0.5, c + dir * cell * 0.5, w, color)
                    }
                    InteriorStyle::Roots => {
                        // Bend each half-stroke by a hash of its shared edge so both
                        // cells that meet there agree on the end point.
                        let end = c + dir * cell * 0.5;
                        let mid = (c + end) * 0.5 + along(dir) * (rng.next_f32() - 0.5) * it.bend;
                        fill.tapered(&Fill::curve(c, mid, end, 5), w, w * 0.7, color);
                        fill.circle(c, w * 0.5, 6, color);
                    }
                }
            }
            if tile == 0 && rng.next_f32() < it.dot_chance {
                let r = range(rng, it.dot.radius);
                let a = rng.next_f32() * std::f32::consts::TAU;
                dot(fill, c, r, a, &it.dot);
            }
        }
    }
}

// ------------------------------------------------------------------ strips

pub fn strips(shape: &Shape, set: &DressingSet, rng: &mut Rng, fill: &mut Fill) {
    for run in &shape.runs {
        let pieces = match run.class {
            EdgeClass::Floor => &set.strips.floor,
            EdgeClass::Slope => &set.strips.slope,
            EdgeClass::Wall => &set.strips.wall,
            EdgeClass::Ceiling => &set.strips.ceiling,
            EdgeClass::ArcConcave => &set.strips.arc_concave,
            EdgeClass::ArcConvex => &set.strips.arc_convex,
        };
        for piece in pieces {
            strip(run, piece, set.body.trim_depth, rng, fill);
        }
    }
}

fn strip(run: &Run, piece: &Piece, trim: f32, rng: &mut Rng, fill: &mut Fill) {
    match piece {
        Piece::Plates {
            length,
            seam,
            seam_width,
            vent_chance,
            vent_length,
            slit_spacing,
            slit,
            slit_width,
            bolt_inset,
            bolt,
        } => {
            // One-dimensional WFC: plates and vents, never two vents in a row,
            // and a bolted plate at each end.
            let mut s = 0.0;
            let mut last_vent = true;
            while s < run.length - 1.0 {
                let remaining = run.length - s;
                let vent = !last_vent
                    && remaining > vent_length + 2.0 * bolt_inset * 2.0
                    && rng.next_f32() < *vent_chance;
                let len = if vent {
                    *vent_length
                } else {
                    range(rng, *length)
                };
                let len = if remaining - len < length.0 * 0.5 {
                    remaining
                } else {
                    len.min(remaining)
                };
                let e = s + len;
                if s > 0.0 {
                    let (p, n) = run.at(s);
                    fill.segment(p - n * 0.5, p - n * trim, *seam_width, srgb(*seam));
                }
                if vent && e < run.length {
                    let mut k = s + slit_spacing;
                    while k < e - slit_spacing * 0.5 {
                        let (p, n) = run.at(k);
                        fill.segment(p - n * 2.5, p - n * (trim - 2.5), *slit_width, srgb(*slit));
                        k += slit_spacing;
                    }
                } else {
                    for b in [s + bolt_inset, (e - bolt_inset).max(s + bolt_inset)] {
                        let (p, n) = run.at(b);
                        dot(fill, p - n * trim * 0.5, bolt.radius.0, 0.0, bolt);
                    }
                }
                last_vent = vent;
                s = e;
            }
        }
        Piece::Bolts {
            spacing,
            depth,
            dot: d,
        } => run.walk(*spacing, |p, n, _| {
            let r = d.radius.0;
            dot(fill, p - n * *depth, r, 0.0, d);
        }),
        Piece::Ribs {
            spacing,
            from,
            to,
            slant,
            width,
            color,
        } => run.walk(*spacing, |p, n, _| {
            let t = along(n);
            fill.segment(
                p - n * *from - t * slant * 0.5,
                p - n * *to + t * slant * 0.5,
                *width,
                srgb(*color),
            );
        }),
        Piece::Hoops {
            depths,
            width,
            color,
        } => {
            for d in depths {
                let pts: Vec<Vec2> = run
                    .points
                    .iter()
                    .zip(&run.normals)
                    .map(|(p, n)| *p - *n * *d)
                    .collect();
                fill.polyline(&pts, *width, srgb(*color));
            }
        }
        Piece::Grass {
            spacing,
            length,
            lean,
            width,
            colors,
            min_up,
        } => run.walk(*spacing, |p, n, _| {
            if n.y < *min_up {
                return;
            }
            let len = range(rng, *length);
            let l = (rng.next_f32() - 0.5) * 2.0 * lean;
            let color = pick(rng, colors).copied().unwrap_or((0.4, 0.5, 0.3));
            blade(fill, p - n * 2.0, n, len, l, *width, srgb(color));
        }),
        Piece::Mushrooms {
            spacing,
            chance,
            size,
            stem,
            cap,
            spot,
        } => {
            let mut s = range(rng, *spacing);
            while s < run.length {
                if rng.next_f32() < *chance {
                    let (p, n) = run.at(s);
                    let a = range(rng, *size);
                    mushroom(fill, p - n, n, a, *stem, *cap, *spot);
                    mushroom(
                        fill,
                        p - n + along(n) * a * 1.7,
                        n,
                        a * 0.65,
                        *stem,
                        *cap,
                        *spot,
                    );
                }
                s += range(rng, *spacing);
            }
        }
        Piece::Fringe {
            spacing,
            length,
            width,
            sway,
            color,
            min_down,
        } => run.walk(*spacing, |p, n, _| {
            if n.y > -*min_down {
                return;
            }
            let len = range(rng, *length);
            let tip = p + Vec2::new((rng.next_f32() - 0.5) * sway * 0.4, -len);
            let mid = p + Vec2::new((rng.next_f32() - 0.5) * sway, -len * 0.6);
            fill.tapered(
                &Fill::curve(p + Vec2::Y * 1.0, mid, tip, 5),
                *width,
                width * 0.3,
                srgb(*color),
            );
        }),
        Piece::Leaves {
            spacing,
            chance,
            size,
            colors,
        } => run.walk(*spacing, |p, n, _| {
            if rng.next_f32() > *chance {
                return;
            }
            let r = range(rng, *size);
            let color = pick(rng, colors).copied().unwrap_or((0.3, 0.4, 0.2));
            let a = rng.next_f32() * std::f32::consts::PI;
            fill.ellipse(p + n * 1.5, Vec2::new(r, r * 0.6), a, 10, srgb(color));
        }),
        Piece::Cracks {
            spacing,
            depth,
            width,
            color,
        } => run.walk(*spacing, |p, n, _| {
            let d = range(rng, *depth);
            let t = along(n);
            let mut pts = vec![p - n * 3.0];
            for k in 1..=4 {
                let u = k as f32 / 4.0;
                pts.push(p - n * (3.0 + (d - 3.0) * u) + t * (rng.next_f32() - 0.5) * d * 0.35);
            }
            fill.tapered(&pts, *width, width * 0.3, srgb(*color));
        }),
    }
}

// ------------------------------------------------------------------ corners

/// Clips convex `subject` to convex counter-clockwise `clip` (Sutherland–Hodgman).
fn clip_convex(subject: &[Vec2], clip: &[Vec2]) -> Vec<Vec2> {
    let mut out = subject.to_vec();
    for i in 0..clip.len() {
        if out.is_empty() {
            break;
        }
        let (a, b) = (clip[i], clip[(i + 1) % clip.len()]);
        let inside = |p: Vec2| (b - a).perp_dot(p - a) >= 0.0;
        let input = std::mem::take(&mut out);
        for j in 0..input.len() {
            let (p, q) = (input[j], input[(j + 1) % input.len()]);
            let (pin, qin) = (inside(p), inside(q));
            if pin {
                out.push(p);
            }
            if pin != qin {
                let d = (b - a).perp_dot(q - p);
                if d.abs() > 1e-9 {
                    let t = (b - a).perp_dot(a - p) / d;
                    out.push(p + (q - p) * t);
                }
            }
        }
    }
    out
}

/// Draws `poly` (convex) only where it overlaps the solids.
fn clipped(fill: &mut Fill, solids: &[Vec<Vec2>], poly: &[Vec2], color: Color) {
    let (lo, hi) = poly.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), v| {
        (lo.min(*v), hi.max(*v))
    });
    for s in solids {
        let (a, b) = s.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), v| {
            (lo.min(*v), hi.max(*v))
        });
        if a.x > hi.x || a.y > hi.y || b.x < lo.x || b.y < lo.y {
            continue;
        }
        let part = clip_convex(poly, s);
        if part.len() >= 3 {
            fill.polygon(&part, color);
        }
    }
}

pub fn corners(
    shape: &Shape,
    solids: &[Vec<Vec2>],
    set: &DressingSet,
    rng: &mut Rng,
    fill: &mut Fill,
) {
    for corner in &shape.corners {
        let pieces = match corner.kind {
            CornerKind::Outer => &set.corners.outer,
            CornerKind::Inner => &set.corners.inner,
        };
        let (at, inward) = (corner.at, corner.inward);
        let angle = inward.to_angle();
        for piece in pieces {
            match piece {
                CornerPiece::Cap {
                    size,
                    inset,
                    color,
                    edge,
                    edge_width,
                    bolt_depth,
                    bolt,
                } => {
                    let center = at + inward * *inset;
                    let square = |half: f32| -> Vec<Vec2> {
                        (0..4)
                            .map(|k| {
                                center
                                    + Vec2::from_angle(
                                        angle + k as f32 * std::f32::consts::FRAC_PI_2,
                                    ) * half
                                        * std::f32::consts::SQRT_2
                            })
                            .collect()
                    };
                    clipped(fill, solids, &square(size * 0.5 + edge_width), srgb(*edge));
                    clipped(fill, solids, &square(size * 0.5), srgb(*color));
                    dot(fill, at + inward * *bolt_depth, bolt.radius.0, 0.0, bolt);
                }
                CornerPiece::Bracket {
                    radius,
                    spread,
                    color,
                    edge,
                    edge_width,
                    bolts,
                    bolt,
                } => {
                    let sector = |r: f32| -> Vec<Vec2> {
                        let mut pts = vec![at];
                        pts.extend((0..=12).map(|k| {
                            at + Vec2::from_angle(angle - spread + 2.0 * spread * k as f32 / 12.0)
                                * r
                        }));
                        pts
                    };
                    clipped(fill, solids, &sector(radius + edge_width), srgb(*edge));
                    clipped(fill, solids, &sector(*radius), srgb(*color));
                    for k in 0..*bolts {
                        let u = if *bolts > 1 {
                            k as f32 / (*bolts - 1) as f32 - 0.5
                        } else {
                            0.0
                        };
                        let p = at + Vec2::from_angle(angle + u * spread * 1.1) * radius * 0.6;
                        dot(fill, p, bolt.radius.0, 0.0, bolt);
                    }
                }
                CornerPiece::Tuft {
                    blades,
                    spread,
                    length,
                    width,
                    colors,
                } => {
                    let out = -inward;
                    for j in 0..*blades {
                        let a = (j as f32 - (*blades as f32 - 1.0) * 0.5) * spread;
                        let dir = Vec2::from_angle(out.to_angle() + a);
                        let color = pick(rng, colors).copied().unwrap_or((0.4, 0.5, 0.3));
                        blade(
                            fill,
                            at - out * 2.0,
                            dir,
                            range(rng, *length),
                            0.0,
                            *width,
                            srgb(color),
                        );
                    }
                }
                CornerPiece::Fern {
                    fronds,
                    width,
                    leaflet,
                    color,
                } => {
                    let out = -inward;
                    for (offset, length) in fronds {
                        let dir = Vec2::from_angle(out.to_angle() + offset);
                        let side = dir.perp() * offset.signum().max(0.0).mul_add(2.0, -1.0);
                        let pts: Vec<Vec2> = (0..=10)
                            .map(|k| {
                                let u = k as f32 / 10.0;
                                at + dir * length * u + side * length * 0.25 * u * u
                            })
                            .collect();
                        fill.tapered(&pts, *width, width * 0.4, srgb(*color));
                        for (k, w) in pts.windows(2).enumerate().skip(1) {
                            let u = k as f32 / 10.0;
                            let t = (w[1] - w[0]).normalize_or_zero();
                            let l = leaflet * (1.0 - u) + 1.5;
                            for s in [-1.0, 1.0] {
                                fill.segment(
                                    w[0],
                                    w[0] + t.perp() * l * s + t * 2.0,
                                    width * 0.6,
                                    srgb(*color),
                                );
                            }
                        }
                    }
                }
                CornerPiece::Mushrooms {
                    count,
                    size,
                    stem,
                    cap,
                    spot,
                } => {
                    // Only at the foot of a wall, standing on the floor.
                    if inward.y > -0.2 {
                        continue;
                    }
                    let away = Vec2::new(-inward.x.signum(), 0.0);
                    for k in 0..*count {
                        let s = range(rng, *size);
                        mushroom(
                            fill,
                            at + away * (8.0 + k as f32 * 11.0),
                            Vec2::Y,
                            s,
                            *stem,
                            *cap,
                            *spot,
                        );
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ hanging

pub struct Hanger {
    kind: usize,
    anchor: Vec2,
    end: Vec2,
    length: f32,
    phase: f32,
    sag: f32,
}

pub fn plan_hanging(shape: &Shape, set: &DressingSet, rng: &mut Rng) -> Vec<Hanger> {
    let h = &set.hanging;
    let mut hangers = Vec::new();
    for run in &shape.runs {
        if !matches!(run.class, EdgeClass::Ceiling | EdgeClass::ArcConcave) {
            continue;
        }
        let mut anchors = Vec::new();
        let mut s = range(rng, h.spacing) * 0.5;
        while s < run.length - 4.0 {
            let (p, n) = run.at(s);
            if n.y <= -h.min_down {
                anchors.push(p);
            }
            s += range(rng, h.spacing);
        }
        for (i, &a) in anchors.iter().enumerate() {
            let mut drop = shape.cell;
            while drop < 4000.0 && !shape.is_solid(a - Vec2::Y * (drop + shape.cell)) {
                drop += shape.cell;
            }
            let room = drop - h.floor_gap;
            if room < 16.0 {
                continue;
            }
            let Some(kind) = weighted(
                rng,
                &h.kinds
                    .iter()
                    .enumerate()
                    .map(|(k, (_, w))| (k, *w))
                    .collect::<Vec<_>>(),
            )
            .copied() else {
                continue;
            };
            let phase = rng.next_f32() * std::f32::consts::TAU;
            match &h.kinds[kind].0 {
                Hang::Span { reach, sag, .. } => {
                    let Some(&b) = anchors.get(i + 1) else {
                        continue;
                    };
                    if a.distance(b) > *reach {
                        continue;
                    }
                    let sag = range(rng, *sag).min(room);
                    hangers.push(Hanger {
                        kind,
                        anchor: a,
                        end: b,
                        length: 0.0,
                        phase,
                        sag,
                    });
                }
                Hang::Chain { length, .. }
                | Hang::Vine { length, .. }
                | Hang::Light { length, .. } => {
                    let length = range(rng, *length).min(room);
                    hangers.push(Hanger {
                        kind,
                        anchor: a,
                        end: a,
                        length,
                        phase,
                        sag: 0.0,
                    });
                }
            }
        }
    }
    hangers
}

fn leaf_row(fill: &mut Fill, pts: &[Vec2], row: &LeafRow, t: f32, phase: f32) {
    let mut acc = 0.0;
    let mut next = row.spacing * 0.5;
    let mut side = 1.0;
    for (k, w) in pts.windows(2).enumerate() {
        let len = w[0].distance(w[1]);
        while next <= acc + len && len > 0.0 {
            let u = (next - acc) / len;
            let p = w[0].lerp(w[1], u);
            let dir = (w[1] - w[0]) / len;
            let a = dir.to_angle() + side * 1.1 + (t * 1.7 + phase + k as f32).sin() * 0.15;
            let color = row
                .colors
                .get((k + (side > 0.0) as usize) % row.colors.len().max(1))
                .copied()
                .unwrap_or((0.3, 0.4, 0.2));
            fill.ellipse(
                p + Vec2::from_angle(a) * row.size.0 * 0.6,
                Vec2::new(row.size.0, row.size.1),
                a,
                8,
                srgb(color),
            );
            side = -side;
            next += row.spacing;
        }
        acc += len;
    }
}

pub fn draw_hanging(
    hangers: &[Hanger],
    shape: &Shape,
    set: &DressingSet,
    t: f32,
    seed: u64,
    fill: &mut Fill,
) {
    let h = &set.hanging;
    for hanger in hangers {
        let Some((kind, _)) = h.kinds.get(hanger.kind) else {
            continue;
        };
        let swing = (t * h.sway_speed + hanger.phase).sin() * h.sway;
        let a = hanger.anchor;
        let down = Vec2::from_angle(-std::f32::consts::FRAC_PI_2 + swing);
        let end = a + down * hanger.length;
        let line = |n: usize, wiggle: f32| -> Vec<Vec2> {
            (0..=n)
                .map(|k| {
                    let u = k as f32 / n as f32;
                    a.lerp(end, u) + down.perp() * (u * 5.0 + hanger.phase).sin() * wiggle * u
                })
                .collect()
        };
        match kind {
            Hang::Chain {
                link,
                width,
                color,
                hook,
                ..
            } => {
                let n = (hanger.length / link).max(2.0) as usize;
                for k in 0..n {
                    let u = (k as f32 + 0.5) / n as f32;
                    let c = a.lerp(end, u);
                    let radii = if k % 2 == 0 {
                        Vec2::new(link * 0.3, link * 0.6)
                    } else {
                        Vec2::new(link * 0.12, link * 0.6)
                    };
                    let ring: Vec<Vec2> = (0..=10)
                        .map(|j| {
                            c + Vec2::from_angle(down.to_angle()).rotate(
                                Vec2::from_angle(j as f32 / 10.0 * std::f32::consts::TAU)
                                    * Vec2::new(radii.y, radii.x),
                            )
                        })
                        .collect();
                    fill.polyline(&ring, *width, srgb(*color));
                }
                let hook_pts: Vec<Vec2> = (0..=10)
                    .map(|j| {
                        let ang = std::f32::consts::FRAC_PI_2
                            + j as f32 / 10.0 * 1.45 * std::f32::consts::PI;
                        end + down * hook + Vec2::from_angle(ang + swing) * hook
                    })
                    .collect();
                fill.polyline(&hook_pts, width * 1.4, srgb(*color));
            }
            Hang::Vine {
                width,
                color,
                leaves,
                ..
            } => {
                let pts = line(((hanger.length / 8.0) as usize).max(3), 2.5);
                fill.polyline(&pts, *width, srgb(*color));
                leaf_row(fill, &pts, leaves, t, hanger.phase);
            }
            Hang::Light {
                cord,
                cord_width,
                shade,
                bulb,
                glow,
                glow_size,
                cone,
                pulse,
                ..
            } => {
                fill.polyline(&[a, end], *cord_width, srgb(*cord));
                let p = 1.0 - pulse * 0.5 + pulse * 0.5 * (t * 2.0 + hanger.phase).sin();
                let g = rgba((glow.0, glow.1, glow.2, glow.3 * p));
                let clear = rgba((glow.0, glow.1, glow.2, 0.0));
                let side = down.perp();
                if *cone {
                    let top = end + down * 4.0;
                    let bottom = end + down * *glow_size;
                    fill.polygon_colors(
                        &[
                            top - side * 6.0,
                            top + side * 6.0,
                            bottom + side * glow_size * 0.5,
                            bottom - side * glow_size * 0.5,
                        ],
                        &[g, g, clear, clear],
                    );
                } else {
                    fill.glow(end + down * bulb.1 * 0.5, *glow_size, 20, g, clear);
                }
                if let Some((size, color)) = shade {
                    fill.polygon(
                        &[
                            end - side * size * 0.15,
                            end + side * size * 0.15,
                            end + down * size * 0.5 + side * size * 0.5,
                            end + down * size * 0.5 - side * size * 0.5,
                        ],
                        srgb(*color),
                    );
                    fill.ellipse(
                        end + down * size * 0.5,
                        Vec2::new(bulb.0, bulb.1 * 0.5),
                        down.to_angle() + std::f32::consts::FRAC_PI_2,
                        10,
                        srgb(bulb.2),
                    );
                } else {
                    fill.ellipse(
                        end + down * bulb.1 * 0.5,
                        Vec2::new(bulb.0, bulb.1),
                        down.to_angle() + std::f32::consts::FRAC_PI_2,
                        12,
                        srgb(bulb.2),
                    );
                }
            }
            Hang::Span {
                width,
                color,
                leaves,
                ..
            } => {
                let sag = hanger.sag * (1.0 + 0.06 * (t * 0.9 + hanger.phase).sin());
                let n = ((hanger.anchor.distance(hanger.end) / 8.0) as usize).max(6);
                let pts: Vec<Vec2> = (0..=n)
                    .map(|k| {
                        let u = k as f32 / n as f32;
                        hanger.anchor.lerp(hanger.end, u) - Vec2::Y * 4.0 * sag * u * (1.0 - u)
                    })
                    .collect();
                fill.polyline(&pts, *width, srgb(*color));
                if let Some(row) = leaves {
                    leaf_row(fill, &pts, row, t, hanger.phase);
                }
            }
        }
    }

    // Motes drifting in the open.
    let m = &h.motes;
    if m.count > 0 && m.color.3 > 0.0 {
        let mut rng = Rng::new(seed ^ 0x5eed);
        let size = shape.open_max - shape.open_min;
        for k in 0..m.count {
            let base = shape.open_min + Vec2::new(rng.next_f32(), rng.next_f32()) * size;
            let f = k as f32;
            let p = base + Vec2::new((t * 0.6 + f).sin(), (t * 0.8 + f * 1.3).cos()) * m.drift;
            if shape.is_solid(p) {
                continue;
            }
            let a = m.color.3 * (0.5 + 0.5 * (t * m.twinkle + f * 2.1).sin());
            fill.glow(
                p,
                m.radius,
                10,
                rgba((m.color.0, m.color.1, m.color.2, a)),
                rgba((m.color.0, m.color.1, m.color.2, 0.0)),
            );
        }
    }
}

// ------------------------------------------------------------------ skyline

fn truss(fill: &mut Fill, x0: f32, x1: f32, y: f32, depth: f32, line: f32, color: Color) {
    fill.segment(Vec2::new(x0, y), Vec2::new(x1, y), line, color);
    fill.segment(
        Vec2::new(x0, y - depth),
        Vec2::new(x1, y - depth),
        line,
        color,
    );
    let n = ((x1 - x0) / depth).round().max(2.0) as usize;
    let step = (x1 - x0) / n as f32;
    for k in 0..n {
        let xa = x0 + k as f32 * step;
        fill.segment(
            Vec2::new(xa, y - depth),
            Vec2::new(xa + step * 0.5, y),
            line,
            color,
        );
        fill.segment(
            Vec2::new(xa + step * 0.5, y),
            Vec2::new(xa + step, y - depth),
            line,
            color,
        );
    }
}

fn lattice(fill: &mut Fill, x: f32, y0: f32, y1: f32, w: f32, line: f32, color: Color) {
    fill.segment(Vec2::new(x, y0), Vec2::new(x, y1), line, color);
    fill.segment(Vec2::new(x + w, y0), Vec2::new(x + w, y1), line, color);
    let mut y = y0;
    while y + w <= y1 {
        fill.segment(Vec2::new(x, y), Vec2::new(x + w, y + w), line, color);
        fill.segment(Vec2::new(x + w, y), Vec2::new(x, y + w), line, color);
        fill.segment(Vec2::new(x, y), Vec2::new(x + w, y), line, color);
        y += w;
    }
}

fn span(fill: &mut Fill, style: &Span2, x0: f32, x1: f32, y: f32, rng: &mut Rng, color: Color) {
    match style {
        Span2::Truss { depth, line } => truss(fill, x0, x1, y, *depth, *line, color),
        Span2::Branch {
            width,
            sag,
            leaf_radius,
            leaf_spacing,
        } => {
            let a = Vec2::new(x0, y);
            let b = Vec2::new(x1, y);
            let pts = Fill::curve(a, (a + b) * 0.5 - Vec2::Y * *sag, b, 16);
            fill.tapered(&pts, *width, width * 0.5, color);
            let mut x = x0 + leaf_spacing * 0.5;
            while x < x1 - leaf_spacing * 0.3 {
                let u = (x - x0) / (x1 - x0).max(1.0);
                let yy = y - sag * 4.0 * u * (1.0 - u) * 0.5;
                for _ in 0..4 {
                    let r = range(rng, *leaf_radius);
                    fill.circle(
                        Vec2::new(
                            x + (rng.next_f32() - 0.5) * leaf_spacing,
                            yy + 6.0 + rng.next_f32() * r * 1.4,
                        ),
                        r,
                        12,
                        color,
                    );
                }
                x += leaf_spacing;
            }
        }
    }
}

fn column(fill: &mut Fill, style: &Column, x: f32, y0: f32, y1: f32, color: Color) {
    match style {
        Column::Lattice { width, line } => lattice(fill, x, y0, y1, *width, *line, color),
        Column::Trunk { width, flare } => {
            fill.rect(
                Vec2::new(x + width * 0.5, (y0 + y1) * 0.5 + 15.0),
                Vec2::new(*width, y1 - y0 + 30.0),
                color,
            );
            let base = [
                Vec2::new(x - flare, y0),
                Vec2::new(x + width + flare, y0),
                Vec2::new(x + width, y0 + flare * 1.6),
                Vec2::new(x, y0 + flare * 1.6),
            ];
            fill.polygon(&base, color);
        }
    }
}

/// One skyline layer, in world pixels, standing on the floor of the open space.
pub fn skyline(shape: &Shape, layer: &SkyLayer, rng: &mut Rng) -> Fill {
    let mut fill = Fill::default();
    let color = srgb(layer.color);
    let (left, right) = (shape.open_min.x, shape.open_max.x);
    let base = shape.open_min.y;
    let top = shape.open_max.y;
    let mut towers: Vec<(f32, f32, f32)> = Vec::new();
    let mut x = left + range(rng, layer.gap);
    if !layer.structures.is_empty() {
        while x < right {
            let lot = range(rng, layer.lot);
            match weighted(rng, &layer.structures) {
                Some(Structure::Tank {
                    height,
                    width,
                    hoop_spacing,
                    hoop_width,
                    hoop,
                }) => {
                    let (h, w) = (range(rng, *height), range(rng, *width).min(lot.max(20.0)));
                    fill.rect(
                        Vec2::new(x + w * 0.5, base + h * 0.5),
                        Vec2::new(w, h),
                        color,
                    );
                    let dome: Vec<Vec2> = (0..=12)
                        .map(|k| {
                            let a = k as f32 / 12.0 * std::f32::consts::PI;
                            Vec2::new(
                                x + w * 0.5 + a.cos() * w * 0.5,
                                base + h + a.sin() * w * 0.22,
                            )
                        })
                        .collect();
                    fill.polygon(&dome, color);
                    let mut y = base + hoop_spacing;
                    while y < base + h {
                        fill.segment(
                            Vec2::new(x, y),
                            Vec2::new(x + w, y),
                            *hoop_width,
                            srgb(*hoop),
                        );
                        y += hoop_spacing;
                    }
                }
                Some(Structure::Stack { height, width, lip }) => {
                    let (h, w) = (range(rng, *height).min(top - base), range(rng, *width));
                    let cx = x + lot * 0.4;
                    fill.rect(Vec2::new(cx, base + h * 0.5), Vec2::new(w, h), color);
                    fill.rect(
                        Vec2::new(cx, base + h - lip * 0.5),
                        Vec2::new(w + lip, *lip),
                        color,
                    );
                }
                Some(Structure::Tower {
                    height,
                    width,
                    line,
                }) => {
                    let (h, w) = (range(rng, *height).min(top - base), range(rng, *width));
                    let tx = x + lot * 0.25;
                    lattice(&mut fill, tx, base, base + h, w, *line, color);
                    towers.push((tx, w, base + h));
                }
                Some(Structure::Pine {
                    height,
                    width,
                    tiers,
                }) => {
                    let (h, w) = (
                        range(rng, *height).min(top - base + 60.0),
                        range(rng, *width),
                    );
                    let cx = x + lot * 0.5;
                    let tw = (w * 0.12).max(4.0);
                    fill.rect(Vec2::new(cx, base + h * 0.3), Vec2::new(tw, h * 0.6), color);
                    let tiers = (*tiers).max(1);
                    for k in 0..tiers {
                        let y = base + h * 0.3 + k as f32 * h * 0.6 / tiers as f32;
                        let ww = w * (1.0 - k as f32 * 0.6 / tiers as f32);
                        fill.polygon(
                            &[
                                Vec2::new(cx - ww * 0.5, y),
                                Vec2::new(cx + ww * 0.5, y),
                                Vec2::new(cx, y + h * 0.32),
                            ],
                            color,
                        );
                    }
                }
                Some(Structure::Tree {
                    height,
                    width,
                    blobs,
                }) => {
                    let (h, w) = (
                        range(rng, *height).min(top - base + 60.0),
                        range(rng, *width),
                    );
                    let cx = x + lot * 0.5;
                    fill.rect(
                        Vec2::new(cx, base + h * 0.3),
                        Vec2::new((w * 0.1).max(6.0), h * 0.6),
                        color,
                    );
                    for _ in 0..*blobs {
                        let c = Vec2::new(
                            cx + (rng.next_f32() - 0.5) * w * 0.7,
                            base + h * (0.6 + rng.next_f32() * 0.3),
                        );
                        fill.circle(c, w * (0.2 + rng.next_f32() * 0.16), 16, color);
                    }
                }
                Some(Structure::Snag {
                    height,
                    width,
                    branches,
                    line,
                }) => {
                    let (h, w) = (range(rng, *height).min(top - base), range(rng, *width));
                    let cx = x + lot * 0.5;
                    fill.tapered(
                        &[Vec2::new(cx, base), Vec2::new(cx, base + h)],
                        w * 0.6,
                        w * 0.2,
                        color,
                    );
                    for k in 0..*branches {
                        let y = base + h * (0.45 + 0.5 * k as f32 / (*branches).max(1) as f32);
                        let s = if rng.next_f32() < 0.5 { -1.0 } else { 1.0 };
                        fill.tapered(
                            &[Vec2::new(cx, y), Vec2::new(cx + s * w * 1.6, y + w * 0.8)],
                            *line * 1.5,
                            line * 0.5,
                            color,
                        );
                    }
                    towers.push((cx - w * 0.3, w * 0.6, base + h));
                }
                Some(Structure::Gap) | None => {}
            }
            x += lot + range(rng, layer.gap);
        }
    }
    for pair in towers.windows(2) {
        if rng.next_f32() < layer.bridge_chance {
            let ((ax, aw, at), (bx, _, bt)) = (pair[0], pair[1]);
            let y = at.min(bt) - 20.0 - rng.next_f32() * 40.0;
            span(&mut fill, &layer.bridge, ax + aw, bx, y, rng, color);
        }
    }
    let count = layer.spans.0
        + (rng.next_f32() * (layer.spans.1.saturating_sub(layer.spans.0) + 1) as f32) as u32;
    for _ in 0..count.min(layer.spans.1.max(layer.spans.0)) {
        let y = base + (top - base) * range(rng, layer.span_height);
        let x0 = left + rng.next_f32() * (right - left) * 0.3;
        let x1 = (x0 + range(rng, layer.span_length)).min(right - 20.0);
        span(&mut fill, &layer.span, x0, x1, y, rng, color);
        let cw = match layer.column {
            Column::Lattice { width, .. } | Column::Trunk { width, .. } => width,
        };
        column(&mut fill, &layer.column, x0 + 10.0, base, y, color);
        column(&mut fill, &layer.column, x1 - 10.0 - cw, base, y, color);
    }
    fill
}

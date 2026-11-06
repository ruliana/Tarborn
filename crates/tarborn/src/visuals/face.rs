//! Eyes and body language. Every enemy shares one vocabulary: each state has a
//! [`Mood`] (how open the eyes are, how the body stretches and leans) and a gaze
//! (where the pupils point), so a state reads the same on a grub as on a brute.
//! Each kind has its own [`EyeSet`] on top. The tar blob has eyes too, the only
//! pale thing on it, and in a dark room they are all you see.
//!
//! Everything here is pure drawing math; `mod.rs` feeds it the simulation.

use std::f32::consts::TAU;

use bevy::prelude::*;
use serde::Deserialize;
use tarborn_sim::{AttackPhase, EnemyKind, State};

use super::fill::Fill;
use super::tuning::Rgb;

// ---------------------------------------------------------------- tuning

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Side by side.
    Row,
    /// Around a circle, the first one on top.
    Cluster,
}

/// One kind's eyes. Sizes and distances are fractions of the body radius.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct EyeSet {
    pub count: u32,
    pub layout: Layout,
    /// Width and height of one eye.
    pub size: (f32, f32),
    /// Row: distance between neighbours. Cluster: radius of the circle.
    pub spacing: f32,
    /// Centre of the group from the body centre: x toward the facing side, y up.
    pub offset: (f32, f32),
    /// Pupil diameter as a fraction of the eye's smaller side.
    pub pupil: f32,
    /// Raises the eyes on a stalk this long (0 = no stalk).
    pub stalk: f32,
    /// Thickness of a brow over each eye (0 = none).
    pub brow: f32,
}

/// How a state looks: eyes and body.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct Mood {
    /// Lid openness, 0 shut to 1 wide open.
    pub open: f32,
    /// Eye size multiplier.
    pub scale: f32,
    /// Pupil size multiplier.
    pub pupil: f32,
    /// Lid tilt in radians; positive lowers the inner corners (a frown).
    pub slant: f32,
    /// Body height multiplier; the width changes to keep the volume.
    pub stretch: f32,
    /// Body lean toward the facing side (shear, per unit of height).
    pub lean: f32,
    /// Side-to-side sway (shear amplitude) and its rate in Hz.
    pub wobble: f32,
    pub wobble_hz: f32,
    /// Breathing: height pulse amplitude and rate in Hz.
    pub bob: f32,
    pub bob_hz: f32,
}

impl Mood {
    pub fn lerp(self, to: Mood, k: f32) -> Mood {
        let l = |a: f32, b: f32| a + (b - a) * k;
        Mood {
            open: l(self.open, to.open),
            scale: l(self.scale, to.scale),
            pupil: l(self.pupil, to.pupil),
            slant: l(self.slant, to.slant),
            stretch: l(self.stretch, to.stretch),
            lean: l(self.lean, to.lean),
            wobble: l(self.wobble, to.wobble),
            wobble_hz: l(self.wobble_hz, to.wobble_hz),
            bob: l(self.bob, to.bob),
            bob_hz: l(self.bob_hz, to.bob_hz),
        }
    }
}

/// The shared state vocabulary.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Moods {
    pub unaware: Mood,
    pub surprised: Mood,
    pub searching: Mood,
    pub chasing: Mood,
    pub defending: Mood,
    pub windup: Mood,
    pub strike: Mood,
    pub recover: Mood,
    pub dizzy: Mood,
    pub prone: Mood,
    pub fleeing: Mood,
    pub dodging: Mood,
}

impl Moods {
    pub fn of(&self, state: State) -> Mood {
        match state {
            State::Unaware => self.unaware,
            State::Surprised => self.surprised,
            State::Searching => self.searching,
            State::Chasing => self.chasing,
            State::Defending => self.defending,
            State::Attacking(AttackPhase::Windup) => self.windup,
            State::Attacking(AttackPhase::Strike) => self.strike,
            State::Attacking(AttackPhase::Recover) => self.recover,
            State::Dizzy => self.dizzy,
            State::Prone => self.prone,
            State::Fleeing => self.fleeing,
            State::Dodging => self.dodging,
        }
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct BlobFace {
    pub eyes: EyeSet,
    pub white: Rgb,
    pub pupil: Rgb,
    pub idle: Mood,
    /// Gripping: small and squinting.
    pub shrunk: Mood,
    /// Just hurt: a wince.
    pub hurt: Mood,
    /// Right after a pogo or a lunge hit: wide eyes.
    pub wide: Mood,
    pub wide_time: f32,
    /// The eyes lag the body on a spring, so they slosh in the goo.
    pub slosh_stiffness: f32,
    pub slosh_damping: f32,
    /// Furthest the eyes slosh from their place (fraction of the blob radius).
    pub slosh_max: f32,
    /// The eyes shift this far toward where they look (fraction of the blob
    /// radius), which is how hollow eyes show a gaze.
    pub gaze_shift: f32,
    /// A dark ring around each eye (px), so eyes that slosh past the edge
    /// of the body still read.
    pub border: f32,
    pub border_color: Rgb,
    /// Looks at the nearest enemy this close (metres).
    pub glance_range: f32,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct DarkLook {
    /// Opacity of the dark over the dark room.
    pub alpha: f32,
    pub color: Rgb,
    /// Glow around each eye: radius (multiple of the eye's smaller side) and opacity.
    pub glow_radius: f32,
    pub glow_alpha: f32,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct FaceLook {
    /// How fast a face and body blend into a new state (per second).
    pub blend: f32,
    /// Average seconds between blinks, and how long a blink lasts.
    pub blink_every: f32,
    pub blink_time: f32,
    /// Enemy eyes: a dark socket with a pale pupil, red when attacking.
    pub socket: Rgb,
    pub glint: Rgb,
    pub danger: Rgb,
    /// Body outline width, px.
    pub outline_width: f32,
    /// Searching: how fast the pupils sweep (Hz).
    pub sweep_hz: f32,
    /// Dizzy: how fast the pupils spin (turns per second).
    pub spin_hz: f32,
    /// Prone: width of the X strokes (fraction of the eye width).
    pub x_width: f32,
    /// Surprised: an expanding ring around the head.
    pub ping_time: f32,
    pub ping_radius: f32,
    pub ping_width: f32,
    /// Dizzy: dots circling the head.
    pub dizzy_dots: u32,
    pub dizzy_dot_radius: f32,
    pub dizzy_hz: f32,
    pub moods: Moods,
    pub blob: BlobFace,
    pub dummy: EyeSet,
    pub grub: EyeSet,
    pub guard: EyeSet,
    pub spitter: EyeSet,
    pub skitter: EyeSet,
    pub brute: EyeSet,
    pub dark: DarkLook,
}

impl FaceLook {
    pub fn eyes(&self, kind: EnemyKind) -> &EyeSet {
        match kind {
            EnemyKind::Dummy => &self.dummy,
            EnemyKind::Grub => &self.grub,
            EnemyKind::Guard => &self.guard,
            EnemyKind::Spitter => &self.spitter,
            EnemyKind::Skitter => &self.skitter,
            EnemyKind::Brute => &self.brute,
        }
    }
}

// ---------------------------------------------------------------- pose

/// Squash, stretch and lean, anchored at the feet so a body stays on the ground.
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub feet: Vec2,
    sx: f32,
    sy: f32,
    shear: f32,
}

impl Pose {
    /// `phase` offsets the sway and breathing so neighbours don't move in step.
    pub fn new(feet: Vec2, mood: &Mood, facing: f32, now: f32, phase: f32) -> Self {
        let breathe = 1.0 + mood.bob * (now * mood.bob_hz * TAU + phase).sin();
        let sy = (mood.stretch * breathe).max(0.05);
        let sway = mood.wobble * (now * mood.wobble_hz * TAU + phase * 1.7).sin();
        Self {
            feet,
            sx: 1.0 / sy.sqrt(),
            sy,
            shear: mood.lean * facing + sway,
        }
    }

    pub fn apply(&self, p: Vec2) -> Vec2 {
        let d = p - self.feet;
        self.feet + Vec2::new(d.x * self.sx + d.y * self.shear, d.y * self.sy)
    }
}

/// The polygon pushed out by `width` around its centroid, for an outline.
pub fn grow(points: &[Vec2], width: f32) -> Vec<Vec2> {
    let c = points.iter().copied().sum::<Vec2>() / points.len().max(1) as f32;
    points
        .iter()
        .map(|p| *p + (*p - c).normalize_or_zero() * width)
        .collect()
}

// ---------------------------------------------------------------- eyes

/// Where each eye of a set sits, relative to the group centre, in units of
/// the body radius (x toward the facing side).
pub fn eye_offsets(set: &EyeSet) -> Vec<Vec2> {
    let n = set.count as usize;
    (0..n)
        .map(|i| match set.layout {
            Layout::Row => Vec2::X * (i as f32 - (n as f32 - 1.0) * 0.5) * set.spacing,
            Layout::Cluster if n == 1 => Vec2::ZERO,
            Layout::Cluster => {
                Vec2::from_angle(TAU * 0.25 + i as f32 / n as f32 * TAU) * set.spacing
            }
        })
        .collect()
}

/// A 0..1 blink amount (1 = shut) that comes every `every` seconds or so,
/// at a different moment for each `seed`.
pub fn blink(now: f32, seed: f32, every: f32, length: f32) -> f32 {
    if every <= 0.0 || length <= 0.0 {
        return 0.0;
    }
    // Vary the interval a little per blink so it doesn't feel clockwork.
    let cycle = now / every + seed;
    let n = cycle.floor();
    let jitter = hash(n + seed * 7.13) * 0.5;
    let t = (cycle - n - jitter).rem_euclid(1.0) * every;
    if t < length {
        (t / length * std::f32::consts::PI).sin()
    } else {
        0.0
    }
}

/// A cheap 0..1 hash.
pub fn hash(x: f32) -> f32 {
    ((x * 12.9898).sin() * 43758.547).fract().abs()
}

/// What to draw for one eye.
pub struct Eye {
    pub center: Vec2,
    /// Full width and height, px.
    pub size: Vec2,
    /// 0 shut .. 1 open.
    pub open: f32,
    /// Lid tilt (radians), already signed for this eye.
    pub slant: f32,
    /// Where it looks: a direction, length 0..1.
    pub gaze: Vec2,
    /// Pupil diameter, px.
    pub pupil: f32,
    pub white: Color,
    pub iris: Color,
    pub lid: Color,
    /// Knocked out: an X instead of an eye.
    pub crossed: bool,
    pub x_width: f32,
    /// A ring around the eye, px (0 = none), so it reads even off the body.
    pub border: f32,
    pub border_color: Color,
}

const EYE_SEGMENTS: usize = 16;

fn ellipse_points(center: Vec2, radii: Vec2) -> Vec<Vec2> {
    (0..EYE_SEGMENTS)
        .map(|k| center + Vec2::from_angle(k as f32 / EYE_SEGMENTS as f32 * TAU) * radii)
        .collect()
}

/// The part of a convex polygon on the side of the line through `q` that
/// `n` points to.
pub fn clip(points: &[Vec2], q: Vec2, n: Vec2) -> Vec<Vec2> {
    let inside = |p: Vec2| (p - q).dot(n) >= 0.0;
    let mut out = Vec::with_capacity(points.len() + 2);
    for (i, &a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        let (ia, ib) = (inside(a), inside(b));
        if ia {
            out.push(a);
        }
        if ia != ib {
            let da = (a - q).dot(n);
            let db = (b - q).dot(n);
            out.push(a + (b - a) * (da / (da - db)));
        }
    }
    out
}

pub fn draw_eye(fill: &mut Fill, eye: &Eye) {
    let r = eye.size * 0.5;
    if eye.crossed {
        let w = eye.size.x * eye.x_width;
        let (a, b) = (Vec2::new(r.x, r.y), Vec2::new(r.x, -r.y));
        if eye.border > 0.0 {
            let (ea, eb) = (
                a + a.normalize() * eye.border,
                b + b.normalize() * eye.border,
            );
            let wb = w + eye.border * 2.0;
            fill.segment(eye.center - ea, eye.center + ea, wb, eye.border_color);
            fill.segment(eye.center - eb, eye.center + eb, wb, eye.border_color);
        }
        fill.segment(eye.center - a, eye.center + a, w, eye.white);
        fill.segment(eye.center - b, eye.center + b, w, eye.white);
        return;
    }
    if eye.border > 0.0 {
        fill.polygon(
            &ellipse_points(eye.center, r + Vec2::splat(eye.border)),
            eye.border_color,
        );
    }
    let outline = ellipse_points(eye.center, r);
    fill.polygon(&outline, eye.white);
    // The pupil stays inside the eye. No pupil makes a hollow eye.
    if eye.pupil > 0.0 {
        let pr = (eye.pupil * 0.5).min(r.x).min(r.y);
        let room = (r - Vec2::splat(pr)).max(Vec2::ZERO);
        let gaze = eye.gaze.clamp_length_max(1.0);
        fill.circle(eye.center + gaze * room, pr, 10, eye.iris);
    }
    // The lid covers the eye from the top down to a tilted line.
    let open = eye.open.clamp(0.0, 1.0);
    if open < 0.999 {
        let q = eye.center + Vec2::Y * (r.y - (1.0 - open) * eye.size.y);
        let n = Vec2::from_angle(eye.slant).rotate(Vec2::Y);
        let lid = clip(&outline, q, n);
        fill.polygon(&lid, eye.lid);
    }
}

/// A soft halo behind an eye, for the dark.
pub fn draw_glow(fill: &mut Fill, center: Vec2, radius: f32, color: Color, alpha: f32) {
    fill.glow(
        center,
        radius,
        16,
        color.with_alpha(alpha),
        color.with_alpha(0.0),
    );
}

/// A ring of `width`, drawn as segments.
pub fn ring(fill: &mut Fill, center: Vec2, radius: f32, width: f32, color: Color) {
    let n = 20;
    let points: Vec<Vec2> = (0..=n)
        .map(|k| center + Vec2::from_angle(k as f32 / n as f32 * TAU) * radius)
        .collect();
    fill.polyline(&points, width, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_keeps_the_upper_half() {
        let square = [
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, -1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, 1.0),
        ];
        let top = clip(&square, Vec2::ZERO, Vec2::Y);
        assert_eq!(top.len(), 4);
        assert!(top.iter().all(|p| p.y >= -1e-6));
    }

    #[test]
    fn pose_keeps_the_feet_still() {
        let mood = Mood {
            open: 1.0,
            scale: 1.0,
            pupil: 1.0,
            stretch: 1.4,
            lean: 0.3,
            ..default()
        };
        let feet = Vec2::new(10.0, 5.0);
        let pose = Pose::new(feet, &mood, 1.0, 0.0, 0.0);
        assert!(pose.apply(feet).distance(feet) < 1e-4);
        // Stretched up and leaning toward the facing side.
        let top = pose.apply(feet + Vec2::Y * 10.0);
        assert!(top.y > feet.y + 13.0);
        assert!(top.x > feet.x);
    }

    #[test]
    fn rows_are_centred() {
        let set = EyeSet {
            count: 3,
            layout: Layout::Row,
            size: (0.2, 0.2),
            spacing: 0.5,
            offset: (0.0, 0.0),
            pupil: 0.5,
            stalk: 0.0,
            brow: 0.0,
        };
        let sum: Vec2 = eye_offsets(&set).iter().sum();
        assert!(sum.length() < 1e-5);
    }

    #[test]
    fn blinks_are_brief() {
        let shut = (0..10_000)
            .filter(|k| blink(*k as f32 * 0.001, 0.3, 3.0, 0.12) > 0.5)
            .count();
        // About a third of the blink length per 10 s, give or take the jitter.
        assert!(shut > 0 && shut < 1_000, "{shut}");
    }
}

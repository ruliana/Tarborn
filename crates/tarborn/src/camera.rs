//! The camera rig: follow with look-ahead, zoom out at speed, zoom in while
//! gripping, and screen shake on impacts.
//!
//! Everything here is plain math with no Bevy systems, so it is unit tested
//! headlessly. Every number lives in [`CameraParams`], which loads from
//! `camera.ron` and reloads while the game runs (see `main.rs`).
//!
//! Smoothness: zoom and look-ahead move with a critically damped spring
//! ([`smooth_damp`]), which starts and stops with zero velocity, so nothing
//! snaps. On top of that every zoom channel has a hard cap on how fast it may
//! change, so even a bad tuning can't make the view lurch.

use bevy::math::Vec2;
use serde::Deserialize;
use tarborn_sim::{Event, HitKind};

/// All camera tuning. Missing fields in `camera.ron` keep these defaults.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct CameraParams {
    /// Master switch for look-ahead, speed zoom, grip zoom and shake. Plain
    /// follow stays on. The C key flips it in game, for A/B comparison.
    pub effects: bool,
    pub follow: FollowParams,
    pub look_ahead: LookAheadParams,
    pub zoom: ZoomParams,
    pub shake: ShakeParams,
    pub trauma: TraumaParams,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct FollowParams {
    /// How fast the camera closes the gap to its target horizontally, per
    /// second (exponential: 1 - e^(-rate * dt) of the gap each frame).
    pub rate_x: f32,
    /// The same vertically. Lower than horizontal keeps hops from bobbing
    /// the whole screen.
    pub rate_y: f32,
    /// Exponential follow trails a moving target by speed / rate. This
    /// fraction of that trail is cancelled (0 = classic lag, 1 = none), so
    /// at high speed the blob doesn't drift toward the trailing edge.
    pub lag_compensation: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LookAheadParams {
    /// Lead the blob by where it will be this many seconds from now.
    pub seconds: f32,
    /// Fraction of the vertical lead kept (vertical lead is often unwelcome).
    pub vertical: f32,
    /// Longest lead, metres, per axis.
    pub max_x: f32,
    pub max_y: f32,
    /// Below this speed (m/s) there is no lead at all...
    pub min_speed: f32,
    /// ...ramping to the full lead at this speed.
    pub full_speed: f32,
    /// Spring time (seconds) for the lead to settle on a new value. Bigger
    /// is lazier; a bounce reversing direction swings the view gently.
    pub smooth_time: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ZoomParams {
    /// Base zoom (orthographic scale: bigger shows more) per room.
    pub arena: f32,
    pub training: f32,

    /// Speed is measured from the blob's velocity, smoothed so brief spikes
    /// (a lunge lasts 0.16 s) don't pump the zoom. Rate per second while
    /// speed rises...
    pub speed_rise_rate: f32,
    /// ...and while it falls.
    pub speed_fall_rate: f32,
    /// No speed zoom below this smoothed speed (m/s)...
    pub speed_start: f32,
    /// ...reaching the full zoom-out at this one.
    pub speed_full: f32,
    /// Full speed zoom-out as a fraction of the base: 0.25 shows 25% more.
    pub speed_out: f32,
    /// Shape of the ramp between start and full: 1 is linear, 2 eases in.
    pub speed_curve: f32,
    /// After slowing down, keep the wide view this long before easing back,
    /// so stop-and-go doesn't breathe in and out.
    pub speed_hold: f32,
    /// Spring times (seconds) for the speed zoom going out and coming back.
    pub speed_out_time: f32,
    pub speed_in_time: f32,

    /// Gripping zooms in by this fraction of the base: 0.08 shows 8% less.
    pub grip_in: f32,
    /// Grip has to be held this long before the zoom starts, so quick taps
    /// (dodges, bursts) don't twitch the view.
    pub grip_delay: f32,
    /// Spring times (seconds) for the grip zoom going in and letting go.
    pub grip_in_time: f32,
    pub grip_out_time: f32,

    /// Hard cap on how fast any zoom channel may change, as a fraction of
    /// the base per second. The anti-snap guarantee.
    pub max_rate: f32,
    /// The final zoom factor (speed and grip combined) stays in this range.
    pub min_factor: f32,
    pub max_factor: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ShakeParams {
    /// Overall multiplier on shake and kick (0 turns them off).
    pub scale: f32,
    /// Trauma lost per second. Trauma is 0..1 and drives the shake.
    pub decay: f32,
    /// Shake strength is trauma raised to this power: 2 or 3 makes small
    /// hits subtle and big ones violent.
    pub exponent: f32,
    /// Largest shake offset, in screen pixels.
    pub max_offset: f32,
    /// Largest shake rotation, degrees.
    pub max_angle: f32,
    /// How fast the shake wiggles (noise samples per second).
    pub frequency: f32,
    /// Directional kick: a spring that pushes the view along the impact and
    /// springs back. Natural frequency, Hz.
    pub kick_frequency: f32,
    /// Kick spring damping ratio: 1 returns without overshoot, lower wobbles.
    pub kick_damping: f32,
    /// Largest kick offset, screen pixels.
    pub kick_max: f32,
}

/// How much trauma (0..1) each event adds, and how far a kick pushes
/// (screen pixels). Events away from the player fade with distance.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TraumaParams {
    pub hit_lunge: f32,
    pub hit_slam: f32,
    pub hit_shockwave: f32,
    pub hit_pogo: f32,
    pub hit_impact: f32,
    /// Extra on top of the hit, for a kill.
    pub kill: f32,
    /// A slam landing: base plus this per m/s of landing speed.
    pub slam: f32,
    pub slam_per_speed: f32,
    /// Shield block.
    pub blocked: f32,
    /// An enemy hurt by something else (thrown body, spit, swing).
    pub struck: f32,
    /// A brute's ground slam.
    pub quake: f32,
    pub player_hurt: f32,
    pub player_died: f32,
    pub thrown: f32,
    pub grabbed: f32,
    pub bounced: f32,
    pub launched: f32,
    /// Events happening at the player count fully within `near` metres and
    /// fade to nothing at `far`.
    pub near: f32,
    pub far: f32,
    /// Kick for player hits, along the hit direction.
    pub kick_hit: f32,
    /// Kick for a slam landing, downward, plus this per m/s.
    pub kick_slam: f32,
    pub kick_slam_per_speed: f32,
    /// Kick when the player gets hurt.
    pub kick_hurt: f32,
}

impl Default for CameraParams {
    fn default() -> Self {
        Self {
            effects: true,
            follow: FollowParams::default(),
            look_ahead: LookAheadParams::default(),
            zoom: ZoomParams::default(),
            shake: ShakeParams::default(),
            trauma: TraumaParams::default(),
        }
    }
}

impl Default for FollowParams {
    fn default() -> Self {
        Self {
            rate_x: 6.0,
            rate_y: 4.5,
            lag_compensation: 0.8,
        }
    }
}

impl Default for LookAheadParams {
    fn default() -> Self {
        Self {
            seconds: 0.25,
            vertical: 0.35,
            max_x: 3.0,
            max_y: 1.5,
            min_speed: 3.0,
            full_speed: 10.0,
            smooth_time: 0.45,
        }
    }
}

impl Default for ZoomParams {
    fn default() -> Self {
        Self {
            arena: 1.6,
            training: 1.0,
            speed_rise_rate: 4.0,
            speed_fall_rate: 2.0,
            speed_start: 8.0,
            speed_full: 20.0,
            speed_out: 0.2,
            speed_curve: 1.5,
            speed_hold: 0.6,
            speed_out_time: 0.5,
            speed_in_time: 0.9,
            grip_in: 0.07,
            grip_delay: 0.2,
            grip_in_time: 0.45,
            grip_out_time: 0.35,
            max_rate: 0.5,
            min_factor: 0.8,
            max_factor: 1.4,
        }
    }
}

impl Default for ShakeParams {
    fn default() -> Self {
        Self {
            scale: 1.0,
            decay: 1.6,
            exponent: 2.0,
            max_offset: 14.0,
            max_angle: 1.2,
            frequency: 22.0,
            kick_frequency: 4.0,
            kick_damping: 0.6,
            kick_max: 18.0,
        }
    }
}

impl Default for TraumaParams {
    fn default() -> Self {
        Self {
            hit_lunge: 0.3,
            hit_slam: 0.45,
            hit_shockwave: 0.2,
            hit_pogo: 0.2,
            hit_impact: 0.25,
            kill: 0.15,
            slam: 0.25,
            slam_per_speed: 0.01,
            blocked: 0.2,
            struck: 0.2,
            quake: 0.5,
            player_hurt: 0.5,
            player_died: 0.7,
            thrown: 0.08,
            grabbed: 0.1,
            bounced: 0.0,
            launched: 0.05,
            near: 4.0,
            far: 16.0,
            kick_hit: 6.0,
            kick_slam: 6.0,
            kick_slam_per_speed: 0.25,
            kick_hurt: 8.0,
        }
    }
}

/// What the rig needs to know about the player each frame.
#[derive(Clone, Copy, Debug)]
pub struct Subject {
    /// Blob centre, metres.
    pub position: Vec2,
    /// Blob velocity, m/s.
    pub velocity: Vec2,
    /// Grip is held (the blob is shrunk).
    pub gripping: bool,
    /// The room's base zoom.
    pub base_zoom: f32,
}

/// Where to put the camera this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// Camera centre, metres (shake not included).
    pub center: Vec2,
    /// Shake and kick offset, screen pixels (multiply by `zoom` for world pixels).
    pub shake: Vec2,
    /// Roll, radians.
    pub angle: f32,
    /// Orthographic scale.
    pub zoom: f32,
}

/// The camera's moving state.
#[derive(Clone, Debug, Default)]
pub struct CameraRig {
    center: Vec2,
    lead: Vec2,
    lead_velocity: Vec2,
    speed: f32,
    speed_target: f32,
    speed_held_for: f32,
    /// Speed zoom-out amount (fraction of base) and its spring velocity.
    speed_zoom: f32,
    speed_zoom_velocity: f32,
    grip_for: f32,
    grip_zoom: f32,
    grip_zoom_velocity: f32,
    trauma: f32,
    kick: Vec2,
    kick_velocity: Vec2,
    time: f32,
}

impl CameraRig {
    /// A rig resting on `position`.
    pub fn at(position: Vec2) -> Self {
        Self {
            center: position,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub fn trauma(&self) -> f32 {
        self.trauma
    }

    /// Adds trauma (clamped to 1).
    pub fn add_trauma(&mut self, amount: f32) {
        self.trauma = (self.trauma + amount.max(0.0)).min(1.0);
    }

    /// Pushes the view `pixels` along `direction`, springing back after.
    pub fn kick(&mut self, direction: Vec2, pixels: f32, params: &ShakeParams) {
        let Some(dir) = direction.try_normalize() else {
            return;
        };
        // For a spring released at rest with velocity v, the first peak is
        // roughly v / omega (damping trims it), so this lands near `pixels`.
        let omega = std::f32::consts::TAU * params.kick_frequency.max(0.01);
        self.kick_velocity += dir * pixels * omega;
    }

    /// Feeds one simulation event in. `player` is the blob centre, metres.
    pub fn react(&mut self, event: &Event, player: Vec2, params: &CameraParams) {
        let t = &params.trauma;
        let fade = |at: Vec2| {
            let d = at.distance(player);
            if d <= t.near {
                1.0
            } else if d >= t.far || t.far <= t.near {
                0.0
            } else {
                1.0 - (d - t.near) / (t.far - t.near)
            }
        };
        match *event {
            Event::Hit { kind, at, .. } => {
                let amount = match kind {
                    HitKind::Lunge => t.hit_lunge,
                    HitKind::Slam => t.hit_slam,
                    HitKind::Shockwave => t.hit_shockwave,
                    HitKind::Pogo => t.hit_pogo,
                    HitKind::Impact => t.hit_impact,
                };
                self.add_trauma(amount);
                self.kick(at - player, t.kick_hit, &params.shake);
            }
            Event::Killed { at, .. } => self.add_trauma(t.kill * fade(at)),
            Event::Slammed { at, speed } => {
                self.add_trauma((t.slam + t.slam_per_speed * speed) * fade(at));
                let push = t.kick_slam + t.kick_slam_per_speed * speed;
                self.kick(Vec2::NEG_Y, push, &params.shake);
            }
            Event::Blocked { at, .. } => self.add_trauma(t.blocked * fade(at)),
            Event::Struck { at, .. } => self.add_trauma(t.struck * fade(at)),
            Event::Quake { at, .. } => self.add_trauma(t.quake * fade(at)),
            Event::PlayerHurt { .. } => {
                self.add_trauma(t.player_hurt);
                // No direction is reported, so push down: it reads as a blow.
                self.kick(Vec2::NEG_Y, t.kick_hurt, &params.shake);
            }
            Event::PlayerDied => self.add_trauma(t.player_died),
            Event::Thrown { .. } => self.add_trauma(t.thrown),
            Event::Grabbed { .. } => self.add_trauma(t.grabbed),
            Event::Bounced { .. } => self.add_trauma(t.bounced),
            Event::Launched { power, .. } => self.add_trauma(t.launched * (1.0 + power)),
            _ => {}
        }
    }

    /// Advances the rig `dt` seconds and returns the pose to draw with.
    pub fn update(&mut self, subject: Subject, params: &CameraParams, dt: f32) -> Pose {
        self.time += dt;
        let effects = params.effects;
        let speed_now = subject.velocity.length();

        // Smoothed speed, quicker up than down.
        let rate = if speed_now > self.speed {
            params.zoom.speed_rise_rate
        } else {
            params.zoom.speed_fall_rate
        };
        self.speed += (speed_now - self.speed) * approach(rate, dt);

        // Look-ahead lead.
        let la = &params.look_ahead;
        let lead_target = if effects {
            let ramp = ramp(self.speed, la.min_speed, la.full_speed);
            let raw = subject.velocity * la.seconds * Vec2::new(1.0, la.vertical) * ramp;
            let lead = raw.clamp(
                Vec2::new(-la.max_x, -la.max_y),
                Vec2::new(la.max_x, la.max_y),
            );
            let f = &params.follow;
            let trail = Vec2::new(
                subject.velocity.x / f.rate_x.max(0.01),
                subject.velocity.y / f.rate_y.max(0.01),
            );
            lead + trail * f.lag_compensation
        } else {
            Vec2::ZERO
        };
        self.lead = Vec2::new(
            smooth_damp(
                self.lead.x,
                lead_target.x,
                &mut self.lead_velocity.x,
                la.smooth_time,
                f32::INFINITY,
                dt,
            ),
            smooth_damp(
                self.lead.y,
                lead_target.y,
                &mut self.lead_velocity.y,
                la.smooth_time,
                f32::INFINITY,
                dt,
            ),
        );

        // Follow.
        let target = subject.position + self.lead;
        let f = &params.follow;
        self.center.x += (target.x - self.center.x) * approach(f.rate_x, dt);
        self.center.y += (target.y - self.center.y) * approach(f.rate_y, dt);

        // Speed zoom, with a hold before it eases back.
        let z = &params.zoom;
        let wanted = if effects {
            ramp(self.speed, z.speed_start, z.speed_full).powf(z.speed_curve.max(0.01))
                * z.speed_out
        } else {
            0.0
        };
        if wanted >= self.speed_target {
            self.speed_target = wanted;
            self.speed_held_for = 0.0;
        } else {
            self.speed_held_for += dt;
            if self.speed_held_for >= z.speed_hold {
                self.speed_target = wanted;
            }
        }
        let time = if self.speed_target > self.speed_zoom {
            z.speed_out_time
        } else {
            z.speed_in_time
        };
        self.speed_zoom = smooth_damp(
            self.speed_zoom,
            self.speed_target,
            &mut self.speed_zoom_velocity,
            time,
            z.max_rate,
            dt,
        );

        // Grip zoom, after a short delay.
        self.grip_for = if subject.gripping {
            self.grip_for + dt
        } else {
            0.0
        };
        let grip_target = if effects && self.grip_for >= z.grip_delay {
            z.grip_in
        } else {
            0.0
        };
        let time = if grip_target > self.grip_zoom {
            z.grip_in_time
        } else {
            z.grip_out_time
        };
        self.grip_zoom = smooth_damp(
            self.grip_zoom,
            grip_target,
            &mut self.grip_zoom_velocity,
            time,
            z.max_rate,
            dt,
        );
        let factor = ((1.0 + self.speed_zoom) * (1.0 - self.grip_zoom)).clamp(
            z.min_factor.min(z.max_factor),
            z.max_factor.max(z.min_factor),
        );

        // Shake: trauma-driven noise, plus a directional kick spring.
        let s = &params.shake;
        self.trauma = (self.trauma - s.decay * dt).max(0.0);
        let omega = std::f32::consts::TAU * s.kick_frequency.max(0.01);
        // Semi-implicit Euler is stable here for any sane frame time; split
        // long frames so a hitch can't blow the spring up.
        let steps = (dt * omega / 0.5).ceil().max(1.0) as u32;
        let h = dt / steps as f32;
        for _ in 0..steps {
            let accel =
                -omega * omega * self.kick - 2.0 * s.kick_damping * omega * self.kick_velocity;
            self.kick_velocity += accel * h;
            self.kick += self.kick_velocity * h;
        }
        self.kick = self.kick.clamp_length_max(s.kick_max);
        let (shake, angle) = if effects {
            let amount = self.trauma.powf(s.exponent.max(0.01)) * s.scale;
            let t = self.time * s.frequency;
            let offset = Vec2::new(noise(t, 1), noise(t, 2)) * s.max_offset * amount;
            let angle = s.max_angle.to_radians() * amount * noise(t, 3);
            (offset + self.kick * s.scale, angle)
        } else {
            (Vec2::ZERO, 0.0)
        };

        Pose {
            center: self.center,
            shake,
            angle,
            zoom: subject.base_zoom * factor,
        }
    }
}

/// Fraction of a gap closed this frame by exponential smoothing at `rate`/s.
fn approach(rate: f32, dt: f32) -> f32 {
    1.0 - (-rate.max(0.0) * dt).exp()
}

/// 0 at or below `lo`, 1 at or above `hi`, linear between.
fn ramp(x: f32, lo: f32, hi: f32) -> f32 {
    if hi <= lo {
        return if x >= hi { 1.0 } else { 0.0 };
    }
    ((x - lo) / (hi - lo)).clamp(0.0, 1.0)
}

/// Critically damped spring toward `target` (Game Programming Gems 4,
/// "Critically Damped Ease-In/Ease-Out Smoothing"). Settles in about
/// `smooth_time` seconds, never overshoots, and never moves faster than
/// `max_rate` per second.
pub fn smooth_damp(
    current: f32,
    target: f32,
    velocity: &mut f32,
    smooth_time: f32,
    max_rate: f32,
    dt: f32,
) -> f32 {
    if dt <= 0.0 {
        return current;
    }
    let smooth_time = smooth_time.max(1e-4);
    let omega = 2.0 / smooth_time;
    let x = omega * dt;
    let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let max_change = max_rate * smooth_time;
    let change = (current - target).clamp(-max_change, max_change);
    let goal = current - change;
    let temp = (*velocity + omega * change) * dt;
    *velocity = (*velocity - omega * temp) * exp;
    let mut out = goal + (change + temp) * exp;
    // Don't overshoot the real target.
    if (target - current > 0.0) == (out > target) {
        out = target;
        *velocity = 0.0;
    }
    // Enforce the rate cap on the actual step too.
    let limit = max_rate * dt;
    if (out - current).abs() > limit {
        out = current + (out - current).signum() * limit;
        *velocity = (out - current) / dt;
    }
    out
}

/// Smooth 1D value noise in -1..1, one independent stream per `seed`.
fn noise(t: f32, seed: u32) -> f32 {
    let i = t.floor();
    let f = t - i;
    let a = lattice(i as i32, seed);
    let b = lattice(i as i32 + 1, seed);
    let s = f * f * (3.0 - 2.0 * f);
    a + (b - a) * s
}

fn lattice(i: i32, seed: u32) -> f32 {
    let mut h = (i as u32).wrapping_mul(0x9E37_79B9) ^ seed.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    (h as f32 / u32::MAX as f32) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f32 = 1.0 / 60.0;

    fn subject(velocity: Vec2, gripping: bool) -> Subject {
        Subject {
            position: Vec2::ZERO,
            velocity,
            gripping,
            base_zoom: 1.0,
        }
    }

    fn run(rig: &mut CameraRig, s: Subject, p: &CameraParams, seconds: f32, dt: f32) -> Vec<Pose> {
        (0..(seconds / dt).round() as usize)
            .map(|_| rig.update(s, p, dt))
            .collect()
    }

    #[test]
    fn shipped_camera_file_parses_and_matches_defaults() {
        let text = include_str!("../../../camera.ron");
        let parsed: CameraParams = ron::from_str(text).expect("camera.ron parses");
        assert_eq!(parsed, CameraParams::default());
    }

    #[test]
    fn partial_file_keeps_defaults() {
        let parsed: CameraParams = ron::from_str("(shake: (scale: 0.5))").unwrap();
        assert_eq!(parsed.shake.scale, 0.5);
        assert_eq!(parsed.shake.decay, ShakeParams::default().decay);
        assert_eq!(parsed.zoom, ZoomParams::default());
    }

    #[test]
    fn standing_still_is_calm() {
        let p = CameraParams::default();
        let mut rig = CameraRig::at(Vec2::ZERO);
        let pose = *run(&mut rig, subject(Vec2::ZERO, false), &p, 2.0, FRAME)
            .last()
            .unwrap();
        assert_eq!(pose.zoom, 1.0);
        assert_eq!(pose.shake, Vec2::ZERO);
        assert!(pose.center.length() < 1e-4);
    }

    #[test]
    fn high_speed_zooms_out_smoothly_and_leads() {
        let p = CameraParams::default();
        let mut rig = CameraRig::at(Vec2::ZERO);
        let poses = run(
            &mut rig,
            subject(Vec2::new(25.0, 0.0), false),
            &p,
            3.0,
            FRAME,
        );
        let last = poses.last().unwrap();
        assert!(
            (last.zoom - (1.0 + p.zoom.speed_out)).abs() < 0.01,
            "{}",
            last.zoom
        );
        assert!(
            last.center.x > p.look_ahead.max_x * 0.9,
            "camera leads ahead"
        );
        // Never faster than the rate cap.
        for w in poses.windows(2) {
            assert!((w[1].zoom - w[0].zoom).abs() <= p.zoom.max_rate * FRAME + 1e-5);
        }
    }

    #[test]
    fn a_lone_lunge_barely_moves_the_zoom() {
        let p = CameraParams::default();
        let mut rig = CameraRig::at(Vec2::ZERO);
        // One 0.16 s lunge at 15 m/s, then rest.
        run(
            &mut rig,
            subject(Vec2::new(15.0, 0.0), false),
            &p,
            0.16,
            FRAME,
        );
        let poses = run(&mut rig, subject(Vec2::ZERO, false), &p, 2.0, FRAME);
        let peak = poses.iter().map(|p| p.zoom).fold(0.0, f32::max);
        assert!(peak < 1.02, "peak zoom {peak}");
    }

    #[test]
    fn slowing_down_holds_then_eases_back() {
        let p = CameraParams::default();
        let mut rig = CameraRig::at(Vec2::ZERO);
        run(
            &mut rig,
            subject(Vec2::new(25.0, 0.0), false),
            &p,
            3.0,
            FRAME,
        );
        let wide = rig.update(subject(Vec2::ZERO, false), &p, FRAME).zoom;
        let poses = run(&mut rig, subject(Vec2::ZERO, false), &p, 4.0, FRAME);
        assert!(
            poses[10].zoom > wide - 0.01,
            "holds wide right after stopping"
        );
        assert!((poses.last().unwrap().zoom - 1.0).abs() < 0.005);
    }

    #[test]
    fn grip_tap_does_nothing_but_holding_zooms_in() {
        let p = CameraParams::default();
        let mut rig = CameraRig::at(Vec2::ZERO);
        let tap = run(
            &mut rig,
            subject(Vec2::ZERO, true),
            &p,
            p.zoom.grip_delay * 0.5,
            FRAME,
        );
        assert!(tap.iter().all(|pose| pose.zoom == 1.0));
        let hold = run(&mut rig, subject(Vec2::ZERO, true), &p, 2.0, FRAME);
        assert!((hold.last().unwrap().zoom - (1.0 - p.zoom.grip_in)).abs() < 0.005);
        let release = run(&mut rig, subject(Vec2::ZERO, false), &p, 2.0, FRAME);
        assert!((release.last().unwrap().zoom - 1.0).abs() < 0.005);
    }

    #[test]
    fn zoom_starts_gently() {
        // A spring starting from rest: the first frames move much less than
        // the average, so there is no visible snap.
        let p = CameraParams::default();
        let mut rig = CameraRig::at(Vec2::ZERO);
        run(
            &mut rig,
            subject(Vec2::ZERO, true),
            &p,
            p.zoom.grip_delay,
            FRAME,
        );
        let poses = run(&mut rig, subject(Vec2::ZERO, true), &p, 0.5, FRAME);
        let first = 1.0 - poses[0].zoom;
        let avg = (1.0 - poses.last().unwrap().zoom) / poses.len() as f32;
        assert!(first < avg, "first {first} avg {avg}");
    }

    #[test]
    fn frame_rate_does_not_change_the_feel() {
        let p = CameraParams::default();
        let s = subject(Vec2::new(25.0, 4.0), true);
        let mut slow = CameraRig::at(Vec2::ZERO);
        let mut fast = CameraRig::at(Vec2::ZERO);
        let a = *run(&mut slow, s, &p, 1.0, 1.0 / 30.0).last().unwrap();
        let b = *run(&mut fast, s, &p, 1.0, 1.0 / 240.0).last().unwrap();
        assert!((a.zoom - b.zoom).abs() < 0.01, "{} vs {}", a.zoom, b.zoom);
        assert!(a.center.distance(b.center) < 0.1);
    }

    #[test]
    fn hits_shake_and_the_shake_dies_out() {
        let p = CameraParams::default();
        let mut rig = CameraRig::at(Vec2::ZERO);
        let hit = Event::Hit {
            enemy: 0,
            kind: HitKind::Slam,
            damage: 40.0,
            at: Vec2::new(1.0, 0.0),
        };
        rig.react(&hit, Vec2::ZERO, &p);
        assert!(rig.trauma() > 0.0);
        let poses = run(&mut rig, subject(Vec2::ZERO, false), &p, 0.2, FRAME);
        assert!(poses.iter().any(|pose| pose.shake.length() > 1.0));
        let poses = run(&mut rig, subject(Vec2::ZERO, false), &p, 3.0, FRAME);
        assert_eq!(rig.trauma(), 0.0);
        assert!(poses.last().unwrap().shake.length() < 0.1);
    }

    #[test]
    fn far_away_events_do_not_shake() {
        let p = CameraParams::default();
        let mut rig = CameraRig::at(Vec2::ZERO);
        let quake = Event::Quake {
            enemy: 0,
            at: Vec2::new(p.trauma.far + 1.0, 0.0),
        };
        rig.react(&quake, Vec2::ZERO, &p);
        assert_eq!(rig.trauma(), 0.0);
    }

    #[test]
    fn trauma_is_capped() {
        let p = CameraParams::default();
        let mut rig = CameraRig::at(Vec2::ZERO);
        for _ in 0..10 {
            rig.react(&Event::PlayerDied, Vec2::ZERO, &p);
        }
        assert_eq!(rig.trauma(), 1.0);
    }

    #[test]
    fn effects_off_means_plain_follow() {
        let p = CameraParams {
            effects: false,
            ..CameraParams::default()
        };
        let mut rig = CameraRig::at(Vec2::ZERO);
        rig.react(&Event::PlayerDied, Vec2::ZERO, &p);
        let poses = run(
            &mut rig,
            subject(Vec2::new(25.0, 0.0), true),
            &p,
            2.0,
            FRAME,
        );
        assert!(
            poses
                .iter()
                .all(|pose| pose.zoom == 1.0 && pose.angle == 0.0)
        );
        assert!(poses.iter().all(|pose| pose.shake == Vec2::ZERO));
    }

    #[test]
    fn smooth_damp_respects_rate_cap_and_lands() {
        let mut v = 0.0;
        let mut x = 0.0;
        for _ in 0..600 {
            let next = smooth_damp(x, 1.0, &mut v, 0.3, 0.5, FRAME);
            assert!(next - x <= 0.5 * FRAME + 1e-6);
            assert!(next <= 1.0);
            x = next;
        }
        assert!((x - 1.0).abs() < 1e-3);
    }
}

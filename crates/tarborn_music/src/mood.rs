//! How tense the moment is, from what the game sees: the director's
//! intensity input, and a danger level.
//!
//! The game describes the moment as a [`Scene`] (engine-free: enemy states
//! by name, health, windups, the combo). [`Mood`] turns it into:
//! - **intensity** (0..1): the strongest engaged enemy's state weight, plus
//!   a little per extra engaged enemy, at least `combo_floor` while a combo
//!   runs, plus some of the danger. It rises fast and falls slowly.
//! - **danger** (0..1): low health and windups aimed at the player.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The `mood` section of `music.ron`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MoodParams {
    /// Weight of each enemy state (by name, as the game reports it). A state
    /// left out weighs 0.
    pub states: BTreeMap<String, f32>,
    /// Added for each engaged enemy (weight above 0) past the first, up to
    /// `crowd_max` in all.
    pub crowd: f32,
    pub crowd_max: f32,
    /// Intensity kept while the player's combo runs.
    pub combo_floor: f32,
    /// Danger from health: `health_weight * (1 - health)`.
    pub health_weight: f32,
    /// Danger per enemy winding up an attack, up to 1 in all.
    pub windup_weight: f32,
    /// Share of the danger added to the intensity.
    pub danger_mix: f32,
    /// Seconds for the smoothed intensity to cover most of a rise, and of a
    /// fall.
    pub rise: f32,
    pub fall: f32,
}

impl Default for MoodParams {
    fn default() -> Self {
        let states = [
            ("searching", 0.35),
            ("surprised", 0.5),
            ("chasing", 0.6),
            ("defending", 0.6),
            ("windup", 0.9),
            ("strike", 1.0),
            ("recover", 0.85),
            ("dizzy", 0.8),
            ("prone", 0.8),
            ("dodging", 0.8),
            ("fleeing", 0.5),
        ];
        Self {
            states: states.iter().map(|&(k, v)| (k.to_string(), v)).collect(),
            crowd: 0.1,
            crowd_max: 0.3,
            combo_floor: 0.75,
            health_weight: 0.6,
            windup_weight: 0.3,
            danger_mix: 0.3,
            rise: 0.3,
            fall: 4.0,
        }
    }
}

/// What the game sees this tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    /// The state of each living enemy near enough to matter (on screen).
    pub enemies: Vec<&'static str>,
    /// Enemies near enough winding up an attack.
    pub windups: u32,
    /// Health left, 0..1.
    pub health: f32,
    /// Hits in the running combo.
    pub combo: u32,
    pub dead: bool,
}

/// The smoothed mood.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mood {
    pub intensity: f32,
    pub danger: f32,
    /// The unsmoothed intensity of the last scene.
    pub target: f32,
}

impl Mood {
    /// The intensity and danger a scene asks for, before smoothing.
    pub fn judge(scene: &Scene, p: &MoodParams) -> (f32, f32) {
        if scene.dead {
            return (0.0, 0.0);
        }
        let weight = |s: &str| p.states.get(s).copied().unwrap_or(0.0);
        let engaged: Vec<f32> = scene
            .enemies
            .iter()
            .map(|s| weight(s))
            .filter(|&w| w > 0.0)
            .collect();
        let strongest = engaged.iter().copied().fold(0.0, f32::max);
        let crowd = (engaged.len().saturating_sub(1) as f32 * p.crowd).min(p.crowd_max);
        let danger = (p.health_weight * (1.0 - scene.health.clamp(0.0, 1.0))
            + p.windup_weight * scene.windups as f32)
            .clamp(0.0, 1.0);
        let mut intensity = strongest + if strongest > 0.0 { crowd } else { 0.0 };
        if scene.combo > 0 {
            intensity = intensity.max(p.combo_floor);
        }
        intensity += p.danger_mix * danger;
        (intensity.clamp(0.0, 1.0), danger)
    }

    /// Follows a scene over `dt` seconds.
    pub fn update(&mut self, scene: &Scene, dt: f32, p: &MoodParams) {
        let (target, danger) = Self::judge(scene, p);
        let tau = if target > self.intensity {
            p.rise
        } else {
            p.fall
        };
        let k = 1.0 - (-dt / tau.max(1e-3)).exp();
        self.intensity += (target - self.intensity) * k;
        self.danger = danger;
        self.target = target;
    }
}

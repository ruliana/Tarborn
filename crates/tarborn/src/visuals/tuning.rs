//! Every number of the look, read from `visuals.ron` and re-read while the game runs.

use std::path::Path;
use std::time::SystemTime;

use bevy::prelude::*;
use serde::Deserialize;

use super::dressing::shape::ShapeParams;

/// The tuning file next to this crate's Cargo.toml. Its contents are also
/// compiled in, so the game still has a look when the file isn't there.
const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/visuals.ron");
const BUILT_IN: &str = include_str!("../../assets/visuals.ron");
/// Seconds between checks for an edited file.
const POLL: f32 = 0.5;

pub type Rgb = (f32, f32, f32);

pub fn srgb(c: Rgb) -> Color {
    Color::srgb(c.0, c.1, c.2)
}

pub fn linear(c: Rgb) -> Vec4 {
    let l = srgb(c).to_linear();
    Vec4::new(l.red, l.green, l.blue, 1.0)
}

#[derive(Resource, Deserialize, Clone, Debug, PartialEq)]
pub struct VisualTuning {
    pub level: LevelLook,
    pub dressing: DressingLook,
    pub tar: TarLook,
    pub enemy: EnemyLook,
    pub post: PostLook,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct LevelLook {
    pub hazard: Rgb,
    pub spike_width: f32,
    pub spike_height: f32,
}

/// How the level dressing reads the shape; the pieces come from the sets in
/// `assets/dressing/`.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct DressingLook {
    /// The set shown first (B cycles sets).
    pub default_set: String,
    pub seed: u64,
    pub shape: ShapeParams,
    /// Depths up to this many px are stored for the body shader.
    pub depth_max: f32,
    /// How far past the level's bounds the outside is filled in.
    pub surround: f32,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct TarLook {
    pub body: Rgb,
    pub highlight: Rgb,
    pub round: f32,
    pub bevel: f32,
    pub aa: f32,
    pub light: (f32, f32, f32),
    pub spec_power: f32,
    pub spec_strength: f32,
    pub rim_strength: f32,
    pub rim_power: f32,
    pub tint_body: f32,
    pub pickup_radius: f32,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct EnemyLook {
    pub fill: Rgb,
    pub open: Rgb,
    pub danger: Rgb,
    pub flash: Rgb,
    pub coat: Rgb,
    pub coat_width: f32,
    pub ink_max_lightness: f32,
    pub spit_radius_scale: f32,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct PostLook {
    pub grain: f32,
    pub grain_fps: f32,
    pub vignette: f32,
    pub vignette_start: f32,
    pub vignette_end: f32,
    pub dither_pixel: f32,
    pub dither_spread: f32,
    pub palette: [Rgb; 4],
    pub danger: Rgb,
    pub red_keep: f32,
}

impl Default for VisualTuning {
    fn default() -> Self {
        ron::from_str(BUILT_IN).expect("the built-in visuals.ron parses")
    }
}

/// Re-reads the tuning file when it changes. A broken edit is reported and
/// the last good tuning stays in place.
pub fn reload(
    time: Res<Time>,
    mut tuning: ResMut<VisualTuning>,
    mut since: Local<f32>,
    mut seen: Local<Option<SystemTime>>,
) {
    *since += time.delta_secs();
    if *since < POLL {
        return;
    }
    *since = 0.0;
    let Ok(modified) = std::fs::metadata(Path::new(PATH)).and_then(|m| m.modified()) else {
        return;
    };
    if *seen == Some(modified) {
        return;
    }
    let first = seen.is_none();
    *seen = Some(modified);
    let text = match std::fs::read_to_string(PATH) {
        Ok(text) => text,
        Err(err) => return warn!("visuals.ron: {err}"),
    };
    match ron::from_str::<VisualTuning>(&text) {
        Ok(new) if new != *tuning => {
            if !first {
                info!("visuals.ron reloaded");
            }
            *tuning = new;
        }
        Ok(_) => {}
        Err(err) => warn!("visuals.ron: {err}"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn built_in_tuning_parses() {
        let _ = super::VisualTuning::default();
    }
}

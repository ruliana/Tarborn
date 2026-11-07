//! The controls' tuning, read from `assets/feel.ron` and re-read while the
//! game runs. The simulation gets a fresh copy every tick, so a new room or
//! stage picks it up too.

use std::path::Path;
use std::time::SystemTime;

use bevy::prelude::*;
use tarborn_sim::FeelParams;

const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/feel.ron");
const BUILT_IN: &str = include_str!("../assets/feel.ron");
/// Seconds between checks for an edited file.
const POLL: f32 = 0.5;

#[derive(Resource)]
pub struct FeelTuning {
    pub params: FeelParams,
    modified: Option<SystemTime>,
    check_in: f32,
}

impl FeelTuning {
    pub fn load() -> Self {
        let mut tuning = Self {
            params: ron::from_str(BUILT_IN).expect("built-in feel.ron is valid"),
            modified: None,
            check_in: 0.0,
        };
        tuning.refresh();
        tuning
    }

    /// Re-reads the file if it changed. A broken file keeps the last good
    /// tuning, so a typo mid-edit never wrecks the controls.
    fn refresh(&mut self) {
        let Ok(modified) = std::fs::metadata(Path::new(PATH)).and_then(|m| m.modified()) else {
            return;
        };
        if self.modified == Some(modified) {
            return;
        }
        self.modified = Some(modified);
        match std::fs::read_to_string(PATH)
            .map_err(|e| e.to_string())
            .and_then(|text| ron::from_str::<FeelParams>(&text).map_err(|e| e.to_string()))
        {
            Ok(params) => {
                self.params = params;
                info!("controls tuning loaded from {PATH}");
            }
            Err(e) => warn!("{PATH}: {e}"),
        }
    }
}

pub fn reload(time: Res<Time>, mut tuning: ResMut<FeelTuning>) {
    tuning.check_in -= time.delta_secs();
    if tuning.check_in <= 0.0 {
        tuning.check_in = POLL;
        tuning.refresh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_file_matches_the_simulation_defaults() {
        let params: FeelParams = ron::from_str(BUILT_IN).unwrap();
        assert_eq!(params, FeelParams::default());
    }
}

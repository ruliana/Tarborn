//! Generative music for Tarborn, without an engine.
//!
//! Music is a library of short symbolic [`score::Fragment`]s (notes, not
//! audio) played through instrument [`render::Patch`]es built on the
//! `tarborn_sfx` synth. This step holds the note format and an offline
//! renderer that turns a fragment into samples (and WAV files through
//! [`tarborn_sfx::wav`]). Later steps add the jazz theory core, the grammar
//! proposer, the metrics, the quality-diversity search that fills the
//! library, and the live director.
//!
//! Every number that shapes the sound lives in RON data
//! (`crates/tarborn/assets/music.ron` and `assets/music/`); nothing is tuned
//! in code. Rendering is deterministic: the same fragment, patches, settings
//! and seed always produce the same samples.

pub mod analysis;
pub mod grammar;
pub mod patches;
pub mod render;
pub mod rng;
pub mod score;
pub mod stream;
pub mod style;
pub mod theory;
pub mod time;

pub use render::{Patch, PatchBank, RenderSettings, render};
pub use score::{Fragment, Note, Role};

use serde::Deserialize;

/// The music tuning file, `crates/tarborn/assets/music.ron`. Later steps add
/// their own sections; unknown sections are left for them.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct MusicFile {
    /// How fragments are rendered offline (previews, tests, the library).
    pub render: RenderSettings,
    /// The live stream and its sync with the sim.
    pub stream: stream::StreamParams,
    /// A debug click track on the sim's beat clock.
    pub click: ClickTrack,
}

/// A metronome on the sim's beat clock, to check that the stream keeps time.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ClickTrack {
    pub enabled: bool,
    /// Tempo of the beat clock until the director (step 8) sets it per stage.
    pub bpm: f32,
    pub beats_per_bar: u32,
    pub volume: f32,
    /// Played on the first beat of each bar.
    pub downbeat: tarborn_sfx::Sfx,
    /// Played on the other beats.
    pub beat: tarborn_sfx::Sfx,
}

impl Default for ClickTrack {
    fn default() -> Self {
        Self {
            enabled: false,
            bpm: 120.0,
            beats_per_bar: 4,
            volume: 0.5,
            downbeat: tarborn_sfx::Sfx::default(),
            beat: tarborn_sfx::Sfx::default(),
        }
    }
}

/// Loads a RON value with the same options every Tarborn data file uses
/// (`Option` fields may be written without `Some`).
pub fn from_ron<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, ron::error::SpannedError> {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(text)
}

/// Writes a RON value in the readable layout used for library files.
pub fn to_ron<T: serde::Serialize>(value: &T) -> Result<String, ron::Error> {
    ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default().struct_names(false))
}

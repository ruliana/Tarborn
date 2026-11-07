//! A stage's musical style, read from `assets/music/styles/<name>.ron`.
//!
//! The jazz core is shared; a style only sets numbers and choices: tempo,
//! swing, keys, which harmony grammar to use and how each part plays at
//! each intensity layer. A new stage style is a new file, not new code.
//!
//! Per-layer values are lists indexed by intensity layer (0 is the calmest);
//! a list shorter than the number of layers repeats its last entry.

use crate::theory::{Mode, SwingCurve, VoicingKind, VoicingRules};
use serde::{Deserialize, Serialize};

/// A value per intensity layer.
pub type PerLayer<T> = Vec<T>;

/// Reads a per-layer list, repeating the last entry past its end.
pub fn at<T: Copy + Default>(list: &[T], layer: u8) -> T {
    list.get(layer as usize)
        .or(list.last())
        .copied()
        .unwrap_or_default()
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Style {
    pub name: String,
    /// BPM range the tune's tempo is drawn from.
    pub tempo: [f32; 2],
    pub beats_per_bar: u32,
    pub swing: SwingCurve,
    /// Intensity layers the parts are generated for.
    pub layers: u8,
    /// Tonic note names to draw from ("C", "Bb", ...).
    pub keys: Vec<String>,
    pub harmony: Harmony,
    pub bass: BassStyle,
    pub comp: CompStyle,
    pub drums: DrumStyle,
    pub lead: LeadStyle,
    pub stinger: StingerStyle,
}

/// Which harmony grammar the style uses.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Harmony {
    /// Bebop and hard bop: forms built from ii-V chains.
    Functional(FunctionalHarmony),
    /// Modal jazz: one mode held for many bars.
    Modal(ModalHarmony),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FunctionalHarmony {
    /// Chance the tune is in a minor key.
    pub minor: f32,
    /// Weights of the 12-bar minor blues and the 32-bar AABA song form.
    pub blues_weight: f32,
    pub aaba_weight: f32,
    /// Chance a dominant becomes its tritone substitute.
    pub tritone_sub: f32,
    /// Chance a dominant resolving to a minor chord, or any dominant in a
    /// minor key, is played altered rather than 7b9.
    pub altered: f32,
    /// Chance a two-bar ii-V is squeezed into one bar (two beats each).
    pub fast_ii_v: f32,
    /// Chance a bar of tonic is replaced by a secondary ii-V into the next
    /// chord.
    pub secondary: f32,
    /// Bridge key, in semitones above the home key, with weights.
    pub bridge_keys: Vec<(i32, f32)>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModalHarmony {
    /// Modes the vamp can hold, with weights.
    pub modes: Vec<(Mode, f32)>,
    /// Bars the A vamp lasts.
    pub vamp_bars: Vec<u32>,
    /// Bars the bridge lasts, and how far (semitones) it shifts the mode.
    pub bridge_bars: Vec<u32>,
    pub bridge_shift: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BassStyle {
    pub patch: String,
    /// Lowest and highest MIDI pitch.
    pub range: [u8; 2],
    /// Per layer: how the bass plays.
    pub feel: PerLayer<BassFeel>,
    /// Chance the note before a chord change approaches the new root by a
    /// half step (else by a scale step).
    pub chromatic_approach: f32,
    /// Chance of a swung ghost note on the "and" of a walking beat.
    pub ghost: f32,
    /// Largest leap between two notes, in semitones.
    pub max_leap: u8,
    /// Velocity of beat 1, the other beats and ghosts.
    pub accent: f32,
    pub velocity: f32,
    pub ghost_velocity: f32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum BassFeel {
    /// Quarter notes.
    #[default]
    Walking,
    /// Half notes: root and fifth.
    TwoFeel,
    /// The root held, restruck each bar.
    Pedal,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CompStyle {
    /// Per layer: the patches each hit sounds on (empty: no comping).
    pub patches: PerLayer<Vec<String>>,
    pub voicings: Vec<VoicingKind>,
    pub rules: VoicingRules,
    /// Per layer: least and most hits per bar.
    pub hits: PerLayer<[u32; 2]>,
    /// Weight of offbeat eighths against downbeats when placing hits.
    pub offbeat_weight: f32,
    /// Hit length range in beats.
    pub length: [f32; 2],
    pub velocity: [f32; 2],
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DrumStyle {
    pub ride: String,
    pub hat: String,
    pub brushes: String,
    /// Per layer: chance of the ride's skip note on the "and" of 2 and 4.
    pub ride_skip: PerLayer<f32>,
    /// Per layer: hi-hat foot on 2 and 4.
    pub hat_on: PerLayer<bool>,
    /// Per layer: chance of a brush swish on each beat.
    pub brush: PerLayer<f32>,
    pub ride_velocity: [f32; 2],
    pub hat_velocity: f32,
    pub brush_velocity: [f32; 2],
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LeadStyle {
    pub patch: String,
    pub range: [u8; 2],
    /// Per layer: chance a bar carries the motif (0: no lead).
    pub density: PerLayer<f32>,
    /// Motif length in beats, from this list.
    pub motif_beats: Vec<u32>,
    /// Chance an eighth slot of the motif rhythm sounds.
    pub fill: f32,
    /// Chance a weak-beat note is a chromatic passing tone.
    pub chromatic: f32,
    /// Largest leap in semitones.
    pub max_leap: u8,
    pub velocity: [f32; 2],
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StingerStyle {
    /// Bars of the cadence a stinger plays.
    pub bars: u32,
    /// Patches, besides bass, the stinger chord sounds on.
    pub patches: Vec<String>,
}

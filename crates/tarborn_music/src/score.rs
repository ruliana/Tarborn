//! The symbolic note format.
//!
//! A [`Fragment`] is a few bars of one part (bass, comping, lead, band drums
//! or a stinger) at one intensity layer of one section. Times are in beats
//! from the start of the fragment, written straight: swing is applied when
//! the fragment is played, so the same notes can swing harder or softer per
//! style and tempo.

use crate::theory::{Chord, Mode, Scale};
use serde::{Deserialize, Serialize};

/// What a fragment plays in the band. The director layers fragments by role.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub enum Role {
    #[default]
    Bass,
    /// Chords: piano, horn stabs, marimba.
    Comp,
    Lead,
    /// The band's own light drummer (ride, hi-hat, brushes).
    Drums,
    /// A short standalone piece for menus, stage transitions or death.
    Stinger,
}

/// One note.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Note {
    /// Start, in beats from the start of the fragment, before swing.
    pub start: f32,
    /// Length in beats, before swing. Only patches with `gate` hold for it;
    /// percussive patches ring out their own envelope.
    pub length: f32,
    /// MIDI note number (60 is middle C). Unpitched patches ignore it.
    pub pitch: u8,
    /// Loudness, 0 to 1.
    pub velocity: f32,
    /// Name of the patch that plays it.
    pub patch: String,
}

impl Default for Note {
    fn default() -> Self {
        Self {
            start: 0.0,
            length: 1.0,
            pitch: 60,
            velocity: 0.8,
            patch: String::new(),
        }
    }
}

/// A few bars of one part.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Fragment {
    pub role: Role,
    /// Intensity layer this fragment belongs to, 0 being the calmest.
    pub layer: u8,
    /// Section label in the form ("A", "B", "head", "vamp", ...).
    pub section: String,
    /// Length in bars.
    pub bars: u32,
    /// Beats per bar.
    pub beats_per_bar: u32,
    /// The style file it was generated for ("industrial", "forest").
    pub style: String,
    /// The harmony underneath, so parts generated apart still fit and the
    /// metrics can judge notes against their chord.
    pub chords: Vec<ChordSpan>,
    pub notes: Vec<Note>,
}

/// A chord over a stretch of a fragment, with the scale it implies (the
/// chord's own chord-scale, or a mode held for a modal vamp).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ChordSpan {
    /// Start and length in beats.
    pub start: f32,
    pub length: f32,
    pub chord: Chord,
    pub mode: Mode,
}

impl ChordSpan {
    pub fn scale(&self) -> Scale {
        Scale::new(self.chord.root, self.mode)
    }

    pub fn covers(&self, beat: f32) -> bool {
        beat >= self.start && beat < self.start + self.length
    }
}

impl Default for Fragment {
    fn default() -> Self {
        Self {
            role: Role::Bass,
            layer: 0,
            section: String::from("A"),
            bars: 1,
            beats_per_bar: 4,
            style: String::new(),
            chords: Vec::new(),
            notes: Vec::new(),
        }
    }
}

impl Fragment {
    /// Length in beats.
    pub fn beats(&self) -> f32 {
        (self.bars * self.beats_per_bar) as f32
    }

    /// The chord sounding at `beat`.
    pub fn chord_at(&self, beat: f32) -> Option<&ChordSpan> {
        self.chords.iter().find(|c| c.covers(beat))
    }
}

/// Frequency in Hz of a MIDI note number, in equal temperament with A4 at
/// `a4` Hz.
pub fn midi_to_hz(pitch: f32, a4: f32) -> f32 {
    a4 * ((pitch - 69.0) / 12.0).exp2()
}

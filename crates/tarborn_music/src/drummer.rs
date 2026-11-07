//! The player as drummer (Ape Out style): each move plays a hit from the
//! stage's kit right away, unquantized, and the band listens.
//!
//! - **Hits**: the style's [`Kit`] maps move names (as the game reports
//!   them) to a patch and a velocity, scaled by how strong the move was.
//! - **Key-tuned**: a hit can sound the tune's root, or walk up the current
//!   chord's tones one hit at a time, so a run of accents plays an arpeggio
//!   that always fits the harmony.
//! - **Ducking**: the more the player plays, the softer the band drummer
//!   ([`Drummer::duck`]).
//! - **Fills**: a burst of hits makes the band answer with a fill.
//! - **Section change**: a slam on (or just before) a bar line moves the
//!   tune on to its next section.

use crate::theory::{Chord, Pc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

/// How a hit is pitched.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum Tune {
    /// As generated (unpitched kit pieces ignore pitch anyway).
    #[default]
    Fixed,
    /// The tune's tonic, nearest the patch's register.
    Root,
    /// The current chord's tones, one step further up each hit.
    ChordTone,
}

/// One move's hit.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KitHit {
    pub patch: String,
    pub velocity: f32,
    /// How much the move's strength (0..1) scales the velocity: 0 ignores
    /// it, 1 plays a zero-strength move silent.
    #[serde(default)]
    pub dynamics: f32,
    #[serde(default)]
    pub tune: Tune,
}

/// A style's player kit and how the band listens to it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Kit {
    /// Move name -> hit. Moves left out make no sound.
    pub hits: BTreeMap<String, KitHit>,
    /// MIDI pitch that tuned hits are kept near.
    pub register: i32,
    /// Seconds of hits the ducking looks back over, and the number of hits
    /// in that window at which the band drummer is fully ducked.
    pub duck_window: f32,
    pub duck_hits: f32,
    /// This many hits within `burst_window` seconds is a burst; the band
    /// answers with a fill, at most once per `fill_cooldown` seconds.
    pub burst_hits: usize,
    pub burst_window: f32,
    pub fill_cooldown: f32,
    /// A slam within this many beats of a bar line (before or after) moves
    /// the tune to its next section.
    pub downbeat_window: f32,
    /// Move name that counts as a slam for that.
    pub section_move: String,
    /// Speed (m/s) of a slam or throw that counts as full strength.
    pub full_speed: f32,
}

impl Default for Kit {
    fn default() -> Self {
        Self {
            hits: BTreeMap::new(),
            register: 69,
            duck_window: 2.0,
            duck_hits: 6.0,
            burst_hits: 4,
            burst_window: 1.0,
            fill_cooldown: 4.0,
            downbeat_window: 0.25,
            section_move: "slam".into(),
            full_speed: 24.0,
        }
    }
}

/// A hit to play now.
#[derive(Clone, Debug, PartialEq)]
pub struct Strike {
    pub patch: String,
    pub pitch: u8,
    pub velocity: f32,
}

/// What the band should do in answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    Fill,
    NextSection,
}

/// Where the music is when a move happens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moment<'a> {
    /// The tune's tonic.
    pub key: Pc,
    /// The chord sounding, if known.
    pub chord: Option<&'a Chord>,
    /// Beats from the nearest bar line: negative just before it.
    pub from_bar_line: f32,
}

/// Listens to the player's moves.
#[derive(Clone, Debug)]
pub struct Drummer {
    kit: Kit,
    time: f32,
    recent: VecDeque<f32>,
    step: usize,
    last_fill: Option<f32>,
}

impl Drummer {
    pub fn new(kit: Kit) -> Self {
        Self {
            kit,
            time: 0.0,
            recent: VecDeque::new(),
            step: 0,
            last_fill: None,
        }
    }

    pub fn kit(&self) -> &Kit {
        &self.kit
    }

    /// Moves time on by `dt` seconds.
    pub fn advance(&mut self, dt: f32) {
        self.time += dt;
        let keep = self.kit.duck_window.max(self.kit.burst_window);
        while self.recent.front().is_some_and(|&t| self.time - t > keep) {
            self.recent.pop_front();
        }
    }

    /// How much the band drummer should duck, 0..1.
    pub fn duck(&self) -> f32 {
        let window = self.kit.duck_window;
        let n = self
            .recent
            .iter()
            .filter(|&&t| self.time - t <= window)
            .count();
        (n as f32 / self.kit.duck_hits.max(1e-3)).min(1.0)
    }

    /// A move happened now, with strength 0..1. Returns the hit to play (if
    /// the kit has one) and the band's cues.
    pub fn hear(&mut self, name: &str, strength: f32, at: Moment) -> (Option<Strike>, Vec<Cue>) {
        let mut cues = Vec::new();
        if name == self.kit.section_move && at.from_bar_line.abs() <= self.kit.downbeat_window {
            cues.push(Cue::NextSection);
        }
        let Some(hit) = self.kit.hits.get(name) else {
            return (None, cues);
        };
        self.recent.push_back(self.time);
        let burst = self
            .recent
            .iter()
            .filter(|&&t| self.time - t <= self.kit.burst_window)
            .count();
        let cooled = self
            .last_fill
            .is_none_or(|t| self.time - t >= self.kit.fill_cooldown);
        if burst >= self.kit.burst_hits.max(1) && cooled {
            self.last_fill = Some(self.time);
            cues.push(Cue::Fill);
        }
        let register = self.kit.register;
        let pitch = match hit.tune {
            Tune::Fixed => register,
            Tune::Root => at.key.nearest(register),
            Tune::ChordTone => {
                let tones = match at.chord {
                    Some(c) => c.tones().to_vec(),
                    None => vec![at.key],
                };
                let pc = tones[self.step % tones.len()];
                self.step += 1;
                // Walk upward from the register for an arpeggio feel.
                let p = pc.nearest(register);
                if p < register { p + 12 } else { p }
            }
        };
        let s = strength.clamp(0.0, 1.0);
        let velocity = (hit.velocity * (1.0 - hit.dynamics * (1.0 - s))).clamp(0.0, 1.0);
        let strike = Strike {
            patch: hit.patch.clone(),
            pitch: pitch.clamp(0, 127) as u8,
            velocity,
        };
        (Some(strike), cues)
    }
}

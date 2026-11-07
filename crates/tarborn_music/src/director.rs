//! The live director: picks, bar by bar, what the band plays from the
//! library, following the game's intensity.
//!
//! Every bar line it decides:
//! - **Where in the tune**: the tune's form, section by section; after a few
//!   choruses it moves to another library tune. A run keeps one tempo, so
//!   tunes change without the clock ever jumping.
//! - **How loud the band is**: the intensity layer. It rises at the next bar
//!   line, and falls one layer at a time only after the intensity has stayed
//!   low for a while, so the music doesn't flicker.
//! - **Which variant of each part**: re-picked at every section and, inside
//!   long sections, one part every few bars, so even a calm room keeps
//!   moving. A long calm spell gets a short lift (a "breather").
//! - **How hard the band drummer plays**: it ducks under the player's own
//!   drumming.
//!
//! Layers line up bar for bar (every part of a section shares its chords),
//! so any switch at a bar line is seamless. The run seed drives every
//! choice: two runs differ, and the same seed replays exactly.
//!
//! [`Voicer`] turns the chosen notes into samples for the live stream.

use crate::render::{Patch, PatchBank, RenderSettings};
use crate::rng::Rng;
use crate::score::{ChordSpan, Fragment, Note, Role};
use crate::search::{LibraryTune, Realized, realize};
use crate::style::Style;
use crate::theory::Pc;
use crate::time::swing;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// How the director reacts. The `director` section of `music.ron`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct DirectorParams {
    /// The style the game plays until stages pick their own (step 9).
    pub style: String,
    /// Intensity (0..1) at which each layer above the calmest starts.
    pub layer_at: Vec<f32>,
    /// Bars the intensity must stay below the current layer before the band
    /// drops one layer.
    pub fall_bars: u32,
    /// Choruses (times through the form) a tune plays, least and most,
    /// before the next tune.
    pub choruses: [u32; 2],
    /// Inside a section, one part changes variant every this many bars
    /// (0 = only at section starts).
    pub variety_bars: u32,
    /// After this many bars in the calmest layer, the band lifts one layer
    /// for `breather_bars` bars (0 = never).
    pub breather_after: u32,
    pub breather_bars: u32,
    /// Weight of a part's best variant against its next best, and so on
    /// down (1 = all variants equally likely).
    pub variant_bias: f32,
    /// How much the band drummer's velocity drops at full duck (0..1).
    pub duck_depth: f32,
    /// The band's answer to a burst of player hits: `fill_notes` even hits
    /// over the bar's last `fill_beats` beats, rising in velocity, on the
    /// style's hat (or brushes, when the style brushes).
    pub fill_beats: f32,
    pub fill_notes: u32,
    pub fill_velocity: [f32; 2],
}

impl Default for DirectorParams {
    fn default() -> Self {
        Self {
            style: "industrial".into(),
            layer_at: vec![0.3, 0.7],
            fall_bars: 4,
            choruses: [2, 3],
            variety_bars: 4,
            breather_after: 16,
            breather_bars: 4,
            variant_bias: 2.0,
            duck_depth: 0.6,
            fill_beats: 1.0,
            fill_notes: 3,
            fill_velocity: [0.5, 0.9],
        }
    }
}

/// What the game tells the director each bar.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    /// 0 calm .. 1 full fight.
    pub intensity: f32,
    /// 0 .. 1: how much the player is drumming, so the band drummer ducks.
    pub duck: f32,
}

/// One bar of music, as decided at its bar line.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    /// Bars since the director started.
    pub index: u64,
    /// Seed of the library tune playing.
    pub tune: u32,
    /// Section (index into the tune's sections) and bar inside it.
    pub section: usize,
    pub bar_in_section: u32,
    pub layer: u8,
    /// True when a lift out of a long calm spell is playing.
    pub breather: bool,
    /// The tune's tonic.
    pub key: Pc,
    /// Chords sounding in this bar, `start` relative to the bar line.
    pub chords: Vec<ChordSpan>,
    /// True when the band answers the player with a fill.
    pub fill: bool,
    /// Notes starting in this bar; `start` is in straight beats from the
    /// bar line (swing is applied when voicing).
    pub notes: Vec<Note>,
}

impl Bar {
    /// The chord at `beat` beats after the bar line.
    pub fn chord_at(&self, beat: f32) -> Option<&ChordSpan> {
        self.chords
            .iter()
            .find(|c| c.covers(beat))
            .or(self.chords.last())
    }
}

/// The live director for one style and one run.
pub struct Director {
    style: Style,
    library: Vec<LibraryTune>,
    params: DirectorParams,
    rng: Rng,
    bpm: f32,
    swing: f32,
    /// Library order for this run.
    order: Vec<usize>,
    next_in_order: usize,
    current: Realized,
    tune_seed: u32,
    /// Index into the form, bar inside that section, choruses played and
    /// to play.
    form_at: usize,
    bar_in_section: u32,
    chorus: u32,
    choruses: u32,
    /// Variant playing for each library part (by index into `current.parts`).
    picks: Vec<usize>,
    layer: u8,
    low_bars: u32,
    calm_bars: u32,
    breather_left: u32,
    index: u64,
    fill_pending: bool,
    jump_pending: bool,
}

impl Director {
    /// None if the library is empty.
    pub fn new(
        style: Style,
        library: Vec<LibraryTune>,
        params: DirectorParams,
        run_seed: u32,
    ) -> Option<Self> {
        if library.is_empty() {
            return None;
        }
        let mut rng = Rng::new(run_seed ^ 0xD1EC_7012);
        let mut order: Vec<usize> = (0..library.len()).collect();
        for i in (1..order.len()).rev() {
            let j = rng.int(0, i as i32) as usize;
            order.swap(i, j);
        }
        let first = &library[order[0]];
        // One tempo per run, taken from the first tune: every later tune
        // plays at it, so the beat clock never jumps.
        let bpm = first.bpm;
        let swing = style.swing.ratio(bpm);
        let current = realize(&style, first);
        let tune_seed = first.seed;
        let mut d = Self {
            style,
            params,
            bpm,
            swing,
            order,
            next_in_order: 1,
            current,
            tune_seed,
            form_at: 0,
            bar_in_section: 0,
            chorus: 0,
            choruses: 1,
            picks: Vec::new(),
            layer: 0,
            low_bars: 0,
            calm_bars: 0,
            breather_left: 0,
            index: 0,
            fill_pending: false,
            jump_pending: false,
            rng,
            library,
        };
        d.start_tune();
        Some(d)
    }

    /// The tonic of the tune playing.
    pub fn key(&self) -> Pc {
        self.current.tune.key
    }

    pub fn bpm(&self) -> f32 {
        self.bpm
    }

    pub fn swing(&self) -> f32 {
        self.swing
    }

    pub fn beats_per_bar(&self) -> u32 {
        self.style.beats_per_bar.max(1)
    }

    pub fn style(&self) -> &Style {
        &self.style
    }

    fn start_tune(&mut self) {
        let [lo, hi] = self.params.choruses;
        self.choruses = self.rng.int(lo.max(1) as i32, hi.max(lo).max(1) as i32) as u32;
        self.chorus = 0;
        self.form_at = 0;
        self.bar_in_section = 0;
        self.picks = vec![0; self.current.parts.len()];
        for i in 0..self.picks.len() {
            self.repick(i);
        }
    }

    fn next_tune(&mut self) {
        let k = self.order[self.next_in_order % self.order.len()];
        self.next_in_order += 1;
        self.current = realize(&self.style, &self.library[k]);
        self.tune_seed = self.library[k].seed;
        self.start_tune();
    }

    /// Re-picks one part's variant, best ones more likely.
    fn repick(&mut self, part: usize) {
        let n = self.current.parts[part].1.len();
        let bias = self.params.variant_bias.max(1.0);
        let weights: Vec<f32> = (0..n).map(|r| bias.powi(-(r as i32))).collect();
        self.picks[part] = self.rng.weighted(&weights);
    }

    fn section(&self) -> usize {
        self.current.tune.form[self.form_at]
    }

    fn section_bars(&self) -> u32 {
        self.current.tune.sections[self.section()].bars.max(1)
    }

    /// The layer the intensity asks for.
    fn wanted(&self, intensity: f32) -> u8 {
        let top = self.style.layers.max(1) - 1;
        let n = self
            .params
            .layer_at
            .iter()
            .filter(|&&at| intensity >= at)
            .count();
        (n as u8).min(top)
    }

    fn update_layer(&mut self, input: Input) {
        let wanted = self.wanted(input.intensity);
        if wanted >= self.layer {
            self.layer = wanted;
            self.low_bars = 0;
        } else {
            self.low_bars += 1;
            if self.low_bars >= self.params.fall_bars.max(1) {
                self.layer -= 1;
                self.low_bars = 0;
            }
        }
        if self.layer == 0 {
            self.calm_bars += 1;
        } else {
            self.calm_bars = 0;
        }
        if self.breather_left > 0 {
            self.breather_left -= 1;
        } else if self.params.breather_after > 0
            && self.layer == 0
            && self.calm_bars > self.params.breather_after
        {
            self.breather_left = self.params.breather_bars;
            self.calm_bars = 0;
        }
    }

    /// The band answers with a fill in the next bar it decides.
    pub fn request_fill(&mut self) {
        self.fill_pending = true;
    }

    /// The next bar decided starts the tune's next section.
    pub fn request_next_section(&mut self) {
        self.jump_pending = true;
    }

    /// Decides the next bar. Call once per bar, at or before its bar line.
    pub fn next_bar(&mut self, input: Input) -> Bar {
        if std::mem::take(&mut self.jump_pending) && self.bar_in_section > 0 {
            self.end_section();
        }
        self.update_layer(input);
        let breather = self.breather_left > 0 && self.layer == 0;
        let layer = if breather {
            1.min(self.style.layers.max(1) - 1)
        } else {
            self.layer
        };

        if self.bar_in_section > 0
            && self.params.variety_bars > 0
            && self.bar_in_section.is_multiple_of(self.params.variety_bars)
        {
            let section = self.section();
            let in_section: Vec<usize> = (0..self.picks.len())
                .filter(|&i| self.current.parts[i].0.section == section)
                .collect();
            if !in_section.is_empty() {
                let part = *self.rng.pick(&in_section);
                self.repick(part);
            }
        }

        let section = self.section();
        let bpb = self.beats_per_bar() as f32;
        let (from, to) = (
            self.bar_in_section as f32 * bpb,
            (self.bar_in_section + 1) as f32 * bpb,
        );
        let mut notes = Vec::new();
        for (i, (part, fragments)) in self.current.parts.iter().enumerate() {
            if part.section != section || part.layer != layer {
                continue;
            }
            let f: &Fragment = &fragments[self.picks[i].min(fragments.len() - 1)];
            for n in f.notes.iter().filter(|n| n.start >= from && n.start < to) {
                let mut n = n.clone();
                n.start -= from;
                if part.role == Role::Drums {
                    n.velocity *= 1.0 - self.params.duck_depth * input.duck.clamp(0.0, 1.0);
                }
                notes.push(n);
            }
        }
        notes.sort_by(|a, b| a.start.total_cmp(&b.start));

        let fill = std::mem::take(&mut self.fill_pending);
        if fill {
            self.add_fill(&mut notes);
        }
        let chords = self.current.tune.sections[section]
            .chords
            .iter()
            .filter(|c| c.start < to && c.start + c.length > from)
            .map(|c| ChordSpan {
                start: c.start - from,
                ..c.clone()
            })
            .collect();
        let bar = Bar {
            index: self.index,
            tune: self.library_seed(),
            section,
            bar_in_section: self.bar_in_section,
            layer,
            breather,
            key: self.current.tune.key,
            chords,
            fill,
            notes,
        };
        self.advance();
        bar
    }

    fn library_seed(&self) -> u32 {
        self.tune_seed
    }

    fn add_fill(&self, notes: &mut Vec<Note>) {
        let d = &self.style.drums;
        let brushed = d.brush.iter().any(|&b| b > 0.0);
        let patch = if brushed { &d.brushes } else { &d.hat };
        let n = self.params.fill_notes.max(1);
        let span = self
            .params
            .fill_beats
            .clamp(0.0, self.beats_per_bar() as f32);
        let start = self.beats_per_bar() as f32 - span;
        let [lo, hi] = self.params.fill_velocity;
        for i in 0..n {
            let t = if n > 1 {
                i as f32 / (n - 1) as f32
            } else {
                1.0
            };
            notes.push(Note {
                start: start + span * i as f32 / n as f32,
                length: span / n as f32,
                pitch: 60,
                velocity: lo + (hi - lo) * t,
                patch: patch.clone(),
            });
        }
        notes.sort_by(|a, b| a.start.total_cmp(&b.start));
    }

    fn advance(&mut self) {
        self.index += 1;
        self.bar_in_section += 1;
        if self.bar_in_section < self.section_bars() {
            return;
        }
        self.end_section();
    }

    /// Moves on to the start of the next section in the form.
    fn end_section(&mut self) {
        self.bar_in_section = 0;
        self.form_at += 1;
        if self.form_at < self.current.tune.form.len() {
            // A new section: fresh variants for its parts.
            let section = self.section();
            for i in 0..self.picks.len() {
                if self.current.parts[i].0.section == section {
                    self.repick(i);
                }
            }
            return;
        }
        self.form_at = 0;
        self.chorus += 1;
        if self.chorus >= self.choruses {
            self.next_tune();
        } else {
            for i in 0..self.picks.len() {
                self.repick(i);
            }
        }
    }
}

/// A note ready for the stream.
#[derive(Clone, Debug)]
pub struct Voice {
    /// When it sounds, in beats on the beat clock (swing and timing jitter
    /// applied).
    pub beat: f64,
    pub samples: Arc<[f32]>,
    pub gain: f32,
}

/// Turns notes into samples, caching renders: a jazz band repeats itself.
pub struct Voicer {
    bank: PatchBank,
    settings: RenderSettings,
    cache: HashMap<(String, u8, u32), Arc<[f32]>>,
    /// Most renders kept before the cache is cleared.
    pub cache_limit: usize,
    rng: Rng,
}

impl Voicer {
    pub fn new(bank: PatchBank, settings: RenderSettings, seed: u32) -> Self {
        Self {
            bank,
            settings,
            cache: HashMap::new(),
            cache_limit: 4096,
            rng: Rng::new(seed ^ 0x7015_CE11),
        }
    }

    pub fn set_settings(&mut self, settings: RenderSettings) {
        if settings != self.settings {
            self.cache.clear();
        }
        self.settings = settings;
    }

    /// Voices a player's hit, to sound at once: no swing, no jitter.
    pub fn strike(&mut self, patch: &str, pitch: u8, velocity: f32) -> Option<(Arc<[f32]>, f32)> {
        let s = &self.settings;
        let p: &Patch = self.bank.patches.get(patch)?;
        let gain =
            velocity.clamp(0.0, 1.0).powf(s.velocity_curve.max(0.01)) * p.gain * s.master_volume;
        let key = (patch.to_string(), pitch, 0);
        if self.cache.len() >= self.cache_limit && !self.cache.contains_key(&key) {
            self.cache.clear();
        }
        let (rate, a4) = (s.sample_rate, s.a4);
        let samples = self
            .cache
            .entry(key)
            .or_insert_with(|| p.voiced(pitch as f32, 0.0, a4).render(rate).into())
            .clone();
        Some((samples, gain))
    }

    /// Voices a note of the bar starting at `bar_beat` on the clock. None if
    /// its patch is unknown.
    pub fn voice(
        &mut self,
        note: &Note,
        bar_beat: f64,
        bpm: f32,
        swing_ratio: f32,
    ) -> Option<Voice> {
        let s = &self.settings;
        let patch: &Patch = self.bank.patches.get(&note.patch)?;
        let (r_time, r_vel) = (self.rng.range([-1.0, 1.0]), self.rng.range([-1.0, 1.0]));
        let start = (note.start + s.timing_jitter * r_time).max(0.0);
        let swung = swing(start, swing_ratio);
        let end = swing(start + note.length.max(0.0), swing_ratio);
        let seconds = (end - swung).max(0.0) * 60.0 / bpm.max(1.0);
        let velocity = (note.velocity * (1.0 + s.velocity_jitter * r_vel)).clamp(0.0, 1.0);
        let gain = velocity.powf(s.velocity_curve.max(0.01)) * patch.gain * s.master_volume;
        // Only gated patches depend on the note's length; round it to 10 ms.
        let held = if patch.gate {
            (seconds * 100.0).round() as u32
        } else {
            0
        };
        let key = (note.patch.clone(), note.pitch, held);
        if self.cache.len() >= self.cache_limit && !self.cache.contains_key(&key) {
            self.cache.clear();
        }
        let (rate, a4) = (s.sample_rate, s.a4);
        let samples = self
            .cache
            .entry(key)
            .or_insert_with(|| {
                patch
                    .voiced(note.pitch as f32, held as f32 / 100.0, a4)
                    .render(rate)
                    .into()
            })
            .clone();
        Some(Voice {
            beat: bar_beat + swung as f64,
            samples,
            gain,
        })
    }
}

//! Fun telemetry: what the music did in a run, how the player played with
//! it, and what the player thought of it. The game writes one [`RunLog`]
//! per run; [`summarize`] turns a pile of them into the numbers the offline
//! weights are tuned against (the report binary prints it with `--runs`).
//!
//! - Tension against gameplay: does the band's layer rise with the mood?
//! - Repetition fatigue: tunes heard again after others within a run.
//! - The player as drummer: how near the beat hits land, and how many band
//!   notes they compete with.
//! - The human signal: one-tap stage ratings, and mutes.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Telemetry settings, the `telemetry` section of `music.ron`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TelemetryParams {
    pub enabled: bool,
    /// Where run logs go, relative to the folder holding `music.ron`.
    pub dir: String,
    /// Beats from the nearest beat within which a hit counts as on it.
    pub on_beat: f32,
    /// Beats around a hit within which band notes count as competing.
    pub mask_window: f32,
}

impl Default for TelemetryParams {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: "../logs/music".into(),
            on_beat: 0.125,
            mask_window: 0.125,
        }
    }
}

/// One bar the band played.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct BarLog {
    /// Seconds into the run, at the bar line.
    pub t: f32,
    pub style: String,
    /// Library seed of the tune.
    pub tune: u32,
    pub section: usize,
    pub layer: u8,
    pub breather: bool,
    pub fill: bool,
    pub intensity: f32,
    pub danger: f32,
    pub duck: f32,
    pub notes: u32,
}

/// One drum hit of the player's.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct HitLog {
    pub t: f32,
    /// The move ("jump", "slam", ...).
    pub name: String,
    /// Beats from the nearest beat, negative when early.
    pub from_beat: f32,
    /// Band notes within the mask window.
    pub band_near: u32,
}

/// The player muted or unmuted.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct MuteLog {
    pub t: f32,
    pub muted: bool,
}

/// A one-tap rating of a stage's music.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Rating {
    pub t: f32,
    /// Stage number in the run, from 0.
    pub stage: usize,
    pub style: String,
    /// +1 liked, -1 didn't.
    pub rating: i8,
}

/// Everything logged in one run.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct RunLog {
    /// Unix seconds at the start.
    pub started: u64,
    pub bars: Vec<BarLog>,
    pub hits: Vec<HitLog>,
    pub mutes: Vec<MuteLog>,
    pub ratings: Vec<Rating>,
}

impl RunLog {
    /// Seconds the music was muted, up to `end`.
    pub fn muted_seconds(&self, end: f32) -> f32 {
        let mut total = 0.0;
        let mut since = None;
        for m in &self.mutes {
            match (m.muted, since) {
                (true, None) => since = Some(m.t),
                (false, Some(s)) => {
                    total += m.t - s;
                    since = None;
                }
                _ => {}
            }
        }
        if let Some(s) = since {
            total += (end - s).max(0.0);
        }
        total
    }

    /// Seconds from the first to the last thing logged.
    pub fn length(&self) -> f32 {
        let ts = self
            .bars
            .iter()
            .map(|b| b.t)
            .chain(self.hits.iter().map(|h| h.t))
            .chain(self.mutes.iter().map(|m| m.t))
            .chain(self.ratings.iter().map(|r| r.t));
        let (lo, hi) = ts.fold((f32::MAX, f32::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)));
        if hi >= lo { hi - lo } else { 0.0 }
    }

    /// Tunes started again after another tune played, within the run.
    pub fn tune_repeats(&self) -> (u32, u32) {
        let mut heard = BTreeSet::new();
        let mut last = None;
        let (mut starts, mut repeats) = (0, 0);
        for b in &self.bars {
            let tune = (b.style.as_str(), b.tune);
            if last == Some(tune) {
                continue;
            }
            last = Some(tune);
            starts += 1;
            if !heard.insert(tune) {
                repeats += 1;
            }
        }
        (starts, repeats)
    }
}

/// Pearson correlation; 0 when either side is flat.
pub fn correlation(pairs: &[(f32, f32)]) -> f32 {
    let n = pairs.len() as f64;
    if n < 2.0 {
        return 0.0;
    }
    let (sx, sy) = pairs
        .iter()
        .fold((0.0, 0.0), |(a, b), &(x, y)| (a + x as f64, b + y as f64));
    let (mx, my) = (sx / n, sy / n);
    let (mut cov, mut vx, mut vy) = (0.0, 0.0, 0.0);
    for &(x, y) in pairs {
        let (dx, dy) = (x as f64 - mx, y as f64 - my);
        cov += dx * dy;
        vx += dx * dx;
        vy += dy * dy;
    }
    if vx <= 1e-12 || vy <= 1e-12 {
        return 0.0;
    }
    (cov / (vx * vy).sqrt()) as f32
}

/// What the logs say about one style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleSummary {
    pub bars: usize,
    /// Correlation of the band's layer with the mood's intensity.
    pub tension_follows_play: f32,
    pub mean_layer: f32,
    pub ratings: usize,
    /// Mean rating, -1 to 1.
    pub rating: f32,
}

/// What a pile of run logs says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub runs: usize,
    pub minutes: f32,
    pub styles: BTreeMap<String, StyleSummary>,
    /// Tune starts, and how many were tunes heard before in the same run.
    pub tune_starts: u32,
    pub tune_repeats: u32,
    pub hits: usize,
    /// Mean |beats from the beat| of the player's hits.
    pub hit_offset: f32,
    /// Share of hits on the beat.
    pub on_beat: f32,
    /// Mean band notes competing with a hit.
    pub masking: f32,
    pub mutes: usize,
    /// Share of the logged time spent muted.
    pub muted_share: f32,
}

pub fn summarize(logs: &[RunLog], p: &TelemetryParams) -> Summary {
    let mut s = Summary {
        runs: logs.len(),
        ..Default::default()
    };
    let mut pairs: BTreeMap<String, Vec<(f32, f32)>> = BTreeMap::new();
    let mut ratings: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    let (mut seconds, mut muted) = (0.0, 0.0);
    let (mut offset, mut on, mut near) = (0.0, 0usize, 0u64);
    for log in logs {
        let length = log.length();
        seconds += length;
        let start = log.bars.first().map_or(0.0, |b| b.t);
        muted += log.muted_seconds(start + length);
        s.mutes += log.mutes.iter().filter(|m| m.muted).count();
        let (starts, repeats) = log.tune_repeats();
        s.tune_starts += starts;
        s.tune_repeats += repeats;
        for b in &log.bars {
            pairs
                .entry(b.style.clone())
                .or_default()
                .push((b.intensity, b.layer as f32));
        }
        for r in &log.ratings {
            ratings
                .entry(r.style.clone())
                .or_default()
                .push(r.rating as f32);
        }
        for h in &log.hits {
            s.hits += 1;
            offset += h.from_beat.abs();
            on += (h.from_beat.abs() <= p.on_beat) as usize;
            near += h.band_near as u64;
        }
    }
    for (style, pairs) in pairs {
        let st = s.styles.entry(style).or_default();
        st.bars = pairs.len();
        st.tension_follows_play = correlation(&pairs);
        st.mean_layer = pairs.iter().map(|p| p.1).sum::<f32>() / pairs.len().max(1) as f32;
    }
    for (style, rs) in ratings {
        let st = s.styles.entry(style).or_default();
        st.ratings = rs.len();
        st.rating = rs.iter().sum::<f32>() / rs.len().max(1) as f32;
    }
    s.minutes = seconds / 60.0;
    s.muted_share = if seconds > 0.0 { muted / seconds } else { 0.0 };
    if s.hits > 0 {
        let n = s.hits as f32;
        s.hit_offset = offset / n;
        s.on_beat = on as f32 / n;
        s.masking = near as f32 / n;
    }
    s
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} runs, {:.1} minutes of music",
            self.runs, self.minutes
        )?;
        writeln!(
            f,
            "  {:<12} {:>6} {:>9} {:>11} {:>8} {:>7}",
            "style", "bars", "layer", "tension~play", "ratings", "rating"
        )?;
        for (name, st) in &self.styles {
            writeln!(
                f,
                "  {:<12} {:>6} {:>9.2} {:>11.2} {:>8} {:>+7.2}",
                name, st.bars, st.mean_layer, st.tension_follows_play, st.ratings, st.rating
            )?;
        }
        let share = |a: u32, b: u32| if b > 0 { a as f32 / b as f32 } else { 0.0 };
        writeln!(
            f,
            "  repetition: {} of {} tune starts were repeats ({:.0}%)",
            self.tune_repeats,
            self.tune_starts,
            100.0 * share(self.tune_repeats, self.tune_starts)
        )?;
        writeln!(
            f,
            "  drumming: {} hits, {:.3} beats off on average, {:.0}% on the beat, {:.1} band notes near each",
            self.hits,
            self.hit_offset,
            100.0 * self.on_beat,
            self.masking
        )?;
        write!(
            f,
            "  mutes: {} ({:.0}% of the time muted)",
            self.mutes,
            100.0 * self.muted_share
        )
    }
}

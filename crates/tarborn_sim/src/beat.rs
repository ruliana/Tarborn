//! The musical beat, counted in simulation ticks.
//!
//! The music follows this clock, not the other way round: beats are a pure
//! function of the sim tick, so anything in the sim that lines up with the
//! music (such as enemy windups snapping to the beat) is deterministic and
//! replays exactly from a bot tape. The audio stream chases the clock with a
//! measured offset, so a dropped audio buffer can never change gameplay.

use crate::TICKS_PER_SECOND;

/// Where a tick falls in the music.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatPosition {
    /// Bars since the clock started, from 0.
    pub bar: i64,
    /// Beat inside the bar, from 0.
    pub beat: u32,
    /// How far into the beat, 0 to just under 1.
    pub phase: f32,
}

/// Tempo and meter, anchored to a tick. A tempo change re-anchors the clock
/// so the beat count is continuous across it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatClock {
    bpm: f32,
    beats_per_bar: u32,
    anchor_tick: u64,
    anchor_beat: f64,
}

impl Default for BeatClock {
    fn default() -> Self {
        Self::new(120.0, 4)
    }
}

impl BeatClock {
    /// A clock at `bpm` with beat 0 on tick 0.
    pub fn new(bpm: f32, beats_per_bar: u32) -> Self {
        Self {
            bpm: bpm.max(1.0),
            beats_per_bar: beats_per_bar.max(1),
            anchor_tick: 0,
            anchor_beat: 0.0,
        }
    }

    pub fn bpm(&self) -> f32 {
        self.bpm
    }

    pub fn beats_per_bar(&self) -> u32 {
        self.beats_per_bar
    }

    /// Beats per tick at the current tempo.
    fn rate(&self) -> f64 {
        self.bpm as f64 / 60.0 / TICKS_PER_SECOND as f64
    }

    /// Beats elapsed at `tick` (fractional ticks allowed, for rendering
    /// between ticks). Negative before beat 0.
    pub fn beats_at(&self, tick: f64) -> f64 {
        self.anchor_beat + (tick - self.anchor_tick as f64) * self.rate()
    }

    /// The (fractional) tick on which `beat` falls.
    pub fn tick_of_beat(&self, beat: f64) -> f64 {
        self.anchor_tick as f64 + (beat - self.anchor_beat) / self.rate()
    }

    /// Bar, beat and phase of `tick`.
    pub fn position(&self, tick: u64) -> BeatPosition {
        let beats = self.beats_at(tick as f64);
        let whole = beats.floor();
        let per_bar = self.beats_per_bar as i64;
        let index = whole as i64;
        BeatPosition {
            bar: index.div_euclid(per_bar),
            beat: index.rem_euclid(per_bar) as u32,
            phase: (beats - whole) as f32,
        }
    }

    /// Whole beats that fall after tick `from` and on or before tick `to`:
    /// the beats a step from `from` to `to` crosses.
    pub fn beats_crossed(&self, from: u64, to: u64) -> std::ops::Range<i64> {
        let first = self.beats_at(from as f64).floor() as i64 + 1;
        let last = self.beats_at(to as f64).floor() as i64;
        first..(last + 1).max(first)
    }

    /// Changes tempo from `tick` on, keeping the beat count continuous.
    pub fn set_tempo(&mut self, tick: u64, bpm: f32) {
        self.anchor_beat = self.beats_at(tick as f64);
        self.anchor_tick = tick;
        self.bpm = bpm.max(1.0);
    }

    /// Changes the meter. Bars are counted from beat 0 in the new meter.
    pub fn set_beats_per_bar(&mut self, beats_per_bar: u32) {
        self.beats_per_bar = beats_per_bar.max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beats_follow_ticks_at_tempo() {
        let clock = BeatClock::new(120.0, 4);
        // 120 BPM at 120 Hz: a beat every 60 ticks.
        assert_eq!(
            clock.position(0),
            BeatPosition {
                bar: 0,
                beat: 0,
                phase: 0.0
            }
        );
        assert_eq!(
            clock.position(90),
            BeatPosition {
                bar: 0,
                beat: 1,
                phase: 0.5
            }
        );
        assert_eq!(
            clock.position(60 * 4),
            BeatPosition {
                bar: 1,
                beat: 0,
                phase: 0.0
            }
        );
        assert!((clock.tick_of_beat(3.0) - 180.0).abs() < 1e-9);
    }

    #[test]
    fn ten_minutes_has_the_exact_beat_count() {
        let clock = BeatClock::new(120.0, 4);
        let ticks = (TICKS_PER_SECOND * 600.0) as u64;
        let mut beats = 0;
        for tick in 1..=ticks {
            beats += clock.beats_crossed(tick - 1, tick).count();
        }
        assert_eq!(beats, 1200);
        assert_eq!(clock.position(ticks).bar, 300);
    }

    #[test]
    fn odd_tempos_never_skip_or_double_a_beat() {
        for bpm in [97.0, 133.3, 200.0, 260.0] {
            let clock = BeatClock::new(bpm, 4);
            let mut seen = Vec::new();
            for tick in 1..=12_000u64 {
                seen.extend(clock.beats_crossed(tick - 1, tick));
            }
            let expected: Vec<i64> = (1..=clock.beats_at(12_000.0).floor() as i64).collect();
            assert_eq!(seen, expected, "{bpm} BPM");
        }
    }

    #[test]
    fn tempo_changes_keep_the_count_continuous() {
        let mut clock = BeatClock::new(120.0, 4);
        let before = clock.beats_at(1000.0);
        clock.set_tempo(1000, 240.0);
        assert!((clock.beats_at(1000.0) - before).abs() < 1e-9);
        // Twice as fast afterwards: a beat every 30 ticks.
        assert!((clock.beats_at(1030.0) - before - 1.0).abs() < 1e-9);
        // tick_of_beat inverts beats_at.
        let b = clock.beats_at(1234.5);
        assert!((clock.tick_of_beat(b) - 1234.5).abs() < 1e-6);
    }

    #[test]
    fn negative_beats_place_in_earlier_bars() {
        let mut clock = BeatClock::new(120.0, 4);
        clock.set_tempo(0, 120.0);
        let mut later = clock;
        later.anchor_beat = -1.5;
        let p = later.position(0);
        assert_eq!((p.bar, p.beat), (-1, 2));
        assert!((p.phase - 0.5).abs() < 1e-6);
    }
}

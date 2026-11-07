//! The live music stream: a mixer that plays pre-rendered notes at exact
//! sample positions, and the sync that maps sim ticks to those positions.
//!
//! The sim's beat clock is the master. The audio device runs on its own
//! crystal, drifts, and pulls samples in bursts. [`AudioSync`] watches how
//! many samples the device has taken at each frame, against the sim tick at
//! that moment, and keeps a smoothed offset between the two; notes are then
//! scheduled `lookahead` seconds ahead at `sample_for_tick`. A jump in the
//! offset bigger than `resync_threshold` (a pause, a hitch, a new room)
//! re-anchors at once instead of gliding.

use serde::Deserialize;
use std::sync::Arc;

/// Stream and sync tuning. Read from the `stream` section of `music.ron`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct StreamParams {
    /// Seconds ahead of the sim that notes are scheduled.
    pub lookahead: f32,
    /// Fraction of the gap between the measured and smoothed offset closed
    /// at each observation.
    pub smoothing: f32,
    /// Seconds of offset jump that re-anchors at once.
    pub resync_threshold: f32,
    /// Seconds notes are moved earlier to make up for output latency, on top
    /// of the measured buffer when `auto_compensate` is on.
    pub latency_compensation: f32,
    /// Also move notes earlier by the measured device buffer.
    pub auto_compensate: bool,
    /// Gap in seconds between two sample requests that starts a new device
    /// burst; how the buffer size is measured.
    pub burst_gap: f32,
    /// Device buffers in flight; the latency estimate is burst size times
    /// this.
    pub buffers_in_flight: f32,
    /// Seconds between latency log lines.
    pub log_interval: f32,
    /// Most notes sounding at once; the oldest is dropped beyond it.
    pub max_voices: usize,
    /// Linear gain of the whole stream.
    pub volume: f32,
    /// Seconds between checks of `music.ron` for edits.
    pub reload_interval: f32,
}

impl Default for StreamParams {
    fn default() -> Self {
        Self {
            lookahead: 0.1,
            smoothing: 0.05,
            resync_threshold: 0.1,
            latency_compensation: 0.0,
            auto_compensate: true,
            burst_gap: 0.0005,
            buffers_in_flight: 2.0,
            log_interval: 10.0,
            max_voices: 48,
            volume: 1.0,
            reload_interval: 0.5,
        }
    }
}

/// A rendered note waiting to start at sample `at` of the stream.
#[derive(Clone, Debug)]
pub struct Scheduled {
    pub at: u64,
    pub samples: Arc<[f32]>,
    pub gain: f32,
}

struct Playing {
    samples: Arc<[f32]>,
    next: usize,
    gain: f32,
}

/// Mixes scheduled notes into one mono stream, one sample at a time.
pub struct Mixer {
    cursor: u64,
    pending: Vec<Scheduled>,
    playing: Vec<Playing>,
    max_voices: usize,
    /// Notes that arrived after their start sample had passed.
    pub late: u64,
    /// Notes dropped because too many were sounding.
    pub dropped: u64,
}

impl Mixer {
    pub fn new(max_voices: usize) -> Self {
        Self {
            cursor: 0,
            pending: Vec::new(),
            playing: Vec::new(),
            max_voices: max_voices.max(1),
            late: 0,
            dropped: 0,
        }
    }

    /// Samples produced so far.
    pub fn cursor(&self) -> u64 {
        self.cursor
    }

    pub fn set_max_voices(&mut self, max_voices: usize) {
        self.max_voices = max_voices.max(1);
    }

    pub fn schedule(&mut self, note: Scheduled) {
        if note.at < self.cursor {
            self.late += 1;
        }
        self.pending.push(note);
    }

    /// The next sample of the mix (not limited).
    pub fn next_sample(&mut self) -> f32 {
        let now = self.cursor;
        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].at <= now {
                let note = self.pending.swap_remove(i);
                if self.playing.len() >= self.max_voices {
                    self.playing.remove(0);
                    self.dropped += 1;
                }
                self.playing.push(Playing {
                    samples: note.samples,
                    next: 0,
                    gain: note.gain,
                });
            } else {
                i += 1;
            }
        }
        let mut out = 0.0;
        self.playing.retain_mut(|p| match p.samples.get(p.next) {
            Some(s) => {
                out += s * p.gain;
                p.next += 1;
                true
            }
            None => false,
        });
        self.cursor += 1;
        out
    }
}

/// Keeps the stream's sample position in step with the sim tick.
#[derive(Clone, Debug)]
pub struct AudioSync {
    rate: f64,
    ticks_per_second: f64,
    /// Smoothed `samples taken - tick in samples`.
    offset: Option<f64>,
    /// Times the offset jumped and was re-anchored.
    pub resyncs: u64,
}

impl AudioSync {
    pub fn new(sample_rate: u32, ticks_per_second: f32) -> Self {
        Self {
            rate: sample_rate as f64,
            ticks_per_second: ticks_per_second as f64,
            offset: None,
            resyncs: 0,
        }
    }

    fn tick_in_samples(&self, tick: f64) -> f64 {
        tick / self.ticks_per_second * self.rate
    }

    /// Records that the device had taken `taken` samples when the sim was at
    /// (fractional) `tick`.
    pub fn observe(&mut self, tick: f64, taken: u64, p: &StreamParams) {
        let raw = taken as f64 - self.tick_in_samples(tick);
        let threshold = p.resync_threshold as f64 * self.rate;
        self.offset = Some(match self.offset {
            Some(old) if (raw - old).abs() <= threshold => {
                old + (raw - old) * p.smoothing.clamp(0.0, 1.0) as f64
            }
            Some(_) => {
                self.resyncs += 1;
                raw
            }
            None => raw,
        });
    }

    /// Forgets the offset; the next observation anchors afresh.
    pub fn reset(&mut self) {
        self.offset = None;
    }

    /// The stream sample at which something on `tick` should start, moved
    /// earlier by `compensation` seconds. None before the first observation.
    pub fn sample_for_tick(&self, tick: f64, compensation: f32) -> Option<u64> {
        let offset = self.offset?;
        let s = self.tick_in_samples(tick) + offset - compensation as f64 * self.rate;
        Some(s.max(0.0).round() as u64)
    }
}

/// Measures the device's buffer from the bursts in which it takes samples.
#[derive(Clone, Debug, Default)]
pub struct LatencyMeter {
    bursts: u64,
    total: u64,
    smallest: Option<u64>,
    largest: u64,
}

impl LatencyMeter {
    /// Records one burst of `samples` taken back to back.
    pub fn burst(&mut self, samples: u64) {
        if samples == 0 {
            return;
        }
        self.bursts += 1;
        self.total += samples;
        self.smallest = Some(self.smallest.map_or(samples, |s| s.min(samples)));
        self.largest = self.largest.max(samples);
    }

    pub fn bursts(&self) -> u64 {
        self.bursts
    }

    /// Mean burst in samples.
    pub fn mean_burst(&self) -> f64 {
        if self.bursts == 0 {
            0.0
        } else {
            self.total as f64 / self.bursts as f64
        }
    }

    /// Estimated output latency in seconds: the mean burst times the
    /// buffers in flight.
    pub fn latency(&self, sample_rate: u32, p: &StreamParams) -> f32 {
        (self.mean_burst() * p.buffers_in_flight as f64 / sample_rate as f64) as f32
    }

    /// One log line of the measurement.
    pub fn summary(&self, sample_rate: u32, p: &StreamParams) -> String {
        let ms = |samples: f64| samples * 1000.0 / sample_rate as f64;
        format!(
            "music output: {} bursts, {:.1} ms mean ({:.1} to {:.1} ms), estimated latency {:.1} ms",
            self.bursts,
            ms(self.mean_burst()),
            ms(self.smallest.unwrap_or(0) as f64),
            ms(self.largest as f64),
            self.latency(sample_rate, p) * 1000.0
        )
    }
}

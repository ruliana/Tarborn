use std::sync::Arc;
use tarborn_music::stream::{AudioSync, LatencyMeter, Mixer, Scheduled, StreamParams};
use tarborn_sim::{BeatClock, TICKS_PER_SECOND};

const RATE: u32 = 44_100;

/// Deterministic jitter in -1..=1.
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f64 / u32::MAX as f64) * 2.0 - 1.0
    }
}

/// A frame loop like the game's: wall time in jittery frames, the sim on a
/// fixed-step accumulator, and an audio device on its own (drifting) clock
/// that takes samples in bursts. Every beat is scheduled through the sync
/// and the result is when each click reaches the speaker versus when the
/// sim reached that beat. Returns (worst error s, late notes, clicks).
fn run(seconds: f64, drift: f64, burst: u64, bpm: f32, hitch_at: Option<f64>) -> (f64, u64, usize) {
    let p = StreamParams {
        auto_compensate: false,
        latency_compensation: (burst as f64 * 2.0 / RATE as f64) as f32,
        ..StreamParams::default()
    };
    let clock = BeatClock::new(bpm, 4);
    let tick_dt = 1.0 / TICKS_PER_SECOND as f64;
    let mut sync = AudioSync::new(RATE, TICKS_PER_SECOND);
    let mut mixer = Mixer::new(64);
    let click: Arc<[f32]> = vec![1.0; 8].into();
    let mut rng = Rng(7);
    let (mut wall, mut acc, mut tick) = (0.0f64, 0.0f64, 0u64);
    let mut next_beat = 1i64;
    let mut scheduled = Vec::new();
    let mut hitched = false;
    while wall < seconds {
        let mut frame = (1.0 / 60.0) * (1.0 + 0.2 * rng.next());
        if hitch_at.is_some_and(|t| wall >= t) && !hitched {
            // A 250 ms stall: the sim catches up in one frame.
            hitched = true;
            frame += 0.25;
        }
        wall += frame;
        acc += frame;
        while acc >= tick_dt {
            acc -= tick_dt;
            tick += 1;
        }
        // The device runs `drift` fast and keeps two bursts queued ahead.
        let device = wall * RATE as f64 * drift;
        let taken = ((device / burst as f64).floor() as u64 + 2) * burst;
        while mixer.cursor() < taken {
            mixer.next_sample();
        }
        let now = tick as f64 + acc / tick_dt;
        sync.observe(now, taken, &p);
        let horizon = now + p.lookahead as f64 * TICKS_PER_SECOND as f64;
        while clock.tick_of_beat(next_beat as f64) <= horizon {
            let beat_tick = clock.tick_of_beat(next_beat as f64);
            let at = sync
                .sample_for_tick(beat_tick, p.latency_compensation)
                .unwrap();
            mixer.schedule(Scheduled {
                at,
                samples: click.clone(),
                gain: 1.0,
            });
            scheduled.push((beat_tick, at));
            next_beat += 1;
        }
    }
    // Skip the first second (the smoothing settling) and a second after a
    // hitch (the sim catching up can't be heard in the past).
    let settled = |t: f64| t > 1.0 && hitch_at.is_none_or(|h| t < h || t > h + 1.5);
    let mut worst = 0.0f64;
    for &(beat_tick, at) in &scheduled {
        let sim_time = beat_tick * tick_dt;
        if !settled(sim_time) {
            continue;
        }
        // The speaker plays sample `at` when the device clock reaches it.
        let heard = at as f64 / (RATE as f64 * drift);
        worst = worst.max((heard - sim_time).abs());
    }
    (worst, mixer.late, scheduled.len())
}

#[test]
fn clicks_stay_in_sync_over_ten_minutes() {
    // A device 0.05% fast drifts 0.3 s over 10 minutes if left alone.
    let (worst, late, clicks) = run(600.0, 1.0005, 512, 120.0, None);
    assert!(clicks >= 1199, "{clicks} clicks");
    assert_eq!(late, 0, "every click was scheduled ahead of the device");
    assert!(worst < 0.012, "worst click error {:.1} ms", worst * 1000.0);
}

#[test]
fn slow_devices_and_fast_tempos_keep_sync_too() {
    let (worst, late, _) = run(600.0, 0.9995, 1024, 260.0, None);
    assert_eq!(late, 0);
    assert!(worst < 0.015, "worst click error {:.1} ms", worst * 1000.0);
}

#[test]
fn a_hitch_resyncs_instead_of_drifting() {
    let (worst, _, _) = run(120.0, 1.0, 512, 120.0, Some(30.0));
    assert!(worst < 0.012, "worst click error {:.1} ms", worst * 1000.0);
}

#[test]
fn mixer_starts_notes_on_their_sample() {
    let mut m = Mixer::new(4);
    m.schedule(Scheduled {
        at: 3,
        samples: vec![0.5, 0.25].into(),
        gain: 2.0,
    });
    let out: Vec<f32> = (0..6).map(|_| m.next_sample()).collect();
    assert_eq!(out, vec![0.0, 0.0, 0.0, 1.0, 0.5, 0.0]);
    assert_eq!(m.late, 0);
}

#[test]
fn mixer_counts_late_and_dropped_notes() {
    let mut m = Mixer::new(1);
    for _ in 0..3 {
        m.next_sample();
    }
    let note = |at| Scheduled {
        at,
        samples: vec![1.0; 10].into(),
        gain: 1.0,
    };
    m.schedule(note(0));
    m.schedule(note(4));
    for _ in 0..3 {
        m.next_sample();
    }
    assert_eq!(m.late, 1);
    assert_eq!(m.dropped, 1, "the second note pushed out the first");
}

#[test]
fn latency_meter_estimates_from_bursts() {
    let p = StreamParams::default();
    let mut meter = LatencyMeter::default();
    for _ in 0..10 {
        meter.burst(441);
    }
    meter.burst(0);
    assert_eq!(meter.bursts(), 10);
    // 10 ms bursts, two in flight.
    assert!((meter.latency(RATE, &p) - 0.02).abs() < 1e-6);
    assert!(meter.summary(RATE, &p).contains("20.0 ms"));
}

#[test]
fn the_timeline_runs_through_a_frozen_sim_and_picks_it_up_again() {
    use tarborn_music::stream::Timeline;
    let mut t = Timeline::default();
    // Running: frames of one tick follow the sim exactly.
    for tick in 0..100u64 {
        assert!(!t.advance(tick, 0.0, 1.0, 3.0));
    }
    assert_eq!(t.now(), 99.0);
    // Frozen at 99 for 50 ticks of wall time: music time keeps going.
    for _ in 0..50 {
        assert!(!t.advance(99, 0.0, 1.0, 3.0));
    }
    let frozen = t.now();
    assert!((frozen - 149.0).abs() < 1e-9, "{frozen}");
    // Resumes at 100: re-anchored, no jump.
    assert!(t.advance(100, 0.0, 1.0, 3.0));
    assert!((t.now() - (frozen + 1.0)).abs() < 1e-9);
    assert!((100.0 + t.offset() - t.now()).abs() < 1e-9);
    // A new room restarts the sim at 0: still no jump.
    let before = t.now();
    assert!(t.advance(0, 0.0, 1.0, 3.0));
    assert!((t.now() - (before + 1.0)).abs() < 1e-9);
    // Time never runs backwards, even as the overstep wraps.
    let mut last = t.now();
    for i in 0..40 {
        t.advance(i / 2, (i % 2) as f64 * 0.5, 0.5, 3.0);
        assert!(t.now() >= last);
        last = t.now();
    }
}

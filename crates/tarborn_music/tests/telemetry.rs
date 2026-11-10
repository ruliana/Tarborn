//! Run logs round-trip through RON, and the summary reads the right things
//! out of them.

use tarborn_music::telemetry::{
    BarLog, HitLog, MuteLog, Rating, RunLog, TelemetryParams, correlation, summarize,
};

fn bar(t: f32, style: &str, tune: u32, layer: u8, intensity: f32) -> BarLog {
    BarLog {
        t,
        style: style.into(),
        tune,
        layer,
        intensity,
        ..Default::default()
    }
}

fn run() -> RunLog {
    // Tune 1, then 2, then 1 again: one repeat in three starts. The layer
    // follows the intensity.
    let mut bars = Vec::new();
    for i in 0..30 {
        let tune = [1, 2, 1][i / 10];
        let intensity = (i % 10) as f32 / 10.0;
        bars.push(bar(
            i as f32,
            "industrial",
            tune,
            (intensity * 3.0) as u8,
            intensity,
        ));
    }
    RunLog {
        started: 1,
        bars,
        hits: vec![
            HitLog {
                t: 1.0,
                name: "jump".into(),
                from_beat: 0.05,
                band_near: 2,
            },
            HitLog {
                t: 2.0,
                name: "slam".into(),
                from_beat: -0.4,
                band_near: 0,
            },
        ],
        mutes: vec![
            MuteLog {
                t: 10.0,
                muted: true,
            },
            MuteLog {
                t: 13.0,
                muted: false,
            },
        ],
        ratings: vec![Rating {
            t: 29.0,
            stage: 0,
            style: "industrial".into(),
            rating: 1,
        }],
    }
}

#[test]
fn a_log_round_trips_through_ron() {
    let log = run();
    let text = ron::ser::to_string_pretty(&log, Default::default()).unwrap();
    let back: RunLog = tarborn_music::from_ron(&text).unwrap();
    assert_eq!(back, log);
}

#[test]
fn the_summary_reads_the_logs() {
    let p = TelemetryParams::default();
    let s = summarize(&[run(), run()], &p);
    assert_eq!(s.runs, 2);
    let ind = &s.styles["industrial"];
    assert_eq!(ind.bars, 60);
    assert!(
        ind.tension_follows_play > 0.9,
        "{}",
        ind.tension_follows_play
    );
    assert_eq!(ind.ratings, 2);
    assert_eq!(ind.rating, 1.0);
    assert_eq!((s.tune_starts, s.tune_repeats), (6, 2));
    assert_eq!(s.hits, 4);
    assert!((s.on_beat - 0.5).abs() < 1e-6);
    assert!((s.hit_offset - 0.225).abs() < 1e-6);
    assert!((s.masking - 1.0).abs() < 1e-6);
    assert_eq!(s.mutes, 2);
    assert!(
        (s.muted_share - 3.0 / 29.0).abs() < 1e-4,
        "{}",
        s.muted_share
    );
    let text = s.to_string();
    assert!(
        text.contains("industrial") && text.contains("2 of 6"),
        "{text}"
    );
}

#[test]
fn muting_until_the_end_counts() {
    let log = RunLog {
        bars: vec![
            bar(0.0, "forest", 1, 0, 0.0),
            bar(20.0, "forest", 1, 0, 0.0),
        ],
        mutes: vec![MuteLog {
            t: 5.0,
            muted: true,
        }],
        ..Default::default()
    };
    assert_eq!(log.muted_seconds(20.0), 15.0);
}

#[test]
fn correlation_is_pearson() {
    assert!((correlation(&[(0.0, 0.0), (1.0, 2.0), (2.0, 4.0)]) - 1.0).abs() < 1e-6);
    assert!((correlation(&[(0.0, 2.0), (1.0, 1.0), (2.0, 0.0)]) + 1.0).abs() < 1e-6);
    assert_eq!(correlation(&[(0.0, 1.0), (1.0, 1.0)]), 0.0);
}

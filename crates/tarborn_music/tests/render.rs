use tarborn_music::score::midi_to_hz;
use tarborn_music::time::{beats_to_seconds, swing};
use tarborn_music::{
    Fragment, MusicFile, Note, PatchBank, RenderSettings, from_ron, render, to_ron,
};
use tarborn_sfx::wav;

const FIXTURE: &str = include_str!("fixtures/two_bars.ron");
const PATCHES: &str = include_str!("../../tarborn/assets/music/patches.ron");
const MUSIC: &str = include_str!("../../tarborn/assets/music.ron");

fn fixture() -> Fragment {
    from_ron(FIXTURE).expect("fixture parses")
}

fn bank() -> PatchBank {
    from_ron(PATCHES).expect("patches.ron parses")
}

fn settings() -> RenderSettings {
    from_ron::<MusicFile>(MUSIC)
        .expect("music.ron parses")
        .render
}

#[test]
fn same_seed_renders_bit_identical_wav() {
    let (f, b, s) = (fixture(), bank(), settings());
    let a = wav::encode(&render(&f, &b, &s, 42).unwrap(), s.sample_rate);
    let c = wav::encode(&render(&f, &b, &s, 42).unwrap(), s.sample_rate);
    assert_eq!(a, c);
    assert!(a.len() > 44, "the render is not empty");
}

#[test]
fn different_seeds_humanise_differently() {
    let (f, b, s) = (fixture(), bank(), settings());
    assert!(s.timing_jitter > 0.0 || s.velocity_jitter > 0.0);
    assert_ne!(
        render(&f, &b, &s, 1).unwrap(),
        render(&f, &b, &s, 2).unwrap()
    );
}

#[test]
fn without_jitter_the_seed_does_not_matter() {
    let (f, b) = (fixture(), bank());
    let s = RenderSettings {
        timing_jitter: 0.0,
        velocity_jitter: 0.0,
        ..settings()
    };
    assert_eq!(
        render(&f, &b, &s, 1).unwrap(),
        render(&f, &b, &s, 99).unwrap()
    );
}

#[test]
fn render_length_follows_tempo_and_tail() {
    let (f, b, s) = (fixture(), bank(), settings());
    let samples = render(&f, &b, &s, 1).unwrap();
    let expected = (8.0 * 60.0 / s.bpm + s.tail) * s.sample_rate as f32;
    assert_eq!(samples.len(), expected.ceil() as usize);
    assert!(samples.iter().all(|x| (-1.0..=1.0).contains(x)));
    assert!(samples.iter().any(|x| x.abs() > 0.05), "audible");
}

#[test]
fn notes_start_at_their_swung_time() {
    let b = bank();
    let s = RenderSettings {
        timing_jitter: 0.0,
        velocity_jitter: 0.0,
        tail: 0.0,
        ..settings()
    };
    let note = |start| Fragment {
        notes: vec![Note {
            start,
            patch: "ride".into(),
            ..Note::default()
        }],
        ..Fragment::default()
    };
    let samples = render(&note(1.5), &b, &s, 1).unwrap();
    let first = samples.iter().position(|x| *x != 0.0).unwrap();
    let expected = (beats_to_seconds(1.5, s.bpm, s.swing) * s.sample_rate as f32).round() as usize;
    assert_eq!(first, expected);
}

#[test]
fn unknown_patch_is_an_error() {
    let f = Fragment {
        notes: vec![Note {
            patch: "kazoo".into(),
            ..Note::default()
        }],
        ..Fragment::default()
    };
    assert!(render(&f, &bank(), &settings(), 1).is_err());
}

#[test]
fn pitched_patches_follow_the_note() {
    let b = bank();
    let piano = &b.patches["piano"];
    let up = piano.voiced(piano.root + 12.0, 0.5, 440.0);
    let ratio = up.layers[0].freq / piano.sound.layers[0].freq;
    assert!(
        (ratio - 2.0).abs() < 1e-4,
        "an octave up doubles the pitch: {ratio}"
    );
    let ride = &b.patches["ride"];
    assert_eq!(
        ride.voiced(90.0, 0.5, 440.0),
        ride.sound,
        "unpitched patches stay put"
    );
}

#[test]
fn swing_keeps_downbeats_and_order() {
    for ratio in [0.5, 0.58, 0.62, 0.67] {
        assert_eq!(swing(3.0, ratio), 3.0);
        assert!((swing(2.5, ratio) - (2.0 + ratio)).abs() < 1e-6);
        let mut last = -1.0;
        for i in 0..400 {
            let t = swing(i as f32 / 100.0, ratio);
            assert!(t > last);
            last = t;
        }
    }
    assert!((swing(1.25, 0.5) - 1.25).abs() < 1e-6, "0.5 is straight");
}

#[test]
fn a4_is_440() {
    assert!((midi_to_hz(69.0, 440.0) - 440.0).abs() < 1e-3);
    assert!((midi_to_hz(57.0, 440.0) - 220.0).abs() < 1e-3);
}

#[test]
fn fragments_round_trip_through_ron() {
    let f = fixture();
    let text = to_ron(&f).unwrap();
    assert_eq!(from_ron::<Fragment>(&text).unwrap(), f);
}

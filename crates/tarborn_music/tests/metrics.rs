//! The metrics must rank hand-checked good and bad examples correctly.

use tarborn_music::metrics::{
    MetricParams, density, dissonance, repetition, score, syncopation, tension, voice_leading,
};
use tarborn_music::score::{ChordSpan, Fragment, Note, Role};
use tarborn_music::style::Style;
use tarborn_music::theory::Chord;
use tarborn_music::{MusicFile, from_ron};

const INDUSTRIAL: &str = include_str!("../../tarborn/assets/music/styles/industrial.ron");
const MUSIC: &str = include_str!("../../tarborn/assets/music.ron");

fn style() -> Style {
    from_ron(INDUSTRIAL).unwrap()
}

fn params() -> MetricParams {
    from_ron::<MusicFile>(MUSIC).unwrap().metrics
}

/// ii-V-I in C, one bar each, then a bar of I.
fn changes() -> Vec<ChordSpan> {
    ["Dm7", "G7", "Cmaj7", "Cmaj7"]
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let chord = Chord::parse(s).unwrap();
            ChordSpan {
                start: i as f32 * 4.0,
                length: 4.0,
                chord,
                mode: chord.quality.mode(),
            }
        })
        .collect()
}

fn fragment(role: Role, layer: u8, notes: Vec<(f32, f32, u8)>, patch: &str) -> Fragment {
    Fragment {
        role,
        layer,
        bars: 4,
        beats_per_bar: 4,
        style: "industrial".into(),
        chords: changes(),
        notes: notes
            .into_iter()
            .map(|(start, length, pitch)| Note {
                start,
                length,
                pitch,
                velocity: 0.8,
                patch: patch.into(),
            })
            .collect(),
        ..Fragment::default()
    }
}

fn chords(role: Role, voicings: &[(f32, &[u8])]) -> Fragment {
    let mut notes = Vec::new();
    for &(t, v) in voicings {
        for &p in v {
            notes.push((t, 1.0, p));
        }
    }
    fragment(role, 1, notes, "piano")
}

#[test]
fn a_walking_line_beats_random_leaps() {
    let (s, p) = (style(), params());
    // D F A C# | G B D Db | C E G B | C A G E: chord tones, approach notes.
    let good = [
        38, 41, 45, 44, 43, 47, 50, 37, 36, 40, 43, 47, 48, 45, 43, 40,
    ];
    // Leaps to notes outside every chord.
    let bad = [
        38, 51, 30, 54, 42, 29, 53, 39, 30, 54, 37, 52, 31, 49, 34, 53,
    ];
    let line = |ps: &[u8]| {
        fragment(
            Role::Bass,
            1,
            ps.iter()
                .enumerate()
                .map(|(i, &p)| (i as f32, 1.0, p))
                .collect(),
            "bass",
        )
    };
    let (g, b) = (line(&good), line(&bad));
    assert!(tension(&g, &s, &p) < tension(&b, &s, &p));
    assert!(score(&g, &s, &p).total > score(&b, &s, &p).total);
}

#[test]
fn smooth_rootless_comping_beats_jumpy_clusters() {
    let (s, p) = (style(), params());
    // Dm9 -> G13 -> Cmaj9, rootless A/B/A: one or two semitones each.
    let good = chords(
        Role::Comp,
        &[
            (1.5, &[65, 69, 72, 76]),
            (5.5, &[65, 69, 71, 76]),
            (9.5, &[64, 67, 71, 74]),
        ],
    );
    // Clusters that jump an octave every time.
    let bad = chords(
        Role::Comp,
        &[
            (1.5, &[60, 61, 62, 63]),
            (5.5, &[72, 73, 74, 75]),
            (9.5, &[58, 59, 60, 61]),
        ],
    );
    assert!(dissonance(&good, &s, &p) < dissonance(&bad, &s, &p));
    assert!(voice_leading(&good, &s) < voice_leading(&bad, &s));
    assert!(score(&good, &s, &p).total > score(&bad, &s, &p).total);
}

#[test]
fn a_developing_motif_beats_a_loop_and_a_wander() {
    let (s, p) = (style(), params());
    // A three-note cell sequenced down the changes, with a varied ending.
    let motif = vec![
        (0.0, 0.5, 65),
        (0.5, 0.5, 69),
        (1.5, 1.0, 72),
        (4.0, 0.5, 65),
        (4.5, 0.5, 67),
        (5.5, 1.0, 71),
        (8.0, 0.5, 64),
        (8.5, 0.5, 67),
        (9.5, 1.0, 71),
        (12.0, 0.5, 64),
        (12.5, 0.5, 67),
        (13.5, 1.0, 72),
        (14.5, 1.0, 76),
    ];
    // One note over and over.
    let looped: Vec<(f32, f32, u8)> = (0..32).map(|i| (i as f32 * 0.5, 0.5, 72)).collect();
    // Random notes, nothing comes back.
    let wander = vec![
        (0.0, 0.5, 62),
        (1.0, 0.5, 75),
        (1.5, 0.5, 66),
        (3.0, 1.0, 79),
        (4.5, 0.5, 61),
        (6.0, 0.5, 70),
        (7.5, 0.5, 64),
        (8.0, 1.0, 77),
        (10.5, 0.5, 63),
        (11.0, 0.5, 73),
        (13.0, 0.5, 68),
        (15.5, 0.5, 81),
    ];
    let lead = |n| fragment(Role::Lead, 2, n, "horn");
    let (m, l, w) = (lead(motif), lead(looped), lead(wander));
    let r = |f: &Fragment| repetition(f, &p);
    assert!(
        r(&l) > r(&m) && r(&m) > r(&w),
        "{} {} {}",
        r(&l),
        r(&m),
        r(&w)
    );
    let total = |f: &Fragment| score(f, &s, &p).total;
    assert!(total(&m) > total(&l), "{} vs loop {}", total(&m), total(&l));
    assert!(
        total(&m) > total(&w),
        "{} vs wander {}",
        total(&m),
        total(&w)
    );
}

#[test]
fn tension_rises_with_chromatic_clouds() {
    let (s, p) = (style(), params());
    let held = |ps: &[u8]| chords(Role::Comp, &[(0.0, ps), (4.0, ps), (8.0, ps), (12.0, ps)]);
    // A C major triad against the progression versus a chromatic cluster.
    let triad = held(&[60, 64, 67]);
    let cluster = held(&[60, 61, 66, 70]);
    assert!(tension(&triad, &s, &p) < tension(&cluster, &s, &p));
}

#[test]
fn syncopation_counts_offbeats_against_silent_strong_beats() {
    let on: Vec<(f32, f32, u8)> = (0..16).map(|i| (i as f32, 0.5, 60)).collect();
    let off: Vec<(f32, f32, u8)> = (0..16).map(|i| (i as f32 + 0.5, 0.5, 60)).collect();
    let f = |n| fragment(Role::Comp, 1, n, "piano");
    assert_eq!(syncopation(&f(on)), 0.0);
    assert!(syncopation(&f(off)) > 0.0);
}

#[test]
fn dissonance_ranks_intervals() {
    let (s, p) = (style(), params());
    let dyad = |a, b| chords(Role::Comp, &[(0.0, &[a, b])]);
    let d = |f: &Fragment| dissonance(f, &s, &p);
    assert!(d(&dyad(60, 61)) > d(&dyad(60, 66)));
    assert!(d(&dyad(60, 66)) > d(&dyad(60, 64)));
    assert!(d(&dyad(60, 64)) > d(&dyad(60, 67)));
}

#[test]
fn density_is_onsets_per_beat() {
    let f = fragment(
        Role::Lead,
        2,
        vec![(0.0, 1.0, 60), (0.0, 1.0, 64), (2.0, 1.0, 62)],
        "horn",
    );
    assert!((density(&f) - 2.0 / 16.0).abs() < 1e-6);
}

#[test]
fn the_grammar_mostly_lands_on_target() {
    let p = params();
    for text in [
        INDUSTRIAL,
        include_str!("../../tarborn/assets/music/styles/forest.ron"),
    ] {
        let s: Style = from_ron(text).unwrap();
        let mut scores = Vec::new();
        for seed in 0..50 {
            let t = tarborn_music::grammar::tune(&s, seed);
            for f in tarborn_music::grammar::fragments(&s, &t, seed) {
                if !f.notes.is_empty() {
                    scores.push(score(&f, &s, &p).total);
                }
            }
        }
        let mean = scores.iter().sum::<f32>() / scores.len() as f32;
        assert!(mean > 0.85, "{}: mean score {mean}", s.name);
    }
}

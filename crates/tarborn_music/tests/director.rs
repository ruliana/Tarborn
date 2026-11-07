//! Scripted intensity sequences must give the expected layer changes, and
//! runs with different seeds must differ.

use std::collections::BTreeSet;
use std::sync::Arc;
use tarborn_music::director::{Bar, Director, DirectorParams, Input, Voicer};
use tarborn_music::search::LibraryTune;
use tarborn_music::style::Style;
use tarborn_music::{MusicFile, Note, PatchBank, from_ron};

const MUSIC: &str = include_str!("../../tarborn/assets/music.ron");
const PATCHES: &str = include_str!("../../tarborn/assets/music/patches.ron");
const STYLES: [(&str, &str); 2] = [
    (
        include_str!("../../tarborn/assets/music/styles/industrial.ron"),
        include_str!("../../tarborn/assets/music/library/industrial.ron"),
    ),
    (
        include_str!("../../tarborn/assets/music/styles/forest.ron"),
        include_str!("../../tarborn/assets/music/library/forest.ron"),
    ),
];

fn params() -> DirectorParams {
    from_ron::<MusicFile>(MUSIC).unwrap().director
}

fn director(which: usize, params: DirectorParams, seed: u32) -> Director {
    let (style, library) = STYLES[which];
    let style: Style = from_ron(style).unwrap();
    let library: Vec<LibraryTune> = from_ron(library).unwrap();
    Director::new(style, library, params, seed).expect("a library")
}

fn play(d: &mut Director, script: &[(u32, f32)]) -> Vec<Bar> {
    script
        .iter()
        .flat_map(|&(bars, intensity)| (0..bars).map(move |_| intensity))
        .map(|intensity| {
            d.next_bar(Input {
                intensity,
                duck: 0.0,
            })
        })
        .collect()
}

fn calm(mut p: DirectorParams) -> DirectorParams {
    p.breather_after = 0;
    p
}

#[test]
fn layers_follow_intensity_rising_at_once_and_falling_slowly() {
    let p = calm(params());
    for which in 0..STYLES.len() {
        let mut d = director(which, p.clone(), 7);
        let bars = play(&mut d, &[(8, 0.0), (4, 0.5), (4, 1.0), (16, 0.0)]);
        let layers: Vec<u8> = bars.iter().map(|b| b.layer).collect();
        assert!(layers[..8].iter().all(|&l| l == 0), "{layers:?}");
        assert!(layers[8..12].iter().all(|&l| l == 1), "{layers:?}");
        assert!(layers[12..16].iter().all(|&l| l == 2), "{layers:?}");
        // Drops one layer per `fall_bars` of low intensity.
        let f = p.fall_bars as usize;
        assert!(layers[16..16 + f - 1].iter().all(|&l| l == 2), "{layers:?}");
        assert_eq!(layers[16 + f - 1], 1, "{layers:?}");
        assert_eq!(layers[16 + 2 * f - 1], 0, "{layers:?}");
        // Never more than one layer down per bar.
        for w in layers.windows(2) {
            assert!(w[1] + 1 >= w[0], "{layers:?}");
        }
    }
}

#[test]
fn bars_hold_notes_inside_the_bar_and_the_full_band_plays() {
    for which in 0..STYLES.len() {
        let mut d = director(which, params(), 3);
        let bpb = d.beats_per_bar() as f32;
        for bar in play(&mut d, &[(64, 1.0)]) {
            assert!(bar.notes.iter().all(|n| n.start >= 0.0 && n.start < bpb));
            assert!(!bar.notes.is_empty(), "bar {} is silent", bar.index);
        }
        // Calm bars still have the bass and the ride.
        let mut d = director(which, calm(params()), 3);
        let calm_bars = play(&mut d, &[(32, 0.0)]);
        let notes: usize = calm_bars.iter().map(|b| b.notes.len()).sum();
        assert!(notes > 32, "{notes} notes in 32 calm bars");
    }
}

#[test]
fn a_long_calm_spell_gets_a_breather() {
    let p = params();
    assert!(p.breather_after > 0);
    let mut d = director(1, p.clone(), 5);
    let bars = play(&mut d, &[(p.breather_after + p.breather_bars + 4, 0.0)]);
    let lifted: Vec<u64> = bars
        .iter()
        .filter(|b| b.breather)
        .map(|b| b.index)
        .collect();
    assert_eq!(lifted.len(), p.breather_bars as usize, "{lifted:?}");
    assert!(bars.iter().filter(|b| b.breather).all(|b| b.layer == 1));
    assert!(
        bars[..p.breather_after as usize]
            .iter()
            .all(|b| !b.breather)
    );
}

#[test]
fn the_music_keeps_moving() {
    for which in 0..STYLES.len() {
        let mut d = director(which, calm(params()), 11);
        let bars = play(&mut d, &[(400, 0.6)]);
        let tunes: BTreeSet<u32> = bars.iter().map(|b| b.tune).collect();
        assert!(tunes.len() >= 2, "only {tunes:?} in 400 bars");
        // Bars at the same place in the same tune are not always identical.
        let key = |b: &Bar| (b.tune, b.section, b.bar_in_section);
        let mut differs = false;
        for (i, a) in bars.iter().enumerate() {
            if let Some(b) = bars[i + 1..].iter().find(|b| key(b) == key(a)) {
                differs |= a.notes != b.notes;
            }
        }
        assert!(differs, "variants never change");
    }
}

#[test]
fn seeds_replay_exactly_and_different_seeds_differ() {
    let script = [(16, 0.0), (16, 0.8), (16, 0.4)];
    for which in 0..STYLES.len() {
        let a = play(&mut director(which, params(), 1), &script);
        let b = play(&mut director(which, params(), 1), &script);
        let c = play(&mut director(which, params(), 2), &script);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}

#[test]
fn the_band_drummer_ducks() {
    let mut loud = director(0, params(), 9);
    let mut ducked = director(0, params(), 9);
    let drums = |b: Bar| -> f32 {
        b.notes
            .iter()
            .filter(|n| ["ride", "hat", "brushes"].contains(&n.patch.as_str()))
            .map(|n| n.velocity)
            .sum()
    };
    let (mut a, mut b) = (0.0, 0.0);
    for _ in 0..16 {
        a += drums(loud.next_bar(Input {
            intensity: 1.0,
            duck: 0.0,
        }));
        b += drums(ducked.next_bar(Input {
            intensity: 1.0,
            duck: 1.0,
        }));
    }
    let depth = params().duck_depth;
    assert!((b / a - (1.0 - depth)).abs() < 1e-3, "{a} -> {b}");
}

#[test]
fn the_voicer_swings_and_reuses_renders() {
    let bank: PatchBank = from_ron(PATCHES).unwrap();
    let mut settings = from_ron::<MusicFile>(MUSIC).unwrap().render;
    settings.timing_jitter = 0.0;
    settings.velocity_jitter = 0.0;
    let mut v = Voicer::new(bank, settings, 1);
    let note = |start| Note {
        start,
        length: 0.5,
        pitch: 48,
        velocity: 0.8,
        patch: "bass".into(),
    };
    let on = v.voice(&note(0.0), 8.0, 200.0, 0.62).unwrap();
    let off = v.voice(&note(0.5), 8.0, 200.0, 0.62).unwrap();
    assert_eq!(on.beat, 8.0);
    assert!((off.beat - 8.62).abs() < 1e-5, "{}", off.beat);
    assert!(on.gain > 0.0 && !on.samples.is_empty());
    assert!(Arc::ptr_eq(&on.samples, &off.samples));
    let unknown = Note {
        patch: "kazoo".into(),
        ..note(0.0)
    };
    assert!(v.voice(&unknown, 0.0, 200.0, 0.62).is_none());
}

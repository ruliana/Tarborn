//! The player's kit: hits per move, tuned to the harmony, ducking, fills
//! and section jumps; and the director's answers.

use tarborn_music::director::{Director, Input};
use tarborn_music::drummer::{Cue, Drummer, Moment};
use tarborn_music::search::LibraryTune;
use tarborn_music::style::Style;
use tarborn_music::theory::{Chord, Pc};
use tarborn_music::{MusicFile, from_ron};

const MUSIC: &str = include_str!("../../tarborn/assets/music.ron");
const INDUSTRIAL: &str = include_str!("../../tarborn/assets/music/styles/industrial.ron");
const FOREST: &str = include_str!("../../tarborn/assets/music/styles/forest.ron");
const LIBRARY: &str = include_str!("../../tarborn/assets/music/library/industrial.ron");
const PATCHES: &str = include_str!("../../tarborn/assets/music/patches.ron");

fn style(text: &str) -> Style {
    from_ron(text).unwrap()
}

fn at(key: Pc, chord: Option<&Chord>) -> Moment<'_> {
    Moment {
        key,
        chord,
        from_bar_line: 1.5,
    }
}

#[test]
fn every_kit_hit_names_a_real_patch() {
    let bank: tarborn_music::PatchBank = from_ron(PATCHES).unwrap();
    for text in [INDUSTRIAL, FOREST] {
        let s = style(text);
        assert!(!s.kit.hits.is_empty(), "{} has no kit", s.name);
        for (name, hit) in &s.kit.hits {
            assert!(
                bank.patches.contains_key(&hit.patch),
                "{}: {name} -> {}",
                s.name,
                hit.patch
            );
        }
    }
}

#[test]
fn moves_play_their_hits_in_key() {
    let mut d = Drummer::new(style(INDUSTRIAL).kit);
    let (c, _) = Pc::parse("C").unwrap();
    let (kick, _) = d.hear("jump", 1.0, at(c, None));
    let kick = kick.unwrap();
    assert_eq!(kick.patch, "ind_kick");
    assert_eq!(Pc::of(kick.pitch), c, "the kick sounds the tonic");
    assert_eq!(
        d.hear("hit", 1.0, at(c, None)).0.unwrap().patch,
        "ind_snare"
    );
    assert_eq!(
        d.hear("kill", 1.0, at(c, None)).0.unwrap().patch,
        "ind_crash"
    );
    assert!(d.hear("slam_started", 1.0, at(c, None)).0.is_none());
}

#[test]
fn accents_walk_the_chord() {
    let mut d = Drummer::new(style(FOREST).kit);
    let chord = Chord::parse("Dm7").unwrap();
    let (key, _) = Pc::parse("D").unwrap();
    let tones = chord.tones();
    let mut heard = Vec::new();
    for _ in 0..8 {
        let s = d.hear("pogo", 1.0, at(key, Some(&chord))).0.unwrap();
        assert!(tones.contains(&Pc::of(s.pitch)), "{} not in Dm7", s.pitch);
        heard.push(Pc::of(s.pitch));
    }
    assert_eq!(heard[..4], tones[..], "one tone further each hit");
}

#[test]
fn weak_moves_play_softer() {
    let mut d = Drummer::new(style(INDUSTRIAL).kit);
    let (c, _) = Pc::parse("C").unwrap();
    let full = d.hear("launch", 1.0, at(c, None)).0.unwrap().velocity;
    let weak = d.hear("launch", 0.0, at(c, None)).0.unwrap().velocity;
    assert!(weak < full * 0.7, "{full} -> {weak}");
}

#[test]
fn busy_players_duck_the_band_and_get_a_fill() {
    let kit = style(INDUSTRIAL).kit;
    let mut d = Drummer::new(kit.clone());
    let (c, _) = Pc::parse("C").unwrap();
    assert_eq!(d.duck(), 0.0);
    let mut fills = 0;
    for _ in 0..8 {
        let (_, cues) = d.hear("hit", 1.0, at(c, None));
        fills += cues.iter().filter(|&&q| q == Cue::Fill).count();
        d.advance(0.1);
    }
    assert_eq!(fills, 1, "one fill per burst, then a cooldown");
    assert!(d.duck() >= 1.0, "{}", d.duck());
    d.advance(kit.duck_window + 0.1);
    assert_eq!(d.duck(), 0.0, "the band comes back when the player rests");
}

#[test]
fn a_slam_on_the_bar_line_moves_the_tune_on() {
    let kit = style(INDUSTRIAL).kit;
    let mut d = Drummer::new(kit.clone());
    let (c, _) = Pc::parse("C").unwrap();
    for (from_bar_line, jumps) in [(0.1, true), (-0.2, true), (1.0, false)] {
        let (_, cues) = d.hear(
            "slam",
            1.0,
            Moment {
                key: c,
                chord: None,
                from_bar_line,
            },
        );
        assert_eq!(cues.contains(&Cue::NextSection), jumps, "{from_bar_line}");
    }
}

fn director() -> Director {
    let params = from_ron::<MusicFile>(MUSIC).unwrap().director;
    let library: Vec<LibraryTune> = from_ron(LIBRARY).unwrap();
    Director::new(style(INDUSTRIAL), library, params, 4).unwrap()
}

#[test]
fn the_band_answers_with_a_fill() {
    let mut d = director();
    let input = Input {
        intensity: 1.0,
        duck: 0.0,
    };
    let plain = d.next_bar(input);
    assert!(!plain.fill);
    d.request_fill();
    let fill = d.next_bar(input);
    assert!(fill.fill);
    let p = from_ron::<MusicFile>(MUSIC).unwrap().director;
    let hats = |b: &tarborn_music::director::Bar| {
        b.notes
            .iter()
            .filter(|n| n.patch == "hat" && n.start >= 4.0 - p.fill_beats)
            .count()
    };
    assert!(hats(&fill) >= p.fill_notes as usize, "{}", hats(&fill));
    assert!(!d.next_bar(input).fill, "a fill plays once");
}

#[test]
fn a_section_jump_starts_the_next_section() {
    let mut d = director();
    let input = Input::default();
    let first = d.next_bar(input);
    assert_eq!(first.bar_in_section, 0);
    d.next_bar(input);
    d.request_next_section();
    let jumped = d.next_bar(input);
    assert_eq!(jumped.bar_in_section, 0, "{jumped:?}");
    assert!(!jumped.chords.is_empty());
}

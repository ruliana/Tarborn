//! Each screen gets its piece from the library, and pieces hand over on bar
//! lines with bar-by-bar fades.

use tarborn_music::pieces::{Fade, PieceKind, PiecePlayer, ScreenParams, piece};
use tarborn_music::score::Role;
use tarborn_music::search::LibraryTune;
use tarborn_music::style::Style;
use tarborn_music::{MusicFile, from_ron};
use tarborn_sim::TICKS_PER_SECOND;

const MUSIC: &str = include_str!("../../tarborn/assets/music.ron");
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

fn screens() -> ScreenParams {
    from_ron::<MusicFile>(MUSIC).unwrap().screens
}

fn style(which: usize) -> (Style, Vec<LibraryTune>) {
    let (s, l) = STYLES[which];
    (from_ron(s).unwrap(), from_ron(l).unwrap())
}

const KINDS: [PieceKind; 3] = [PieceKind::Intro, PieceKind::Stinger, PieceKind::Ballad];

#[test]
fn every_style_has_every_piece_for_every_tune() {
    let p = screens();
    for which in 0..STYLES.len() {
        let (style, library) = style(which);
        for seed in 0..library.len() as u32 {
            for kind in KINDS {
                let piece = piece(&style, &library, kind, &p, seed)
                    .unwrap_or_else(|| panic!("{}: no {kind:?} for tune {seed}", style.name));
                let notes: usize = piece.bars.iter().map(Vec::len).sum();
                assert!(notes > 0);
                for bar in &piece.bars {
                    for n in bar {
                        assert!((0.0..piece.beats_per_bar as f32).contains(&n.start));
                    }
                }
                let params = p.of(kind);
                if params.max_bars > 0 {
                    assert!(piece.bars.len() as u32 <= params.max_bars);
                }
                assert_eq!(piece.looped, params.looped);
            }
        }
    }
}

#[test]
fn the_ballad_is_slower_and_softer_and_has_no_drums() {
    let p = screens();
    for which in 0..STYLES.len() {
        let (style, library) = style(which);
        let intro = piece(&style, &library, PieceKind::Intro, &p, 0).unwrap();
        let ballad = piece(&style, &library, PieceKind::Ballad, &p, 0).unwrap();
        assert!(ballad.bpm < intro.bpm, "{} vs {}", ballad.bpm, intro.bpm);
        assert!(!ballad.looped);
        let drums = &style.drums;
        let kit = [&drums.ride, &drums.hat, &drums.brushes];
        for n in ballad.bars.iter().flatten() {
            assert!(
                !kit.contains(&&n.patch),
                "drum note {} in the ballad",
                n.patch
            );
        }
        assert!(p.ballad.roles.iter().all(|r| *r != Role::Drums));
    }
}

#[test]
fn fades_step_bar_by_bar() {
    let out = Fade::steady(1.0).towards(10, 2, 0.0);
    let gains: Vec<f32> = (8..14).map(|b| out.gain(b)).collect();
    assert_eq!(gains[..2], [1.0, 1.0]);
    assert!((gains[2] - 2.0 / 3.0).abs() < 1e-6);
    assert!((gains[3] - 1.0 / 3.0).abs() < 1e-6);
    assert_eq!(gains[4..], [0.0, 0.0]);
    assert!(!out.silent_from(11) && out.silent_from(12));
    // A fade that starts halfway through another starts from where it was.
    let back = out.towards(11, 1, 1.0);
    assert!((back.gain(10) - 2.0 / 3.0).abs() < 1e-6);
    assert!((back.gain(11) - (2.0 / 3.0 + 1.0) / 2.0).abs() < 1e-6);
    assert_eq!(back.gain(12), 1.0);
    // No bars: at once.
    assert_eq!(Fade::steady(0.0).towards(3, 0, 1.0).gain(3), 1.0);
}

#[test]
fn a_piece_plays_on_its_bar_lines_and_hands_over_on_one() {
    let p = screens();
    let (style, library) = style(0);
    let intro = piece(&style, &library, PieceKind::Intro, &p, 1).unwrap();
    let len = intro.bars.len() as i64;
    let mut player = PiecePlayer::new(intro, 500.0, TICKS_PER_SECOND);
    let bar = player.bar_tick(1) - player.bar_tick(0);
    assert!(bar > 0.0);
    // Nothing before its first bar line; then one bar per bar line.
    assert!(player.due(499.0).is_empty());
    let first = player.due(500.0);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, 0);
    // Looped: bars keep coming past the end of the piece.
    let many = player.due(500.0 + bar * (len as f64 * 2.0 + 0.5));
    assert_eq!(many.len() as i64, len * 2);
    assert!(!player.done());
    // Fading out: the next bars get quieter, then the piece is done.
    player.fade_out(p.fade_bars);
    let next = player.next_bar();
    let rest = player.due(f64::MAX / 4.0);
    assert_eq!(rest.len() as u32, p.fade_bars);
    let gains: Vec<f32> = rest.iter().map(|r| r.1).collect();
    assert!(gains.windows(2).all(|w| w[1] < w[0]), "{gains:?}");
    assert!(gains.iter().all(|&g| g > 0.0 && g < 1.0));
    assert!(player.done());
    assert_eq!(player.next_bar(), next + p.fade_bars as i64);
}

#[test]
fn a_stinger_plays_once() {
    let p = screens();
    for which in 0..STYLES.len() {
        let (style, library) = style(which);
        let stinger = piece(&style, &library, PieceKind::Stinger, &p, 2).unwrap();
        let len = stinger.bars.len();
        let mut player = PiecePlayer::new(stinger, 0.0, TICKS_PER_SECOND);
        let gains: Vec<f32> = player.due(1e9).iter().map(|b| b.1).collect();
        assert_eq!(gains.len(), len);
        assert!(player.done());
        // A stinger is a hit: it starts at full level.
        assert_eq!(gains[0], 1.0);
    }
}

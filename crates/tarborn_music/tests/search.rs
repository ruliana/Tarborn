//! The library the search wrote must load, hold what the director needs,
//! and still match what the grammar and metrics produce today.

use std::collections::BTreeSet;
use tarborn_music::grammar::validate;
use tarborn_music::metrics::score;
use tarborn_music::search::{LibraryTune, compose, realize};
use tarborn_music::style::Style;
use tarborn_music::{MusicFile, from_ron};

const MUSIC: &str = include_str!("../../tarborn/assets/music.ron");
const STYLES: [(&str, &str, &str); 2] = [
    (
        "industrial",
        include_str!("../../tarborn/assets/music/styles/industrial.ron"),
        include_str!("../../tarborn/assets/music/library/industrial.ron"),
    ),
    (
        "forest",
        include_str!("../../tarborn/assets/music/styles/forest.ron"),
        include_str!("../../tarborn/assets/music/library/forest.ron"),
    ),
];
/// Fewest tunes a style's library may hold.
const MIN_TUNES: usize = 8;

fn file() -> MusicFile {
    from_ron(MUSIC).unwrap()
}

fn each(mut f: impl FnMut(&str, Style, Vec<LibraryTune>)) {
    for (name, style, library) in STYLES {
        let style: Style = from_ron(style).unwrap_or_else(|e| panic!("{name} style: {e}"));
        let library: Vec<LibraryTune> =
            from_ron(library).unwrap_or_else(|e| panic!("{name} library: {e}"));
        f(name, style, library);
    }
}

#[test]
fn library_is_up_to_date() {
    let file = file();
    each(|name, style, library| {
        let fresh = compose(&style, &file.metrics, &file.search);
        assert!(
            fresh == library,
            "{name}: the library is stale; run `cargo run --release -p tarborn_music --bin compose`"
        );
    });
}

#[test]
fn library_holds_distinct_tunes() {
    each(|name, _, library| {
        assert!(
            library.len() >= MIN_TUNES,
            "{name}: only {} tunes",
            library.len()
        );
        let cells: BTreeSet<_> = library.iter().map(|t| t.cell.clone()).collect();
        assert_eq!(cells.len(), library.len(), "{name}: two tunes share a cell");
        let seeds: BTreeSet<_> = library.iter().map(|t| t.seed).collect();
        assert_eq!(seeds.len(), library.len(), "{name}: two tunes share a seed");
    });
}

#[test]
fn every_part_has_variants_from_distinct_cells() {
    let file = file();
    each(|name, style, library| {
        for t in &library {
            assert!(!t.parts.is_empty(), "{name} {}: no parts", t.seed);
            for p in &t.parts {
                assert!(!p.variants.is_empty(), "{name} {}: empty part", t.seed);
                assert!(p.variants.len() <= file.search.variants);
                let cells: BTreeSet<_> = p.variants.iter().map(|v| v.cell).collect();
                assert_eq!(
                    cells.len(),
                    p.variants.len(),
                    "{name} {}: repeated cell",
                    t.seed
                );
                assert!(p.layer < style.layers);
            }
            // The full layer always has a bass, comping and drums in every section.
            let full = style.layers - 1;
            let roles: BTreeSet<_> = t
                .parts
                .iter()
                .filter(|p| p.layer == full)
                .map(|p| (p.section, p.role))
                .collect();
            assert!(roles.len() >= 3, "{name} {}: thin full layer", t.seed);
        }
    });
}

#[test]
fn regenerated_parts_keep_their_scores_and_rules() {
    let file = file();
    each(|name, style, library| {
        for t in &library {
            let r = realize(&style, t);
            assert_eq!(r.tune.bpm, t.bpm, "{name} {}", t.seed);
            assert_eq!(r.tune.key.to_string(), t.key, "{name} {}", t.seed);
            assert!(
                validate(&style, &r.stinger).is_empty(),
                "{name} {}: stinger",
                t.seed
            );
            for (lp, fragments) in &r.parts {
                for (v, f) in lp.variants.iter().zip(fragments) {
                    assert!(!f.notes.is_empty());
                    assert!(
                        validate(&style, f).is_empty(),
                        "{name} {}: breaks a rule",
                        v.seed
                    );
                    let s = score(f, &style, &file.metrics).total;
                    assert!(
                        (s - v.score).abs() < 1e-4,
                        "{name} {}: {s} != {}",
                        v.seed,
                        v.score
                    );
                    assert!(s >= file.search.min_score);
                }
            }
        }
    });
}

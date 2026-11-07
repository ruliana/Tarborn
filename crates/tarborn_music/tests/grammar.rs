use tarborn_music::grammar::{fragments, tune, validate};
use tarborn_music::style::Style;
use tarborn_music::{PatchBank, Role, from_ron};

const INDUSTRIAL: &str = include_str!("../../tarborn/assets/music/styles/industrial.ron");
const FOREST: &str = include_str!("../../tarborn/assets/music/styles/forest.ron");
const PATCHES: &str = include_str!("../../tarborn/assets/music/patches.ron");

fn styles() -> Vec<Style> {
    [INDUSTRIAL, FOREST]
        .iter()
        .map(|s| from_ron(s).expect("style parses"))
        .collect()
}

#[test]
fn a_thousand_seeds_break_no_rules() {
    for style in styles() {
        let mut broken = Vec::new();
        for seed in 0..1000 {
            let t = tune(&style, seed);
            for f in fragments(&style, &t, seed) {
                for v in validate(&style, &f) {
                    broken.push(format!(
                        "{} seed {seed} {:?} layer {}: {}",
                        style.name, f.role, f.layer, v.0
                    ));
                }
            }
        }
        assert!(
            broken.is_empty(),
            "{} violations, first: {:#?}",
            broken.len(),
            &broken[..broken.len().min(8)]
        );
    }
}

#[test]
fn styles_only_use_patches_that_exist() {
    let bank: PatchBank = from_ron(PATCHES).unwrap();
    for style in styles() {
        let mut names = vec![
            &style.bass.patch,
            &style.drums.ride,
            &style.drums.hat,
            &style.drums.brushes,
            &style.lead.patch,
        ];
        names.extend(style.comp.patches.iter().flatten());
        names.extend(style.stinger.patches.iter());
        for n in names {
            assert!(bank.patches.contains_key(n), "{}: no patch {n}", style.name);
        }
    }
}

#[test]
fn generation_is_deterministic_and_seeds_differ() {
    for style in styles() {
        assert_eq!(tune(&style, 7), tune(&style, 7));
        let a = fragments(&style, &tune(&style, 7), 7);
        assert_eq!(a, fragments(&style, &tune(&style, 7), 7));
        let distinct: std::collections::HashSet<String> = (0..20)
            .map(|s| format!("{:?}", fragments(&style, &tune(&style, s), s)))
            .collect();
        assert!(
            distinct.len() > 15,
            "{}: only {} distinct",
            style.name,
            distinct.len()
        );
    }
}

#[test]
fn forms_and_tempos_follow_the_style() {
    let [industrial, forest] = <[Style; 2]>::try_from(styles()).unwrap();
    let mut blues = 0;
    for seed in 0..200 {
        let t = tune(&industrial, seed);
        assert!(t.bpm >= 200.0 && t.bpm <= 260.0);
        assert!(t.swing >= 0.58 - 1e-6 && t.swing <= 0.62 + 1e-6);
        let bars: Vec<u32> = t.sections.iter().map(|s| s.bars).collect();
        match bars.as_slice() {
            [12] => blues += 1,
            [8, 8] => assert_eq!(t.form, vec![0, 0, 1, 0]),
            other => panic!("unexpected form {other:?}"),
        }
        for s in &t.sections {
            let covered: f32 = s.chords.iter().map(|c| c.length).sum();
            assert_eq!(covered, (s.bars * 4) as f32, "chords cover the section");
        }
        let f = tune(&forest, seed);
        assert!(f.bpm >= 110.0 && f.bpm <= 150.0);
        assert!(
            f.sections.iter().all(|s| s.chords.len() == 1),
            "modal: one chord per section"
        );
        let a = f.sections[0].chords[0].chord.root;
        let b = f.sections[1].chords[0].chord.root;
        assert_eq!(a.to(b), 1, "the bridge shifts up a half step");
    }
    assert!(
        blues > 50 && blues < 150,
        "both forms appear: {blues} blues of 200"
    );
}

#[test]
fn calm_is_bass_and_ride_only() {
    for style in styles() {
        let t = tune(&style, 3);
        for f in fragments(&style, &t, 3) {
            if f.layer == 0 {
                match f.role {
                    Role::Comp | Role::Lead => {
                        assert!(f.notes.is_empty(), "{:?} plays when calm", f.role)
                    }
                    Role::Drums => assert!(
                        f.notes
                            .iter()
                            .all(|n| n.patch == style.drums.ride || n.patch == style.drums.brushes),
                        "calm drums are ride (and brushes)"
                    ),
                    _ => {}
                }
            }
        }
    }
}

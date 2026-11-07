use tarborn_music::theory::{
    Chord, Mode, Pc, Quality, Scale, SwingCurve, VoicingKind, VoicingRules, voice_leading_cost,
};
use tarborn_music::time::{beats_to_seconds, swing};

fn chord(s: &str) -> Chord {
    Chord::parse(s).unwrap_or_else(|| panic!("{s} parses"))
}

fn all_chords() -> impl Iterator<Item = Chord> {
    (0..12).flat_map(|r| Quality::ALL.map(move |q| Chord::new(Pc::new(r), q)))
}

fn rule_sets() -> Vec<VoicingRules> {
    vec![
        VoicingRules::default(),
        VoicingRules {
            range: [40, 64],
            low_limit: 52,
            low_interval: 7,
        },
        VoicingRules {
            range: [55, 84],
            low_limit: 0,
            low_interval: 0,
        },
    ]
}

#[test]
fn symbols_round_trip() {
    for c in all_chords() {
        assert_eq!(Chord::parse(&c.to_string()), Some(c), "{c}");
    }
    assert_eq!(chord("F#m7b5").root, Pc::new(6));
    assert_eq!(chord("Bbmaj7").root, Pc::new(10));
    assert!(Chord::parse("H7").is_none());
    assert!(Chord::parse("Cfoo").is_none());
}

#[test]
fn modes_have_the_right_shapes() {
    for m in Mode::ALL {
        let iv = m.intervals();
        assert_eq!(iv[0], 0, "{m:?}");
        assert!(
            iv.windows(2).all(|w| w[0] < w[1] && w[1] - w[0] <= 3),
            "{m:?}"
        );
        assert!(*iv.last().unwrap() < 12);
    }
    // D Dorian is the white keys.
    let d = Scale::new(Pc::new(2), Mode::Dorian);
    assert!((0..12).all(|p| d.contains(Pc::new(p)) == [0, 2, 4, 5, 7, 9, 11].contains(&p)));
    assert_eq!(d.degree(7), 14, "an octave above the root");
}

#[test]
fn chord_scales_contain_their_chords() {
    for c in all_chords() {
        let scale = c.scale();
        for t in c.tones() {
            assert!(scale.contains(t), "{c}: {t} not in {:?}", scale.mode);
        }
        for t in c.tensions() {
            assert!(
                scale.contains(t),
                "{c}: tension {t} not in {:?}",
                scale.mode
            );
        }
    }
}

#[test]
fn every_voicing_stays_in_range_and_in_the_chord() {
    for rules in rule_sets() {
        for c in all_chords() {
            for kind in VoicingKind::ALL {
                let voicings = c.voicings(kind, &rules);
                for v in &voicings {
                    assert_eq!(v.len(), kind.voices(), "{c} {kind:?} {v:?}");
                    assert!(rules.ok(v), "{c} {kind:?} {v:?} breaks {rules:?}");
                    let in_scale = v.iter().filter(|&&p| c.scale().contains(Pc::of(p))).count();
                    if kind == VoicingKind::Quartal {
                        assert!(in_scale >= 3, "{c} quartal {v:?}");
                    } else {
                        assert!(
                            v.iter().all(|&p| c.allows(Pc::of(p))),
                            "{c} {kind:?} {v:?} has a note outside the chord"
                        );
                    }
                }
            }
            // A normal range always has the everyday voicings.
            if rules == VoicingRules::default() {
                for kind in [
                    VoicingKind::Shell,
                    VoicingKind::RootlessA,
                    VoicingKind::RootlessB,
                ] {
                    assert!(c.voice(kind, &rules, None).is_some(), "{c} {kind:?}");
                }
            }
        }
    }
}

#[test]
fn common_quartal_voicings_are_all_scale_tones() {
    let rules = VoicingRules::default();
    for q in [Quality::Min7, Quality::Dom7, Quality::Sus, Quality::Maj7] {
        for r in 0..12 {
            let c = Chord::new(Pc::new(r), q);
            for v in c.voicings(VoicingKind::Quartal, &rules) {
                assert!(
                    v.iter().all(|&p| c.scale().contains(Pc::of(p))),
                    "{c} {v:?}"
                );
                assert!(v.windows(2).all(|w| w[1] - w[0] == 5), "{c} {v:?}");
            }
        }
    }
}

#[test]
fn so_what_voicing_on_d_dorian() {
    // The So What chord: fourths E A D G over D minor.
    let v = chord("Dm7")
        .voicings(VoicingKind::Quartal, &VoicingRules::default())
        .into_iter()
        .find(|v| v[0] == 64)
        .expect("a quartal voicing from E4");
    assert_eq!(v, vec![64, 69, 74, 79]);
}

#[test]
fn voice_leading_known_cases() {
    // Same chord: nothing moves.
    assert_eq!(voice_leading_cost(&[60, 64, 67, 71], &[60, 64, 67, 71]), 0);
    // ii-V with rootless voicings: Dm9 (F A C E) to G13 (F A B E), one
    // semitone.
    assert_eq!(voice_leading_cost(&[65, 69, 72, 76], &[65, 69, 71, 76]), 1);
    // The tritone resolves by half steps: B F to C E.
    assert_eq!(voice_leading_cost(&[59, 65], &[60, 64]), 2);
    // Symmetric, and a doubled voice may split.
    assert_eq!(
        voice_leading_cost(&[60, 67], &[59, 62, 67]),
        voice_leading_cost(&[59, 62, 67], &[60, 67])
    );
    assert_eq!(voice_leading_cost(&[60, 67], &[59, 62, 67]), 3);
    // A leap costs its size.
    assert_eq!(voice_leading_cost(&[60], &[72]), 12);
}

#[test]
fn chosen_voicings_lead_smoothly_through_a_ii_v_i() {
    let rules = VoicingRules::default();
    let mut prev = chord("Dm7")
        .voice(VoicingKind::RootlessA, &rules, None)
        .unwrap();
    for (sym, kind) in [
        ("G7", VoicingKind::RootlessB),
        ("Cmaj7", VoicingKind::RootlessA),
    ] {
        let next = chord(sym).voice(kind, &rules, Some(&prev)).unwrap();
        let cost = voice_leading_cost(&prev, &next);
        assert!(cost <= 4, "{sym}: {prev:?} -> {next:?} costs {cost}");
        // And it's the cheapest of all the voicings of that kind.
        for other in chord(sym).voicings(kind, &rules) {
            assert!(voice_leading_cost(&prev, &other) >= cost);
        }
        prev = next;
    }
}

#[test]
fn tritone_subs_share_the_tritone() {
    for r in 0..12 {
        let g7 = Chord::new(Pc::new(r), Quality::Dom7);
        let sub = g7.tritone_sub();
        assert_eq!(sub.root, Pc::new(r + 6));
        let [_, t3, _, t7] = g7.tones();
        let [_, s3, _, s7] = sub.tones();
        assert_eq!((t3, t7), (s7, s3), "{g7} and {sub}");
    }
    assert_eq!(chord("Cmaj7").tritone_sub(), chord("Cmaj7"));
}

#[test]
fn swing_timing_matches_the_ratio() {
    for ratio in [0.5, 0.55, 0.6, 0.62, 0.67, 0.75] {
        for beat in 0..16 {
            let b = beat as f32;
            assert!((swing(b, ratio) - b).abs() < 1e-5, "downbeats stay put");
            let offbeat = swing(b + 0.5, ratio) - b;
            assert!((offbeat - ratio).abs() < 1e-5, "offbeat at {ratio}");
        }
        // At 120 BPM a beat is half a second, so the offbeat is ratio / 2 s in.
        assert!((beats_to_seconds(0.5, 120.0, ratio) - ratio / 2.0).abs() < 1e-6);
    }
}

#[test]
fn swing_eases_at_speed() {
    let curve = SwingCurve::default();
    assert!((curve.ratio(60.0) - curve.slow_ratio).abs() < 1e-6);
    assert!((curve.ratio(320.0) - curve.fast_ratio).abs() < 1e-6);
    let mut last = 1.0;
    for bpm in (100..=280).step_by(10) {
        let r = curve.ratio(bpm as f32);
        assert!(r <= last);
        last = r;
    }
}

#[test]
fn nearest_pitch_of_a_class() {
    assert_eq!(Pc::new(0).nearest(62), 60);
    assert_eq!(Pc::new(0).nearest(70), 72);
    assert_eq!(Pc::new(7).nearest(67), 67);
}

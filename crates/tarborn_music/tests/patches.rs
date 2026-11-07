use tarborn_music::analysis::{AnalysisParams, describe};
use tarborn_music::patches::{InstrumentFile, generate};
use tarborn_music::{PatchBank, from_ron};
use tarborn_sfx::{Layer, SAMPLE_RATE, Sfx, Wave};

const INSTRUMENTS: &str = include_str!("../../tarborn/assets/music/instruments.ron");
const PATCHES: &str = include_str!("../../tarborn/assets/music/patches.ron");

fn instruments() -> InstrumentFile {
    from_ron(INSTRUMENTS).expect("instruments.ron parses")
}

#[test]
fn every_generated_patch_lands_inside_its_targets() {
    let file = instruments();
    let bank: PatchBank = from_ron(PATCHES).expect("patches.ron parses");
    for (name, instrument) in &file.instruments {
        let patch = bank
            .patches
            .get(name)
            .unwrap_or_else(|| panic!("{name} is missing from patches.ron: regenerate it"));
        let d = instrument.measure(patch, &file.analysis, file.search.sample_rate);
        let (miss, _) = instrument.fit(&d, file.search.time_floor);
        assert!(
            miss == 0.0,
            "{name} measures {d:?}, outside {:?}",
            instrument.targets
        );
    }
}

#[test]
fn the_band_and_both_player_kits_are_there() {
    let file = instruments();
    for name in [
        "bass",
        "ride",
        "hat",
        "brushes",
        "piano",
        "epiano",
        "horn",
        "marimba",
        "ind_kick",
        "ind_accent",
        "ind_snare",
        "ind_crash",
        "for_kick",
        "for_accent",
        "for_rim",
        "for_crash",
    ] {
        assert!(file.instruments.contains_key(name), "{name}");
    }
}

#[test]
fn generation_is_deterministic() {
    let file = instruments();
    let search = tarborn_music::patches::SearchParams {
        candidates: 8,
        ..file.search.clone()
    };
    let piano = &file.instruments["piano"];
    let a = generate(piano, &file.analysis, &search, 5);
    let b = generate(piano, &file.analysis, &search, 5);
    assert_eq!(a.patch, b.patch);
    let c = generate(piano, &file.analysis, &search, 6);
    assert_ne!(a.patch, c.patch);
}

fn tone(layer: Layer) -> Vec<f32> {
    Sfx {
        layers: vec![layer],
        ..Sfx::default()
    }
    .render(SAMPLE_RATE)
}

#[test]
fn a_sine_is_bright_at_its_pitch_and_not_noisy() {
    let d = describe(
        &tone(Layer {
            freq: 1000.0,
            hold: 0.2,
            decay: 0.2,
            ..Layer::default()
        }),
        SAMPLE_RATE,
        &AnalysisParams::default(),
    );
    assert!((d.centroid - 1000.0).abs() < 60.0, "{d:?}");
    assert!(d.noisiness < 0.02, "{d:?}");
}

#[test]
fn noise_is_noisy_even_when_filtered() {
    let d = describe(
        &tone(Layer {
            wave: Wave::Noise,
            freq: 20_000.0,
            hold: 0.2,
            decay: 0.2,
            lowpass: 3000.0,
            ..Layer::default()
        }),
        SAMPLE_RATE,
        &AnalysisParams::default(),
    );
    assert!(d.noisiness > 0.5, "{d:?}");
    assert!(d.centroid < 4000.0, "{d:?}");
}

#[test]
fn attack_and_decay_follow_the_envelope() {
    let p = AnalysisParams::default();
    let slow = describe(
        &tone(Layer {
            attack: 0.1,
            decay: 0.5,
            ..Layer::default()
        }),
        SAMPLE_RATE,
        &p,
    );
    let fast = describe(
        &tone(Layer {
            attack: 0.0,
            decay: 0.1,
            ..Layer::default()
        }),
        SAMPLE_RATE,
        &p,
    );
    assert!(slow.attack > 0.07 && slow.attack < 0.1, "{slow:?}");
    assert!(fast.attack < 0.003, "{fast:?}");
    assert!(slow.decay > fast.decay * 3.0, "{slow:?} vs {fast:?}");
}

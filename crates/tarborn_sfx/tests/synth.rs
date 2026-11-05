use tarborn_sfx::{Layer, Library, SAMPLE_RATE, Sfx, Wave, wav};

const RATE: u32 = SAMPLE_RATE;

fn sfx(layers: Vec<Layer>) -> Sfx {
    Sfx {
        layers,
        ..Sfx::default()
    }
}

fn rms(s: &[f32]) -> f32 {
    (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt()
}

/// Upward zero crossings per second: a rough pitch for a pure tone.
fn pitch_of(s: &[f32]) -> f32 {
    let ups = s.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    ups as f32 * RATE as f32 / s.len() as f32
}

#[test]
fn length_follows_the_envelope_and_delays() {
    let a = Layer {
        attack: 0.1,
        hold: 0.05,
        decay: 0.2,
        ..Layer::default()
    };
    let b = Layer {
        delay: 0.3,
        decay: 0.2,
        ..Layer::default()
    };
    let s = sfx(vec![a, b]);
    assert!((s.length() - 0.5).abs() < 1e-6);
    let samples = s.render(RATE);
    assert_eq!(samples.len(), (0.5 * RATE as f32).ceil() as usize);
}

#[test]
fn rendering_is_deterministic_and_bounded() {
    let layer = Layer {
        wave: Wave::Noise,
        freq: 8000.0,
        lowpass: 3000.0,
        lowpass_end: 200.0,
        resonance: 0.95,
        drive: 3.0,
        gain: 4.0,
        punch: 2.0,
        ..Layer::default()
    };
    let s = sfx(vec![layer.clone(), layer]);
    let a = s.render(RATE);
    assert_eq!(a, s.render(RATE));
    assert!(a.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
    assert!(rms(&a) > 0.05);
}

#[test]
fn slides_reach_the_end_pitch() {
    let flat = |f| {
        Layer {
            freq: f,
            hold: 0.5,
            decay: 0.0,
            ..Layer::default()
        }
        .render(RATE)
    };
    assert!((pitch_of(&flat(220.0)) - 220.0).abs() < 5.0);
    let slide = Layer {
        freq: 200.0,
        freq_end: 800.0,
        slide_time: 0.1,
        hold: 1.0,
        decay: 0.0,
        ..Layer::default()
    }
    .render(RATE);
    // After the slide, it holds the end pitch.
    let tail = &slide[(0.5 * RATE as f32) as usize..];
    assert!((pitch_of(tail) - 800.0).abs() < 10.0, "{}", pitch_of(tail));
}

#[test]
fn arpeggio_steps_the_pitch() {
    let l = Layer {
        freq: 300.0,
        arp_time: 0.25,
        arp_ratio: 2.0,
        hold: 0.5,
        decay: 0.0,
        ..Layer::default()
    };
    let s = l.render(RATE);
    let half = s.len() / 2;
    assert!((pitch_of(&s[..half]) - 300.0).abs() < 8.0);
    assert!((pitch_of(&s[half + 200..]) - 600.0).abs() < 12.0);
}

#[test]
fn lowpass_removes_high_tones_and_highpass_low_ones() {
    let tone = |f, lowpass, highpass| {
        let l = Layer {
            freq: f,
            hold: 0.3,
            decay: 0.0,
            lowpass,
            highpass,
            ..Layer::default()
        };
        rms(&l.render(RATE)[2000..])
    };
    let open = tone(5000.0, 0.0, 0.0);
    assert!(tone(5000.0, 300.0, 0.0) < open * 0.1);
    assert!(tone(60.0, 0.0, 3000.0) < tone(60.0, 0.0, 0.0) * 0.1);
}

#[test]
fn envelope_shapes() {
    let l = Layer {
        attack: 0.1,
        hold: 0.1,
        decay: 0.1,
        punch: 1.0,
        decay_curve: 2.0,
        ..Layer::default()
    };
    assert_eq!(l.envelope(-0.1), 0.0);
    assert!((l.envelope(0.05) - 0.5).abs() < 1e-5);
    assert!(
        (l.envelope(0.1) - 2.0).abs() < 1e-5,
        "punch at start of hold"
    );
    assert!((l.envelope(0.25) - 0.25).abs() < 1e-4, "curved decay");
    assert_eq!(l.envelope(0.31), 0.0);
}

#[test]
fn variation_spans_intensity_and_jitter() {
    let s = Sfx {
        pitch_jitter: 12.0,
        volume_jitter: 0.5,
        intensity_volume: [0.5, 1.0],
        intensity_pitch: [1.0, 0.5],
        ..Sfx::default()
    };
    assert_eq!(s.variation(0.0, 0.5, 0.5), (0.5, 1.0));
    assert_eq!(s.variation(1.0, 0.5, 0.5), (1.0, 0.5));
    let (v, p) = s.variation(0.0, 1.0, 1.0);
    assert!((v - 0.75).abs() < 1e-5 && (p - 2.0).abs() < 1e-5);
}

#[test]
fn layers_only_list_what_they_change() {
    let lib = Library::from_ron(
        r#"(ignored: 3, sounds: { "beep": (volume: 0.5, intensity_pitch: (1.0, 0.8),
            layers: [(wave: Square, freq: 880, decay: 0.1)]) })"#,
    )
    .unwrap();
    let beep = &lib.sounds["beep"];
    assert_eq!(beep.volume, 0.5);
    assert_eq!(beep.layers[0].wave, Wave::Square);
    assert_eq!(beep.layers[0].duty, 0.5);
    assert_eq!(beep.intensity_pitch, [1.0, 0.8]);
}

#[test]
fn typos_in_a_layer_are_errors() {
    assert!(Library::from_ron(r#"(sounds: { "x": (layers: [(frq: 1)]) })"#).is_err());
}

#[test]
fn the_game_sound_file_parses_and_every_sound_is_audible() {
    let lib = Library::from_ron(include_str!("../../tarborn/assets/sfx.ron")).unwrap();
    assert!(lib.sounds.len() > 20);
    for (name, sfx) in &lib.sounds {
        let s = sfx.render(RATE);
        assert!(!s.is_empty(), "{name} is empty");
        assert!(s.iter().all(|x| x.is_finite()), "{name} has NaNs");
        let peak = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.05, "{name} is nearly silent ({peak})");
        assert!(sfx.length() < 2.0, "{name} is too long");
    }
}

#[test]
fn wav_header_is_well_formed() {
    let bytes = wav::encode(&[0.0, 1.0, -1.0], 44_100);
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..16], b"WAVEfmt ");
    assert_eq!(bytes.len(), 44 + 6);
    assert_eq!(i16::from_le_bytes([bytes[46], bytes[47]]), i16::MAX);
}

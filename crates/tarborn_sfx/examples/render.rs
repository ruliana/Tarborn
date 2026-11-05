//! Renders every sound in a sound file to WAV files, to listen outside the game.
//!
//! cargo run -p tarborn_sfx --example render -- crates/tarborn/assets/sfx.ron out/

use std::path::PathBuf;
use tarborn_sfx::{Library, SAMPLE_RATE, wav};

fn main() {
    let mut args = std::env::args().skip(1);
    let file = args
        .next()
        .unwrap_or_else(|| "crates/tarborn/assets/sfx.ron".into());
    let out = PathBuf::from(args.next().unwrap_or_else(|| "sfx_out".into()));
    let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("reading {file}: {e}"));
    let library = Library::from_ron(&text).unwrap_or_else(|e| panic!("parsing {file}: {e}"));
    std::fs::create_dir_all(&out).expect("creating the output folder");
    for (name, sfx) in &library.sounds {
        let samples = sfx.render(SAMPLE_RATE);
        let path = out.join(format!("{name}.wav"));
        std::fs::write(&path, wav::encode(&samples, SAMPLE_RATE)).expect("writing a wav");
        // Loudness, and the peak before the limiter: above 1 means it squashes.
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt();
        let raw_peak = sfx
            .mix(SAMPLE_RATE)
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()));
        println!(
            "{name:16} {:5.2} s  rms {rms:.2}  raw peak {raw_peak:4.2}  -> {}",
            sfx.length(),
            path.display()
        );
    }
}

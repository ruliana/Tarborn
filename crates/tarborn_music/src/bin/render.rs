//! Renders one fragment file to a WAV file, to listen outside the game.
//!
//! cargo run -p tarborn_music --bin render -- <fragment.ron> <out.wav> [seed]
//!     [--patches <patches.ron>] [--settings <music.ron>]

use std::path::PathBuf;
use tarborn_music::{Fragment, MusicFile, PatchBank, from_ron, render};
use tarborn_sfx::wav;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../tarborn/assets");

fn main() {
    let mut positional = Vec::new();
    let mut patches = PathBuf::from(format!("{ASSETS}/music/patches.ron"));
    let mut settings = PathBuf::from(format!("{ASSETS}/music.ron"));
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--patches" => patches = args.next().expect("--patches needs a path").into(),
            "--settings" => settings = args.next().expect("--settings needs a path").into(),
            _ => positional.push(arg),
        }
    }
    let [fragment, out, rest @ ..] = positional.as_slice() else {
        eprintln!(
            "usage: render <fragment.ron> <out.wav> [seed] [--patches p.ron] [--settings music.ron]"
        );
        std::process::exit(2);
    };
    let seed: u32 = rest
        .first()
        .map_or(1, |s| s.parse().expect("seed is a number"));

    let fragment: Fragment = load(&PathBuf::from(fragment));
    let bank: PatchBank = load(&patches);
    let file: MusicFile = load(&settings);
    let samples = render(&fragment, &bank, &file.render, seed).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    std::fs::write(out, wav::encode(&samples, file.render.sample_rate)).expect("writing the wav");
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    println!(
        "{} notes, {:.2} s, peak {peak:.2} -> {out}",
        fragment.notes.len(),
        samples.len() as f32 / file.render.sample_rate as f32
    );
}

fn load<T: serde::de::DeserializeOwned>(path: &PathBuf) -> T {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    from_ron(&text).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()))
}

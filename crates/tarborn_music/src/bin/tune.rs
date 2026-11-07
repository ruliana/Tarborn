//! Generates a tune in a style and renders one chorus of it with the whole
//! band at one intensity layer, to listen to the grammar's raw proposals.
//!
//! cargo run --release -p tarborn_music --bin tune -- <style> <seed> <layer> <out.wav>
//!     <style> is a name in assets/music/styles/ or a path to a style file.

use tarborn_music::grammar::{fragments, tune};
use tarborn_music::style::Style;
use tarborn_music::{Fragment, MusicFile, PatchBank, Role, from_ron, render};
use tarborn_sfx::wav;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../tarborn/assets");

fn load<T: serde::de::DeserializeOwned>(path: &str) -> T {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    from_ron(&text).unwrap_or_else(|e| panic!("parsing {path}: {e}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [style, seed, layer, out] = args.as_slice() else {
        eprintln!("usage: tune <style> <seed> <layer> <out.wav>");
        std::process::exit(2);
    };
    let path = if style.ends_with(".ron") {
        style.clone()
    } else {
        format!("{ASSETS}/music/styles/{style}.ron")
    };
    let style: Style = load(&path);
    let seed: u32 = seed.parse().expect("seed is a number");
    let layer: u8 = layer.parse().expect("layer is a number");
    let bank: PatchBank = load(&format!("{ASSETS}/music/patches.ron"));
    let file: MusicFile = load(&format!("{ASSETS}/music.ron"));

    let t = tune(&style, seed);
    let parts = fragments(&style, &t, seed);
    let mut chorus = Fragment {
        role: Role::Stinger,
        beats_per_bar: t.beats_per_bar,
        bars: 0,
        style: style.name.clone(),
        ..Fragment::default()
    };
    for &s in &t.form {
        let offset = chorus.beats();
        let label = &t.sections[s].label;
        for f in parts
            .iter()
            .filter(|f| &f.section == label && f.layer == layer && f.role != Role::Stinger)
        {
            for n in &f.notes {
                let mut n = n.clone();
                n.start += offset;
                chorus.notes.push(n);
            }
        }
        chorus.bars += t.sections[s].bars;
    }
    let settings = tarborn_music::RenderSettings {
        bpm: t.bpm,
        swing: t.swing,
        ..file.render
    };
    let samples = render(&chorus, &bank, &settings, seed).expect("renders");
    std::fs::write(out, wav::encode(&samples, settings.sample_rate)).expect("writing the wav");
    let chords: Vec<String> = t.sections[0]
        .chords
        .iter()
        .map(|c| c.chord.to_string())
        .collect();
    println!(
        "{} {} {:.0} BPM swing {:.2}, form {:?}, {} notes, {:.1} s -> {out}\n  first section: {}",
        style.name,
        if t.minor { "minor" } else { "major" },
        t.bpm,
        t.swing,
        t.form
            .iter()
            .map(|&s| t.sections[s].label.as_str())
            .collect::<Vec<_>>(),
        chorus.notes.len(),
        samples.len() as f32 / settings.sample_rate as f32,
        chords.join(" "),
    );
}

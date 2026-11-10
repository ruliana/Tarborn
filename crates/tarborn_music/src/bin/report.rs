//! Per-style statistics of what the grammar proposes: rule violations, and
//! each metric's spread and score per role and intensity layer.
//!
//! cargo run --release -p tarborn_music --bin report -- [seeds] [style...]
//!
//! With `--runs [dir]` it summarises the game's per-run music logs instead
//! (the `telemetry` section of `music.ron` says where they are).

use std::collections::BTreeMap;
use tarborn_music::grammar::{fragments, tune, validate};
use tarborn_music::metrics::{Measures, score};
use tarborn_music::style::Style;
use tarborn_music::telemetry::{RunLog, summarize};
use tarborn_music::{MusicFile, Role, from_ron};

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../tarborn/assets");

fn load<T: serde::de::DeserializeOwned>(path: &str) -> T {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    from_ron(&text).unwrap_or_else(|e| panic!("parsing {path}: {e}"))
}

#[derive(Default)]
struct Stats {
    n: usize,
    sum: [f32; 7],
    min: [f32; 7],
    max: [f32; 7],
}

impl Stats {
    fn add(&mut self, m: &Measures, total: f32) {
        let mut v = [0.0; 7];
        v[..6].copy_from_slice(&m.vector());
        v[6] = total;
        if self.n == 0 {
            self.min = v;
            self.max = v;
        }
        for (i, x) in v.into_iter().enumerate() {
            self.sum[i] += x;
            self.min[i] = self.min[i].min(x);
            self.max[i] = self.max[i].max(x);
        }
        self.n += 1;
    }
}

/// Summarises every run log in `dir`.
fn runs(dir: &std::path::Path, file: &MusicFile) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        println!("no runs logged yet in {}", dir.display());
        return;
    };
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "ron"))
        .collect();
    paths.sort();
    let logs: Vec<RunLog> = paths
        .iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(p).ok()?;
            from_ron(&text)
                .inspect_err(|e| eprintln!("{}: {e}", p.display()))
                .ok()
        })
        .collect();
    println!("{}", dir.display());
    println!("{}", summarize(&logs, &file.telemetry));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--runs") {
        let file: MusicFile = load(&format!("{ASSETS}/music.ron"));
        let dir = args.get(1).map_or_else(
            || std::path::Path::new(ASSETS).join(&file.telemetry.dir),
            std::path::PathBuf::from,
        );
        runs(&dir, &file);
        return;
    }
    let seeds: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(200);
    let mut names: Vec<String> = args.iter().skip(1).cloned().collect();
    if names.is_empty() {
        names = vec!["industrial".into(), "forest".into()];
    }
    let file: MusicFile = load(&format!("{ASSETS}/music.ron"));
    let p = &file.metrics;
    for name in names {
        let style: Style = load(&format!("{ASSETS}/music/styles/{name}.ron"));
        let mut stats: BTreeMap<(Role, u8), Stats> = BTreeMap::new();
        let mut violations = 0;
        let mut fragments_n = 0;
        for seed in 0..seeds {
            let t = tune(&style, seed);
            for f in fragments(&style, &t, seed) {
                fragments_n += 1;
                violations += validate(&style, &f).len();
                if f.notes.is_empty() {
                    continue;
                }
                let s = score(&f, &style, p);
                stats
                    .entry((f.role, f.layer))
                    .or_default()
                    .add(&s.measures, s.total);
            }
        }
        println!(
            "\n== {name}: {seeds} tunes, {fragments_n} fragments, {violations} rule violations"
        );
        println!(
            "{:14} {:>5} | {:^17} | {:^17} | {:^17} | {:^17} | {:^17} | {:^17} | score",
            "role/layer",
            "n",
            "tension",
            "dissonance",
            "voice leading",
            "syncopation",
            "repetition",
            "density"
        );
        for ((role, layer), s) in &stats {
            let mut row = format!("{:14} {:>5}", format!("{role:?}/{layer}"), s.n);
            for i in 0..6 {
                row += &format!(
                    " | {:5.2} {:5.2}-{:5.2}",
                    s.sum[i] / s.n as f32,
                    s.min[i],
                    s.max[i]
                );
            }
            row += &format!(" | {:.2} (min {:.2})", s.sum[6] / s.n as f32, s.min[6]);
            println!("{row}");
        }
    }
}

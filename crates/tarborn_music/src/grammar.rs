//! The grammar proposer: seeded generation of tunes (forms and chord
//! progressions) and of every part over them (walking bass, comping, the
//! band drummer, lead motifs, stingers), all driven by a style file.
//!
//! It proposes; it doesn't judge. [`validate`] checks the hard rules every
//! fragment must obey (range, chord fit, voice leading limits, no
//! unresolved chromatic notes), and later steps score and select.
//!
//! The functional grammar builds progressions backwards from their targets,
//! the way jazz harmony is usually analysed (Steedman's chord-sequence
//! grammar): every goal chord may be prepared by its dominant, every
//! dominant by its ii, and any dominant may be replaced by its tritone
//! substitute.

use crate::rng::Rng;
use crate::score::{ChordSpan, Fragment, Note, Role};
use crate::style::{BassFeel, FunctionalHarmony, Harmony, ModalHarmony, Style, at};
use crate::theory::{Chord, Mode, Pc, Quality};

/// One section of a form: its chords, in beats from the section start.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub label: String,
    pub bars: u32,
    pub chords: Vec<ChordSpan>,
}

/// A generated tune: key, tempo and form.
#[derive(Clone, Debug, PartialEq)]
pub struct Tune {
    pub style: String,
    pub key: Pc,
    pub minor: bool,
    pub bpm: f32,
    pub swing: f32,
    pub beats_per_bar: u32,
    pub sections: Vec<Section>,
    /// Section order of one chorus, as indices into `sections`.
    pub form: Vec<usize>,
}

/// Generates a tune in `style`.
pub fn tune(style: &Style, seed: u32) -> Tune {
    let mut rng = Rng::new(seed);
    let key = style
        .keys
        .get(rng.next_u32() as usize % style.keys.len().max(1))
        .and_then(|k| Pc::parse(k))
        .map_or(Pc::new(0), |(pc, _)| pc);
    let bpm = rng.range(style.tempo);
    let bpb = style.beats_per_bar.max(1);
    let (minor, sections, form) = match &style.harmony {
        Harmony::Functional(h) => functional(h, key, bpb, &mut rng),
        Harmony::Modal(h) => modal(h, key, bpb, &mut rng),
    };
    Tune {
        style: style.name.clone(),
        key,
        minor,
        bpm,
        swing: style.swing.ratio(bpm),
        beats_per_bar: bpb,
        sections,
        form,
    }
}

fn is_minor(q: Quality) -> bool {
    matches!(
        q,
        Quality::Min7 | Quality::Min6 | Quality::MinMaj7 | Quality::HalfDim | Quality::Dim7
    )
}

/// The dominant that resolves to `target`.
fn dominant(target: Chord, minor_key: bool, h: &FunctionalHarmony, rng: &mut Rng) -> Chord {
    let to_minor = is_minor(target.quality) || minor_key;
    let quality = if to_minor {
        if rng.chance(h.altered) {
            Quality::Dom7Alt
        } else {
            Quality::Dom7Flat9
        }
    } else if rng.chance(h.altered * 0.5) {
        Quality::Dom7Alt
    } else {
        Quality::Dom7
    };
    let v = Chord::new(target.root.up(7), quality);
    if rng.chance(h.tritone_sub) {
        v.tritone_sub()
    } else {
        v
    }
}

/// The ii that prepares `target`'s dominant.
fn two(target: Chord, minor_key: bool) -> Chord {
    let quality = if is_minor(target.quality) || minor_key {
        Quality::HalfDim
    } else {
        Quality::Min7
    };
    Chord::new(target.root.up(2), quality)
}

/// Bars of chords: each bar holds one chord, or two of half a bar each.
type Bars = Vec<Vec<Chord>>;

fn spans(bars: &Bars, bpb: u32) -> Vec<ChordSpan> {
    let mut out: Vec<ChordSpan> = Vec::new();
    for (i, bar) in bars.iter().enumerate() {
        let each = bpb as f32 / bar.len() as f32;
        for (j, chord) in bar.iter().enumerate() {
            let start = i as f32 * bpb as f32 + j as f32 * each;
            // A chord held over the bar line is one span.
            if let Some(last) = out.last_mut()
                && last.chord == *chord
            {
                last.length += each;
                continue;
            }
            out.push(ChordSpan {
                start,
                length: each,
                chord: *chord,
                mode: chord.quality.mode(),
            });
        }
    }
    out
}

fn functional(
    h: &FunctionalHarmony,
    key: Pc,
    bpb: u32,
    rng: &mut Rng,
) -> (bool, Vec<Section>, Vec<usize>) {
    let minor = rng.chance(h.minor);
    let tonic = |root: Pc, rng: &mut Rng| {
        let q = if minor {
            *rng.pick(&[Quality::Min7, Quality::Min6, Quality::Min7])
        } else {
            *rng.pick(&[Quality::Maj7, Quality::Maj6, Quality::Maj7])
        };
        Chord::new(root, q)
    };
    let i = tonic(key, rng);
    // ii-V into a target, as two bars or (fast) one.
    let cadence = |target: Chord, rng: &mut Rng| -> (Chord, Chord) {
        (two(target, minor), dominant(target, minor, h, rng))
    };
    let blues = rng.weighted(&[h.blues_weight, h.aaba_weight]) == 0;
    if blues {
        // 12-bar minor blues.
        let iv = Chord::new(key.up(5), Quality::Min7);
        let vi = Chord::new(key.up(8), Quality::Dom7);
        let v = dominant(i, true, h, rng);
        let (tii, tv) = cadence(i, rng);
        let mut bars: Bars = vec![
            vec![i],
            vec![iv],
            vec![i],
            vec![i],
            vec![iv],
            vec![iv],
            vec![i],
            vec![i],
            vec![vi],
            vec![v],
            vec![i],
            vec![tii, tv],
        ];
        if rng.chance(h.secondary) {
            let (a, b) = cadence(iv, rng);
            bars[3] = vec![a, b];
        }
        if rng.chance(h.secondary) {
            let (a, b) = cadence(vi, rng);
            bars[7] = vec![a, b];
        }
        let section = Section {
            label: "blues".into(),
            bars: 12,
            chords: spans(&bars, bpb),
        };
        return (true, vec![section], vec![0]);
    }

    // 32-bar AABA.
    let goals = if minor {
        vec![
            Chord::new(key.up(5), Quality::Min7),
            Chord::new(key.up(3), Quality::Maj7),
        ]
    } else {
        vec![
            Chord::new(key.up(5), Quality::Maj7),
            Chord::new(key.up(2), Quality::Min7),
            Chord::new(key.up(9), Quality::Min7),
        ]
    };
    let w = *rng.pick(&goals);
    let mut a: Bars = Vec::new();
    a.push(vec![i]);
    if rng.chance(h.secondary) {
        let (x, y) = cadence(two(w, minor), rng);
        a.push(vec![x, y]);
    } else {
        a.push(vec![i]);
    }
    let (wii, wv) = cadence(w, rng);
    if rng.chance(h.fast_ii_v) {
        let pre = Chord::new(
            key.up(if minor { 8 } else { 9 }),
            if minor { Quality::Maj7 } else { Quality::Min7 },
        );
        a.push(vec![pre]);
        a.push(vec![wii, wv]);
    } else {
        a.push(vec![wii]);
        a.push(vec![wv]);
    }
    a.push(vec![w]);
    a.push(vec![w]);
    let (iii, iv) = cadence(i, rng);
    a.push(vec![iii, iv]);
    let (tii, tv) = cadence(i, rng);
    a.push(vec![tii, tv]);

    let shifts: Vec<f32> = h.bridge_keys.iter().map(|(_, w)| *w).collect();
    let shift = h
        .bridge_keys
        .get(rng.weighted(&shifts))
        .map_or(5, |(s, _)| *s);
    let bi = tonic(key.up(shift), rng);
    let (bii, bv) = cadence(bi, rng);
    let (rii, rv) = cadence(i, rng);
    let mut b: Bars = vec![vec![bii], vec![bv], vec![bi], vec![bi]];
    if rng.chance(h.secondary) {
        let x = Chord::new(
            bi.root.up(5),
            if minor { Quality::Min7 } else { Quality::Maj7 },
        );
        let (xii, xv) = cadence(x, rng);
        b.push(vec![xii, xv]);
        b.push(vec![x]);
    } else {
        b.push(vec![bi]);
        b.push(vec![bi]);
    }
    b.push(vec![rii]);
    b.push(vec![rv]);
    let sections = vec![
        Section {
            label: "A".into(),
            bars: 8,
            chords: spans(&a, bpb),
        },
        Section {
            label: "B".into(),
            bars: 8,
            chords: spans(&b, bpb),
        },
    ];
    (minor, sections, vec![0, 0, 1, 0])
}

/// The chord that names a mode on `root`.
fn modal_chord(root: Pc, mode: Mode) -> Chord {
    let q = match mode {
        Mode::Ionian => Quality::Maj7,
        Mode::Lydian => Quality::Maj7Sharp11,
        Mode::Mixolydian => Quality::Sus,
        Mode::LydianDominant => Quality::Dom7Sharp11,
        Mode::Locrian | Mode::LocrianNat2 => Quality::HalfDim,
        Mode::MelodicMinor => Quality::MinMaj7,
        _ => Quality::Min7,
    };
    Chord::new(root, q)
}

fn modal(h: &ModalHarmony, key: Pc, bpb: u32, rng: &mut Rng) -> (bool, Vec<Section>, Vec<usize>) {
    let weights: Vec<f32> = h.modes.iter().map(|(_, w)| *w).collect();
    let mode = h
        .modes
        .get(rng.weighted(&weights))
        .map_or(Mode::Dorian, |(m, _)| *m);
    let vamp = |root: Pc, bars: u32, label: &str| Section {
        label: label.into(),
        bars,
        chords: vec![ChordSpan {
            start: 0.0,
            length: (bars * bpb) as f32,
            chord: modal_chord(root, mode),
            mode,
        }],
    };
    let a_bars = *rng.pick(&h.vamp_bars);
    let b_bars = *rng.pick(&h.bridge_bars);
    let minor = !matches!(
        mode,
        Mode::Ionian | Mode::Lydian | Mode::Mixolydian | Mode::LydianDominant
    );
    let sections = vec![
        vamp(key, a_bars, "A"),
        vamp(key.up(h.bridge_shift), b_bars, "B"),
    ];
    (minor, sections, vec![0, 0, 1, 0])
}

// ---- Parts -------------------------------------------------------------------

/// Every part of a tune: each section × role × layer, plus a stinger.
pub fn fragments(style: &Style, tune: &Tune, seed: u32) -> Vec<Fragment> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::new();
    for (s, _) in tune.sections.iter().enumerate() {
        for role in [Role::Bass, Role::Comp, Role::Drums, Role::Lead] {
            for layer in 0..style.layers.max(1) {
                let salt = (s as u32) << 16 | (role as u32) << 8 | layer as u32;
                out.push(part(style, tune, s, role, layer, rng.fork(salt).next_u32()));
            }
        }
    }
    out.push(stinger(style, tune, rng.next_u32()));
    out
}

/// One part of one section at one intensity layer.
pub fn part(
    style: &Style,
    tune: &Tune,
    section: usize,
    role: Role,
    layer: u8,
    seed: u32,
) -> Fragment {
    let mut rng = Rng::new(seed);
    let s = &tune.sections[section];
    let mut f = Fragment {
        role,
        layer,
        section: s.label.clone(),
        bars: s.bars,
        beats_per_bar: tune.beats_per_bar,
        style: style.name.clone(),
        chords: s.chords.clone(),
        notes: Vec::new(),
    };
    f.notes = match role {
        Role::Bass => bass(style, &f, layer, &mut rng),
        Role::Comp => comp(style, &f, layer, &mut rng),
        Role::Drums => drums(style, &f, layer, &mut rng),
        Role::Lead => lead(style, &f, layer, &mut rng),
        Role::Stinger => Vec::new(),
    };
    f
}

/// The pitch of `pc` nearest `near`, moved by octaves into `range`.
fn place(pc: Pc, near: i32, [lo, hi]: [u8; 2]) -> i32 {
    let mut p = pc.nearest(near);
    while p < lo as i32 {
        p += 12;
    }
    while p > hi as i32 {
        p -= 12;
    }
    p
}

/// Where the bass puts the next root: within `max_leap` of `from`,
/// preferring the inner range so the line never gets stuck at an edge.
fn root_after(pc: Pc, from: i32, range: [u8; 2], inner: [u8; 2], max_leap: u8) -> i32 {
    let near = pc.nearest(from);
    [near - 12, near, near + 12]
        .into_iter()
        .filter(|&p| p >= range[0] as i32 && p <= range[1] as i32)
        .min_by_key(|&p| {
            let leap = (p - from).abs();
            let outside = p < inner[0] as i32 || p > inner[1] as i32;
            (leap > max_leap as i32, outside, leap)
        })
        .unwrap_or_else(|| place(pc, from, range))
}

fn note(start: f32, length: f32, pitch: i32, velocity: f32, patch: &str) -> Note {
    Note {
        start,
        length,
        pitch: pitch.clamp(0, 127) as u8,
        velocity: velocity.clamp(0.01, 1.0),
        patch: patch.to_owned(),
    }
}

fn bass(style: &Style, f: &Fragment, layer: u8, rng: &mut Rng) -> Vec<Note> {
    let b = &style.bass;
    let range = b.range;
    let feel = at(&b.feel, layer);
    let mut notes = Vec::new();
    let mut last = place(
        f.chords[0].chord.root,
        (range[0] as i32 + range[1] as i32) / 2 - 5,
        range,
    );
    if feel == BassFeel::Pedal {
        let root = f.chords[0].chord.root;
        for bar in 0..f.bars {
            let p = place(root, last, range);
            notes.push(note(
                (bar * f.beats_per_bar) as f32,
                f.beats_per_bar as f32,
                p,
                b.accent,
                &b.patch,
            ));
            last = p;
        }
        return notes;
    }
    // Roots sit a fourth inside the range, so the next root is always
    // within a tritone and the line never has to jump an octave at an edge.
    let inner = [range[0] + 5, range[1] - 5];
    let mut planned: Option<i32> = None;
    for (ci, span) in f.chords.iter().enumerate() {
        let next = &f.chords[(ci + 1) % f.chords.len()];
        let scale = span.scale();
        let beats = span.length.round().max(1.0) as i32;
        let root = planned.unwrap_or_else(|| place(span.chord.root, last, inner));
        let next_root = root_after(next.chord.root, root, range, inner, b.max_leap);
        planned = Some(next_root);
        // The note that leads into the next chord's root.
        let approach = |rng: &mut Rng| -> i32 {
            let side = if next_root >= root { -1 } else { 1 };
            let cands: Vec<i32> = if rng.chance(b.chromatic_approach) {
                vec![next_root + side, next_root - side]
            } else {
                [-2, -1, 1, 2]
                    .iter()
                    .map(|d| next_root + d)
                    .filter(|&p| scale.contains(Pc::new(p)))
                    .collect()
            };
            cands
                .into_iter()
                .find(|&p| p >= range[0] as i32 && p <= range[1] as i32)
                .unwrap_or_else(|| place(next.chord.root.up(7), next_root, range))
        };
        match feel {
            BassFeel::TwoFeel => {
                notes.push(note(
                    span.start,
                    (beats.min(2)) as f32,
                    root,
                    b.accent,
                    &b.patch,
                ));
                last = root;
                if beats >= 4 {
                    let fifth = place(span.chord.tones()[2], (root + next_root) / 2, range);
                    let leap_ok = |p: i32| {
                        (p - root).abs() <= b.max_leap as i32
                            && (p - next_root).abs() <= b.max_leap as i32
                    };
                    let second = if rng.chance(0.5) || !leap_ok(fifth) {
                        approach(rng)
                    } else {
                        fifth
                    };
                    let second = if leap_ok(second) { second } else { root };
                    notes.push(note(
                        span.start + 2.0,
                        (beats - 2) as f32,
                        second,
                        b.velocity,
                        &b.patch,
                    ));
                    last = second;
                }
            }
            _ => {
                let mut pitch = root;
                for i in 0..beats {
                    let start = span.start + i as f32;
                    if i > 0 {
                        pitch = if i == beats - 1 {
                            approach(rng)
                        } else {
                            // Walk through chord tones and scale steps toward
                            // where the line must arrive.
                            let target = next_root;
                            // Mostly toward where the line must arrive, sometimes away.
                            let toward = if target >= pitch { 1 } else { -1 };
                            let dir = if rng.chance(0.25) { -toward } else { toward };
                            let tones = span.chord.tones();
                            // Stay close enough to the next root that the approach
                            // note is always a short step away.
                            let left = beats - 1 - i;
                            let reach = |p: i32| (p - target).abs() <= 7 + (left - 1) * 3;
                            let mut cands: Vec<i32> = (1..=5)
                                .flat_map(|d| [pitch + dir * d, pitch - dir * d])
                                .filter(|&p| scale.contains(Pc::new(p)))
                                .filter(|&p| p >= range[0] as i32 && p <= range[1] as i32)
                                .collect();
                            cands.sort_by_key(|&p| {
                                (
                                    !reach(p),
                                    (p - pitch).signum() != dir,
                                    !tones.contains(&Pc::new(p)),
                                    (p - pitch).abs(),
                                )
                            });
                            cands.into_iter().next().unwrap_or(pitch)
                        };
                    }
                    if (pitch - last).abs() > b.max_leap as i32 {
                        pitch = place(Pc::new(pitch), last, range);
                    }
                    let ghost = i < beats - 1 && rng.chance(b.ghost);
                    let v = if i == 0 { b.accent } else { b.velocity };
                    notes.push(note(
                        start,
                        if ghost { 0.5 } else { 1.0 },
                        pitch,
                        v,
                        &b.patch,
                    ));
                    if ghost {
                        notes.push(note(start + 0.5, 0.5, pitch, b.ghost_velocity, &b.patch));
                    }
                    last = pitch;
                }
            }
        }
    }
    notes
}

/// The pitch classes a voicing or melody may use over `span`.
fn fits(span: &ChordSpan, pc: Pc) -> bool {
    span.chord.allows(pc) || span.scale().contains(pc)
}

fn comp(style: &Style, f: &Fragment, layer: u8, rng: &mut Rng) -> Vec<Note> {
    let c = &style.comp;
    let patches = c
        .patches
        .get(layer as usize)
        .or(c.patches.last())
        .cloned()
        .unwrap_or_default();
    if patches.is_empty() || c.voicings.is_empty() {
        return Vec::new();
    }
    let [lo, hi] = at(&c.hits, layer);
    let slots_per_bar = f.beats_per_bar * 2;
    let mut notes = Vec::new();
    let mut prev: Option<Vec<u8>> = None;
    for bar in 0..f.bars {
        let hits = rng.int(lo as i32, hi as i32).max(0) as usize;
        let mut weights: Vec<f32> = (0..slots_per_bar)
            .map(|s| if s % 2 == 1 { c.offbeat_weight } else { 1.0 })
            .collect();
        let mut slots = Vec::new();
        for _ in 0..hits.min(slots_per_bar as usize) {
            let s = rng.weighted(&weights);
            weights[s] = 0.0;
            slots.push(s as u32);
        }
        slots.sort_unstable();
        for (k, &slot) in slots.iter().enumerate() {
            let start = (bar * f.beats_per_bar) as f32 + slot as f32 * 0.5;
            let Some(span) = f.chord_at(start) else {
                continue;
            };
            let end = slots
                .get(k + 1)
                .map_or(((bar + 1) * f.beats_per_bar) as f32, |&n| {
                    (bar * f.beats_per_bar) as f32 + n as f32 * 0.5
                });
            let length = rng.range(c.length).min(end - start).max(0.25);
            let first = rng.int(0, c.voicings.len() as i32 - 1) as usize;
            let voicing = (0..c.voicings.len())
                .map(|k| c.voicings[(first + k) % c.voicings.len()])
                .filter_map(|kind| span.chord.voice(kind, &c.rules, prev.as_deref()))
                .find(|v| v.iter().all(|&p| fits(span, Pc::of(p))));
            let Some(voicing) = voicing else {
                continue;
            };
            let velocity = rng.range(c.velocity);
            for patch in &patches {
                for &p in &voicing {
                    notes.push(note(start, length, p as i32, velocity, patch));
                }
            }
            prev = Some(voicing);
        }
    }
    notes
}

fn drums(style: &Style, f: &Fragment, layer: u8, rng: &mut Rng) -> Vec<Note> {
    let d = &style.drums;
    let skip = at(&d.ride_skip, layer);
    let hat = at(&d.hat_on, layer);
    let brush = at(&d.brush, layer);
    let mut notes = Vec::new();
    for bar in 0..f.bars {
        for beat in 0..f.beats_per_bar {
            let t = (bar * f.beats_per_bar + beat) as f32;
            let backbeat = beat % 2 == 1;
            let v = if backbeat {
                d.ride_velocity[1]
            } else {
                d.ride_velocity[0]
            };
            notes.push(note(t, 1.0, 60, v, &d.ride));
            if backbeat && beat + 1 < f.beats_per_bar && rng.chance(skip) {
                notes.push(note(t + 0.5, 0.5, 60, d.ride_velocity[0] * 0.8, &d.ride));
            }
            if backbeat && hat {
                notes.push(note(t, 0.5, 60, d.hat_velocity, &d.hat));
            }
            if rng.chance(brush) {
                notes.push(note(t, 1.0, 60, rng.range(d.brush_velocity), &d.brushes));
            }
        }
    }
    notes
}

fn lead(style: &Style, f: &Fragment, layer: u8, rng: &mut Rng) -> Vec<Note> {
    let l = &style.lead;
    let density = at(&l.density, layer);
    if density <= 0.0 || l.motif_beats.is_empty() {
        return Vec::new();
    }
    let beats = (*rng.pick(&l.motif_beats)).min(f.beats_per_bar).max(1);
    // The motif: which eighths sound, and the scale steps between them.
    let slots = beats * 2;
    let rhythm: Vec<u32> = (0..slots)
        .filter(|&s| s == 0 || rng.chance(l.fill))
        .collect();
    let contour: Vec<i32> = rhythm
        .iter()
        .map(|_| {
            if rng.chance(0.2) {
                *rng.pick(&[-4, -3, 3, 4])
            } else {
                rng.int(-2, 2)
            }
        })
        .collect();
    let mid = (l.range[0] as i32 + l.range[1] as i32) / 2;
    let mut last = mid;
    let mut notes: Vec<Note> = Vec::new();
    for bar in 0..f.bars {
        if !rng.chance(density) {
            continue;
        }
        let bar_start = (bar * f.beats_per_bar) as f32;
        let mut placed: Vec<(f32, i32, bool)> = Vec::new();
        for (k, &slot) in rhythm.iter().enumerate() {
            let start = bar_start + slot as f32 * 0.5;
            let Some(span) = f.chord_at(start) else {
                continue;
            };
            let scale = span.scale();
            let degrees = scale.pitches(l.range[0] as i32, l.range[1] as i32);
            if degrees.is_empty() {
                continue;
            }
            let here = degrees
                .iter()
                .position(|&p| p >= last)
                .unwrap_or(degrees.len() - 1) as i32;
            let step = if k == 0 && placed.is_empty() {
                0
            } else {
                contour[k]
            };
            let mut pitch = degrees[(here + step).clamp(0, degrees.len() as i32 - 1) as usize];
            let strong = slot % 2 == 0;
            if strong {
                // On the beat, land on a chord tone or tension.
                pitch = (0..=6)
                    .flat_map(|d| [pitch - d, pitch + d])
                    .find(|&p| {
                        p >= l.range[0] as i32
                            && p <= l.range[1] as i32
                            && span.chord.allows(Pc::new(p))
                    })
                    .unwrap_or(pitch);
            }
            if (pitch - last).abs() > l.max_leap as i32 {
                pitch = last + (pitch - last).signum() * 2;
                pitch = degrees
                    .iter()
                    .copied()
                    .min_by_key(|&p| (p - pitch).abs())
                    .unwrap_or(pitch);
                if strong && !span.chord.allows(Pc::new(pitch)) {
                    pitch = last;
                }
            }
            placed.push((start, pitch, strong));
            last = pitch;
        }
        // Chromatic approach notes on weak eighths that lead a half step
        // into the next note.
        // Backwards, so each note's successor is final before it decides.
        for i in (0..placed.len().saturating_sub(1)).rev() {
            let (_, p, strong) = placed[i];
            let next = placed[i + 1].1;
            let next_in_scale = f
                .chord_at(placed[i + 1].0)
                .is_some_and(|s| s.scale().contains(Pc::new(next)));
            if !strong && next_in_scale && rng.chance(l.chromatic) {
                let approach = next + if next > p { -1 } else { 1 };
                let span = f.chord_at(placed[i].0);
                if span.is_some_and(|s| !s.scale().contains(Pc::new(approach)))
                    && approach >= l.range[0] as i32
                    && approach <= l.range[1] as i32
                    && (approach - placed.get(i.wrapping_sub(1)).map_or(approach, |x| x.1)).abs()
                        <= l.max_leap as i32
                {
                    placed[i].1 = approach;
                }
            }
        }
        let bar_end = bar_start + f.beats_per_bar as f32;
        for (i, &(start, pitch, _)) in placed.iter().enumerate() {
            let end = placed.get(i + 1).map_or(bar_end.min(start + 1.0), |n| n.0);
            notes.push(note(
                start,
                (end - start).max(0.25),
                pitch,
                rng.range(l.velocity),
                &l.patch,
            ));
        }
    }
    notes
}

/// A short cadence into the tonic, for screens and transitions.
pub fn stinger(style: &Style, tune: &Tune, seed: u32) -> Fragment {
    let mut rng = Rng::new(seed);
    let bpb = tune.beats_per_bar;
    let tonic = tune.sections[0].chords[0].chord;
    let h = FunctionalHarmony {
        minor: 0.0,
        blues_weight: 0.0,
        aaba_weight: 1.0,
        tritone_sub: 0.0,
        altered: 0.5,
        fast_ii_v: 0.0,
        secondary: 0.0,
        bridge_keys: Vec::new(),
    };
    let bars_n = style.stinger.bars.max(1);
    let mut bars: Bars = vec![vec![tonic]; bars_n as usize];
    if bars_n >= 2 {
        bars[bars_n as usize - 2] = vec![
            two(tonic, tune.minor),
            dominant(tonic, tune.minor, &h, &mut rng),
        ];
    }
    let chords = spans(&bars, bpb);
    let mut notes = Vec::new();
    let mut prev: Option<Vec<u8>> = None;
    let mut last = (style.bass.range[0] as i32 + style.bass.range[1] as i32) / 2 - 5;
    for span in &chords {
        let root = place(span.chord.root, last, style.bass.range);
        notes.push(note(
            span.start,
            span.length,
            root,
            style.bass.accent,
            &style.bass.patch,
        ));
        last = root;
        let voicing = style
            .comp
            .voicings
            .iter()
            .filter_map(|&k| span.chord.voice(k, &style.comp.rules, prev.as_deref()))
            .find(|v| v.iter().all(|&p| fits(span, Pc::of(p))));
        if let Some(v) = voicing {
            for patch in &style.stinger.patches {
                for &p in &v {
                    notes.push(note(
                        span.start,
                        span.length,
                        p as i32,
                        style.comp.velocity[1],
                        patch,
                    ));
                }
            }
            prev = Some(v);
        }
    }
    Fragment {
        role: Role::Stinger,
        layer: 0,
        section: "stinger".into(),
        bars: bars_n,
        beats_per_bar: bpb,
        style: style.name.clone(),
        chords,
        notes,
    }
}

// ---- Rules -------------------------------------------------------------------

/// A broken rule, in words.
#[derive(Clone, Debug, PartialEq)]
pub struct Violation(pub String);

/// Checks the hard rules a fragment must obey.
pub fn validate(style: &Style, f: &Fragment) -> Vec<Violation> {
    let mut out = Vec::new();
    let mut fail = |msg: String| out.push(Violation(msg));
    let beats = f.beats();
    for n in &f.notes {
        if n.start < 0.0 || n.start >= beats || n.length <= 0.0 || n.start + n.length > beats + 1e-3
        {
            fail(format!("note outside the fragment: {n:?}"));
        }
        if !(n.velocity > 0.0 && n.velocity <= 1.0) {
            fail(format!("bad velocity: {n:?}"));
        }
        if (n.start * 2.0).fract().abs() > 1e-4 {
            fail(format!("off the eighth-note grid: {n:?}"));
        }
        if f.chord_at(n.start).is_none() {
            fail(format!("no chord under {n:?}"));
        }
    }
    let allowed: Vec<&str> = match f.role {
        Role::Bass => vec![&style.bass.patch],
        Role::Comp => style
            .comp
            .patches
            .iter()
            .flatten()
            .map(String::as_str)
            .collect(),
        Role::Drums => vec![&style.drums.ride, &style.drums.hat, &style.drums.brushes],
        Role::Lead => vec![&style.lead.patch],
        Role::Stinger => std::iter::once(style.bass.patch.as_str())
            .chain(style.stinger.patches.iter().map(String::as_str))
            .collect(),
    };
    for n in &f.notes {
        if !allowed.contains(&n.patch.as_str()) {
            fail(format!("patch {:?} not for {:?}", n.patch, f.role));
        }
    }
    let bass: Vec<&Note> = f
        .notes
        .iter()
        .filter(|n| n.patch == style.bass.patch)
        .collect();
    if matches!(f.role, Role::Bass | Role::Stinger) {
        check_bass(style, f, &bass, &mut fail);
    }
    if matches!(f.role, Role::Comp | Role::Stinger) {
        check_chords(style, f, &mut fail);
    }
    if f.role == Role::Lead {
        check_lead(style, f, &mut fail);
    }
    out
}

fn check_bass(style: &Style, f: &Fragment, notes: &[&Note], fail: &mut impl FnMut(String)) {
    let b = &style.bass;
    let pedal = f.role == Role::Bass && at(&b.feel, f.layer) == BassFeel::Pedal;
    for w in notes.windows(2) {
        if w[1].start < w[0].start + w[0].length - 1e-3 {
            fail(format!("bass notes overlap: {:?} {:?}", w[0], w[1]));
        }
        if (w[1].pitch as i32 - w[0].pitch as i32).unsigned_abs() > b.max_leap as u32 {
            fail(format!("bass leaps too far: {:?} {:?}", w[0], w[1]));
        }
    }
    for n in notes {
        if n.pitch < b.range[0] || n.pitch > b.range[1] {
            fail(format!("bass out of range: {n:?}"));
        }
    }
    for span in &f.chords {
        let root = if pedal {
            f.chords[0].chord.root
        } else {
            span.chord.root
        };
        if span.start.fract() != 0.0 {
            continue;
        }
        match notes.iter().find(|n| n.start == span.start) {
            Some(n) if Pc::of(n.pitch) != root => fail(format!(
                "bass plays {} not the root {} at beat {}",
                Pc::of(n.pitch),
                root,
                span.start
            )),
            None if !pedal || span.start % f.beats_per_bar as f32 == 0.0 => {
                fail(format!("no bass on the change at beat {}", span.start))
            }
            _ => {}
        }
    }
}

fn check_chords(style: &Style, f: &Fragment, fail: &mut impl FnMut(String)) {
    let mut groups: Vec<(f32, &str, Vec<u8>)> = Vec::new();
    for n in f.notes.iter().filter(|n| n.patch != style.bass.patch) {
        match groups.iter_mut().find(|g| g.0 == n.start && g.1 == n.patch) {
            Some(g) => g.2.push(n.pitch),
            None => groups.push((n.start, &n.patch, vec![n.pitch])),
        }
    }
    for (start, patch, mut pitches) in groups {
        pitches.sort_unstable();
        if !style.comp.rules.ok(&pitches) {
            fail(format!(
                "{patch} voicing {pitches:?} at {start} breaks the voicing rules"
            ));
        }
        if let Some(span) = f.chord_at(start)
            && let Some(p) = pitches.iter().find(|&&p| !fits(span, Pc::of(p)))
        {
            fail(format!(
                "{patch} plays {} over {} at {start}",
                Pc::of(*p),
                span.chord
            ));
        }
    }
}

fn check_lead(style: &Style, f: &Fragment, fail: &mut impl FnMut(String)) {
    let l = &style.lead;
    let notes = &f.notes;
    for (i, n) in notes.iter().enumerate() {
        if n.pitch < l.range[0] || n.pitch > l.range[1] {
            fail(format!("lead out of range: {n:?}"));
        }
        let Some(span) = f.chord_at(n.start) else {
            continue;
        };
        let pc = Pc::of(n.pitch);
        if n.start.fract() == 0.0 && !span.chord.allows(pc) {
            fail(format!(
                "lead lands on {pc} over {} on beat {}",
                span.chord, n.start
            ));
        }
        if !span.scale().contains(pc) {
            match notes.get(i + 1) {
                Some(next)
                    if (next.pitch as i32 - n.pitch as i32).abs() <= 2
                        && f.chord_at(next.start)
                            .is_some_and(|s| s.scale().contains(Pc::of(next.pitch))) => {}
                _ => fail(format!(
                    "chromatic {pc} at {} doesn't resolve by step",
                    n.start
                )),
            }
        }
        if let Some(next) = notes.get(i + 1) {
            if next.start < n.start + n.length - 1e-3 {
                fail(format!("lead notes overlap at {}", n.start));
            }
            let same_bar = (next.start / f.beats_per_bar as f32).floor()
                == (n.start / f.beats_per_bar as f32).floor();
            if same_bar && (next.pitch as i32 - n.pitch as i32).unsigned_abs() > l.max_leap as u32 {
                fail(format!("lead leaps too far at {}", n.start));
            }
        }
    }
}

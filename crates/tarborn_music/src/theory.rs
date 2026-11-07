//! The jazz theory core: pitch classes, modes, chord symbols, chord-scales,
//! voicings and voice leading.
//!
//! Everything here is plain functions over small integers (MIDI pitches and
//! pitch classes), so the grammar, the metrics and the director can share
//! one vocabulary and it can all be property tested.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// A pitch class, 0 (C) to 11 (B).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pc(u8);

const NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

impl Pc {
    pub fn new(n: i32) -> Self {
        Self(n.rem_euclid(12) as u8)
    }

    pub fn of(pitch: u8) -> Self {
        Self(pitch % 12)
    }

    pub fn index(self) -> u8 {
        self.0
    }

    /// Transposed up by `semitones`.
    pub fn up(self, semitones: i32) -> Self {
        Self::new(self.0 as i32 + semitones)
    }

    /// Semitones up from `self` to `other`, 0 to 11.
    pub fn to(self, other: Pc) -> u8 {
        (other.0 + 12 - self.0) % 12
    }

    /// Parses a note name: a letter, then any number of `b` or `#`.
    pub fn parse(name: &str) -> Option<(Self, usize)> {
        let mut chars = name.char_indices();
        let (_, letter) = chars.next()?;
        let base = match letter {
            'C' => 0,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => return None,
        };
        let mut n = base;
        let mut used = 1;
        for (i, c) in chars {
            match c {
                'b' => n -= 1,
                '#' => n += 1,
                _ => break,
            }
            used = i + 1;
        }
        Some((Self::new(n), used))
    }

    /// The pitch of this class nearest to `pitch` (ties go down).
    pub fn nearest(self, pitch: i32) -> i32 {
        let down = (pitch - self.0 as i32).rem_euclid(12);
        if down <= 6 {
            pitch - down
        } else {
            pitch + 12 - down
        }
    }
}

impl fmt::Display for Pc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(NAMES[self.0 as usize])
    }
}

/// Scales and modes used in jazz, as semitone steps from the root.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub enum Mode {
    Ionian,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Aeolian,
    Locrian,
    /// Jazz melodic minor (same up and down).
    MelodicMinor,
    /// Locrian with a natural 2: the sixth mode of melodic minor.
    LocrianNat2,
    /// Lydian with a b7: the fourth mode of melodic minor.
    LydianDominant,
    /// Super-locrian: the seventh mode of melodic minor, for altered dominants.
    Altered,
    HarmonicMinor,
    /// The fifth mode of harmonic minor, for V7b9 in minor keys.
    PhrygianDominant,
    /// Whole-half diminished, for dim7.
    Diminished,
    /// Half-whole diminished, for 7b9 13.
    DominantDiminished,
    WholeTone,
    /// Minor blues: 1 b3 4 b5 5 b7.
    Blues,
}

impl Mode {
    pub fn intervals(self) -> &'static [u8] {
        match self {
            Mode::Ionian => &[0, 2, 4, 5, 7, 9, 11],
            Mode::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Mode::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Mode::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            Mode::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Mode::Aeolian => &[0, 2, 3, 5, 7, 8, 10],
            Mode::Locrian => &[0, 1, 3, 5, 6, 8, 10],
            Mode::MelodicMinor => &[0, 2, 3, 5, 7, 9, 11],
            Mode::LocrianNat2 => &[0, 2, 3, 5, 6, 8, 10],
            Mode::LydianDominant => &[0, 2, 4, 6, 7, 9, 10],
            Mode::Altered => &[0, 1, 3, 4, 6, 8, 10],
            Mode::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            Mode::PhrygianDominant => &[0, 1, 4, 5, 7, 8, 10],
            Mode::Diminished => &[0, 2, 3, 5, 6, 8, 9, 11],
            Mode::DominantDiminished => &[0, 1, 3, 4, 6, 7, 9, 10],
            Mode::WholeTone => &[0, 2, 4, 6, 8, 10],
            Mode::Blues => &[0, 3, 5, 6, 7, 10],
        }
    }

    pub const ALL: [Mode; 17] = [
        Mode::Ionian,
        Mode::Dorian,
        Mode::Phrygian,
        Mode::Lydian,
        Mode::Mixolydian,
        Mode::Aeolian,
        Mode::Locrian,
        Mode::MelodicMinor,
        Mode::LocrianNat2,
        Mode::LydianDominant,
        Mode::Altered,
        Mode::HarmonicMinor,
        Mode::PhrygianDominant,
        Mode::Diminished,
        Mode::DominantDiminished,
        Mode::WholeTone,
        Mode::Blues,
    ];
}

/// A mode on a root.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Scale {
    pub root: Pc,
    pub mode: Mode,
}

impl Scale {
    pub fn new(root: Pc, mode: Mode) -> Self {
        Self { root, mode }
    }

    pub fn pcs(&self) -> Vec<Pc> {
        self.mode
            .intervals()
            .iter()
            .map(|&i| self.root.up(i as i32))
            .collect()
    }

    pub fn contains(&self, pc: Pc) -> bool {
        self.mode.intervals().contains(&self.root.to(pc))
    }

    /// The scale degree `n` steps from the root (0 is the root), wrapping
    /// into higher octaves as a pitch offset in semitones.
    pub fn degree(&self, n: i32) -> i32 {
        let iv = self.mode.intervals();
        let len = iv.len() as i32;
        let octave = n.div_euclid(len);
        self.root.index() as i32 + iv[n.rem_euclid(len) as usize] as i32 + 12 * octave
    }

    /// The scale pitches from `lo` to `hi`, inclusive, ascending.
    pub fn pitches(&self, lo: i32, hi: i32) -> Vec<i32> {
        (lo..=hi).filter(|&p| self.contains(Pc::new(p))).collect()
    }
}

/// Chord qualities, with their tones and tensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Quality {
    Maj7,
    Maj6,
    /// maj7#11: the Lydian major chord.
    Maj7Sharp11,
    Dom7,
    /// 7#11: Lydian dominant (tritone subs love it).
    Dom7Sharp11,
    /// 7b9: the minor-key dominant.
    Dom7Flat9,
    /// 7alt: altered dominant, b9 #9 #11 b13.
    Dom7Alt,
    /// 7sus4.
    Sus,
    Min7,
    Min6,
    MinMaj7,
    /// m7b5.
    HalfDim,
    Dim7,
}

impl Quality {
    pub const ALL: [Quality; 13] = [
        Quality::Maj7,
        Quality::Maj6,
        Quality::Maj7Sharp11,
        Quality::Dom7,
        Quality::Dom7Sharp11,
        Quality::Dom7Flat9,
        Quality::Dom7Alt,
        Quality::Sus,
        Quality::Min7,
        Quality::Min6,
        Quality::MinMaj7,
        Quality::HalfDim,
        Quality::Dim7,
    ];

    /// The symbol suffix, as written after the root.
    pub fn suffix(self) -> &'static str {
        match self {
            Quality::Maj7 => "maj7",
            Quality::Maj6 => "6",
            Quality::Maj7Sharp11 => "maj7#11",
            Quality::Dom7 => "7",
            Quality::Dom7Sharp11 => "7#11",
            Quality::Dom7Flat9 => "7b9",
            Quality::Dom7Alt => "7alt",
            Quality::Sus => "7sus",
            Quality::Min7 => "m7",
            Quality::Min6 => "m6",
            Quality::MinMaj7 => "mMaj7",
            Quality::HalfDim => "m7b5",
            Quality::Dim7 => "dim7",
        }
    }

    /// Root, third, fifth and seventh (or sixth), in semitones.
    pub fn tones(self) -> [u8; 4] {
        match self {
            Quality::Maj7 | Quality::Maj7Sharp11 => [0, 4, 7, 11],
            Quality::Maj6 => [0, 4, 7, 9],
            Quality::Dom7 | Quality::Dom7Sharp11 | Quality::Dom7Flat9 => [0, 4, 7, 10],
            Quality::Dom7Alt => [0, 4, 8, 10],
            Quality::Sus => [0, 5, 7, 10],
            Quality::Min7 => [0, 3, 7, 10],
            Quality::Min6 => [0, 3, 7, 9],
            Quality::MinMaj7 => [0, 3, 7, 11],
            Quality::HalfDim => [0, 3, 6, 10],
            Quality::Dim7 => [0, 3, 6, 9],
        }
    }

    /// Tensions a voicing may add: the 9 first, then the others.
    pub fn tensions(self) -> &'static [u8] {
        match self {
            Quality::Maj7 | Quality::Maj6 => &[2, 9],
            Quality::Maj7Sharp11 => &[2, 6, 9],
            Quality::Dom7 => &[2, 9],
            Quality::Dom7Sharp11 => &[2, 6, 9],
            Quality::Dom7Flat9 => &[1, 8],
            Quality::Dom7Alt => &[1, 3, 6],
            Quality::Sus => &[2, 9],
            Quality::Min7 | Quality::Min6 | Quality::MinMaj7 => &[2, 5],
            Quality::HalfDim => &[2, 5],
            Quality::Dim7 => &[2, 5, 8],
        }
    }

    /// The chord-scale: the mode improvisers and voicings draw from.
    pub fn mode(self) -> Mode {
        match self {
            Quality::Maj7 | Quality::Maj6 => Mode::Ionian,
            Quality::Maj7Sharp11 => Mode::Lydian,
            Quality::Dom7 | Quality::Sus => Mode::Mixolydian,
            Quality::Dom7Sharp11 => Mode::LydianDominant,
            Quality::Dom7Flat9 => Mode::PhrygianDominant,
            Quality::Dom7Alt => Mode::Altered,
            Quality::Min7 => Mode::Dorian,
            Quality::Min6 | Quality::MinMaj7 => Mode::MelodicMinor,
            Quality::HalfDim => Mode::LocrianNat2,
            Quality::Dim7 => Mode::Diminished,
        }
    }

    pub fn is_dominant(self) -> bool {
        matches!(
            self,
            Quality::Dom7
                | Quality::Dom7Sharp11
                | Quality::Dom7Flat9
                | Quality::Dom7Alt
                | Quality::Sus
        )
    }
}

/// A chord: root and quality. Written and read as a symbol ("Dm7", "G7alt").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub root: Pc,
    pub quality: Quality,
}

impl Chord {
    pub fn new(root: Pc, quality: Quality) -> Self {
        Self { root, quality }
    }

    /// Parses a chord symbol.
    pub fn parse(symbol: &str) -> Option<Self> {
        let (root, used) = Pc::parse(symbol)?;
        let rest = &symbol[used..];
        let quality = Quality::ALL.into_iter().find(|q| q.suffix() == rest)?;
        Some(Self::new(root, quality))
    }

    pub fn scale(&self) -> Scale {
        Scale::new(self.root, self.quality.mode())
    }

    /// Pitch classes of root, third, fifth and seventh.
    pub fn tones(&self) -> [Pc; 4] {
        self.quality.tones().map(|i| self.root.up(i as i32))
    }

    pub fn tensions(&self) -> Vec<Pc> {
        self.quality
            .tensions()
            .iter()
            .map(|&i| self.root.up(i as i32))
            .collect()
    }

    /// Chord tones and tensions together.
    pub fn allows(&self, pc: Pc) -> bool {
        self.tones().contains(&pc) || self.tensions().contains(&pc)
    }

    /// The tritone substitute of a dominant: same tritone, root a b5 away,
    /// written 7#11 as it is usually played. Non-dominants come back as is.
    pub fn tritone_sub(&self) -> Self {
        if self.quality.is_dominant() {
            Self::new(self.root.up(6), Quality::Dom7Sharp11)
        } else {
            *self
        }
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.root, self.quality.suffix())
    }
}

impl Serialize for Chord {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Chord {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Chord::parse(&text).ok_or_else(|| serde::de::Error::custom(format!("bad chord {text:?}")))
    }
}

/// Voicing shapes, as chord-relative intervals from the bottom voice up.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub enum VoicingKind {
    /// Root, 3rd and 7th: the left-hand shell.
    Shell,
    /// Rootless "A": 3, 5, 7, 9.
    RootlessA,
    /// Rootless "B": 7, 9, 3, 5.
    RootlessB,
    /// Stacked fourths from the chord-scale (McCoy Tyner): four voices.
    Quartal,
    /// Root, 5th, 9th (the chord's own fifth and ninth): open fifths for
    /// modal vamps.
    OpenFifths,
}

impl VoicingKind {
    pub const ALL: [VoicingKind; 5] = [
        VoicingKind::Shell,
        VoicingKind::RootlessA,
        VoicingKind::RootlessB,
        VoicingKind::Quartal,
        VoicingKind::OpenFifths,
    ];

    pub fn voices(self) -> usize {
        match self {
            VoicingKind::Shell | VoicingKind::OpenFifths => 3,
            _ => 4,
        }
    }
}

/// Where voicings may go. Comes from the style file.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct VoicingRules {
    /// Lowest and highest MIDI pitch any voice may take.
    pub range: [u8; 2],
    /// Below this pitch, neighbouring voices must be at least
    /// `low_interval` semitones apart, or the chord turns to mud.
    pub low_limit: u8,
    pub low_interval: u8,
}

impl Default for VoicingRules {
    fn default() -> Self {
        Self {
            range: [48, 79],
            low_limit: 52,
            low_interval: 5,
        }
    }
}

impl Chord {
    /// The pitch classes of a voicing, bottom up.
    fn shape(&self, kind: VoicingKind) -> Vec<Pc> {
        let [r, third, fifth, seventh] = self.tones();
        let ninth = self.tensions()[0];
        match kind {
            VoicingKind::Shell => vec![r, third, seventh],
            VoicingKind::RootlessA => vec![third, fifth, seventh, ninth],
            VoicingKind::RootlessB => vec![seventh, ninth, third, fifth],
            VoicingKind::OpenFifths => vec![r, fifth, ninth],
            VoicingKind::Quartal => {
                // Four stacked fourths from the chord-scale tone that keeps
                // the most of them inside the scale, preferring to start on
                // the third, seventh, ninth, then root (So What style).
                let scale = self.scale();
                let mut starts = vec![third, seventh, ninth, r];
                starts.extend(scale.pcs());
                let inside = |s: Pc| (0..4).filter(|&k| scale.contains(s.up(5 * k))).count();
                let best = starts.iter().map(|&s| inside(s)).max().unwrap_or(0);
                let start = starts
                    .into_iter()
                    .find(|&s| inside(s) == best)
                    .unwrap_or(third);
                (0..4).map(|k| start.up(5 * k)).collect()
            }
        }
    }

    /// Every voicing of `kind` within the rules, each ascending.
    pub fn voicings(&self, kind: VoicingKind, rules: &VoicingRules) -> Vec<Vec<u8>> {
        let shape = self.shape(kind);
        let [lo, hi] = rules.range;
        let mut out = Vec::new();
        for bottom in lo..=hi {
            if Pc::of(bottom) != shape[0] {
                continue;
            }
            let mut v = vec![bottom];
            for pc in &shape[1..] {
                let last = *v.last().unwrap() as i32;
                let next = last + Pc::of(last as u8).to(*pc) as i32;
                // Never a unison: a voice equal to the one below goes up an octave.
                let next = if next == last { next + 12 } else { next };
                v.push(next.min(255) as u8);
            }
            if v.iter().all(|&p| p <= hi) && rules.ok(&v) {
                out.push(v);
            }
        }
        out
    }

    /// The voicing of `kind` that moves least from `previous` (or sits
    /// nearest the middle of the range when there is none).
    pub fn voice(
        &self,
        kind: VoicingKind,
        rules: &VoicingRules,
        previous: Option<&[u8]>,
    ) -> Option<Vec<u8>> {
        let centre = (rules.range[0] as i32 + rules.range[1] as i32) / 2;
        self.voicings(kind, rules)
            .into_iter()
            .min_by_key(|v| match previous {
                Some(p) => voice_leading_cost(p, v) * 1000 + centre_distance(v, centre),
                None => centre_distance(v, centre),
            })
    }
}

fn centre_distance(v: &[u8], centre: i32) -> u32 {
    let mean = v.iter().map(|&p| p as i32).sum::<i32>() / v.len().max(1) as i32;
    (mean - centre).unsigned_abs()
}

impl VoicingRules {
    /// Whether a voicing (ascending) is in range and clear down low.
    pub fn ok(&self, v: &[u8]) -> bool {
        let [lo, hi] = self.range;
        v.iter().all(|&p| (lo..=hi).contains(&p))
            && v.windows(2).all(|w| {
                w[1] > w[0] && (w[0] >= self.low_limit || w[1] - w[0] >= self.low_interval)
            })
    }
}

/// Total semitones the voices move from `a` to `b`, with each voice of the
/// larger chord led to one of the smaller in order (so voices may merge or
/// split, but never cross). Both must be ascending.
pub fn voice_leading_cost(a: &[u8], b: &[u8]) -> u32 {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    // dp[i][j]: cost of leading large[..=j] with large[j] on small[i], every
    // small voice up to i used.
    let (n, m) = (small.len(), large.len());
    let inf = u32::MAX / 2;
    let mut dp = vec![vec![inf; m]; n];
    let d = |i: usize, j: usize| (small[i] as i32 - large[j] as i32).unsigned_abs();
    dp[0][0] = d(0, 0);
    for j in 1..m {
        dp[0][j] = dp[0][j - 1] + d(0, j);
        for i in 1..n.min(j + 1) {
            dp[i][j] = dp[i][j - 1].min(dp[i - 1][j - 1]) + d(i, j);
        }
    }
    dp[n - 1][m - 1]
}

/// Swing ratio that eases as the tempo rises: drummers swing hard at
/// ballad tempos and play nearly even eighths at 300 BPM.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SwingCurve {
    pub slow_bpm: f32,
    pub slow_ratio: f32,
    pub fast_bpm: f32,
    pub fast_ratio: f32,
}

impl Default for SwingCurve {
    fn default() -> Self {
        Self {
            slow_bpm: 100.0,
            slow_ratio: 0.67,
            fast_bpm: 280.0,
            fast_ratio: 0.55,
        }
    }
}

impl SwingCurve {
    pub fn ratio(&self, bpm: f32) -> f32 {
        let span = (self.fast_bpm - self.slow_bpm).max(1e-3);
        let u = ((bpm - self.slow_bpm) / span).clamp(0.0, 1.0);
        self.slow_ratio + (self.fast_ratio - self.slow_ratio) * u
    }
}

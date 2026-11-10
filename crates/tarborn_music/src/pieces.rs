//! Short pieces for the screens around play: an intro on the start screen,
//! a stinger between stages and a ballad fragment on death. Each is cut from
//! a library tune, so it shares the stage's harmony and instruments. A
//! [`Fade`] moves a gain bar by bar, so one piece hands over to the next (or
//! to the band) on a bar line; a [`PiecePlayer`] lays a piece on the music
//! timeline and hands out its bars as they come due.
//!
//! Which section, layer, roles, tempo and fades each piece uses live in
//! `screens` in `assets/music.ron`.

use crate::Note;
use crate::score::Role;
use crate::search::{LibraryTune, realize};
use crate::style::Style;
use serde::Deserialize;

/// Which screen piece.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PieceKind {
    /// The start screen, looped.
    Intro,
    /// Between stages.
    Stinger,
    /// On death.
    Ballad,
}

/// How a piece is cut from its tune.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PieceParams {
    /// Play the tune's own stinger instead of a section of its form.
    pub stinger: bool,
    /// Section played: index into the tune's form, counted from the end
    /// when negative (-1 is the last section).
    pub form_at: i32,
    /// Intensity layer of that section (each layer is a whole arrangement).
    pub layer: u8,
    /// Roles kept; empty keeps every role.
    pub roles: Vec<Role>,
    /// Most bars played; 0 plays them all.
    pub max_bars: u32,
    /// Tempo as a share of the tune's.
    pub tempo_scale: f32,
    /// Velocity scale of every note.
    pub velocity: f32,
    /// Start over at the end, until the screen changes.
    pub looped: bool,
    /// Bars over which the piece fades in.
    pub fade_in_bars: u32,
}

impl Default for PieceParams {
    fn default() -> Self {
        Self {
            stinger: false,
            form_at: 0,
            layer: 0,
            roles: Vec::new(),
            max_bars: 0,
            tempo_scale: 1.0,
            velocity: 1.0,
            looped: false,
            fade_in_bars: 0,
        }
    }
}

/// The `screens` section of `music.ron`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ScreenParams {
    /// Bars over which what was playing fades out once the screen changes.
    pub fade_bars: u32,
    /// Bars over which the band fades back in when play starts.
    pub band_fade_in_bars: u32,
    pub intro: PieceParams,
    pub stinger: PieceParams,
    pub ballad: PieceParams,
}

impl Default for ScreenParams {
    fn default() -> Self {
        Self {
            fade_bars: 1,
            band_fade_in_bars: 1,
            intro: PieceParams {
                layer: 1,
                looped: true,
                fade_in_bars: 1,
                ..Default::default()
            },
            stinger: PieceParams {
                stinger: true,
                ..Default::default()
            },
            ballad: PieceParams {
                form_at: -1,
                roles: vec![Role::Bass, Role::Comp],
                max_bars: 4,
                tempo_scale: 0.5,
                velocity: 0.7,
                ..Default::default()
            },
        }
    }
}

impl ScreenParams {
    pub fn of(&self, kind: PieceKind) -> &PieceParams {
        match kind {
            PieceKind::Intro => &self.intro,
            PieceKind::Stinger => &self.stinger,
            PieceKind::Ballad => &self.ballad,
        }
    }
}

/// A screen piece, bar by bar.
#[derive(Clone, Debug)]
pub struct Piece {
    pub kind: PieceKind,
    pub bpm: f32,
    pub swing: f32,
    pub beats_per_bar: u32,
    /// Notes of each bar, starting in beats from its bar line.
    pub bars: Vec<Vec<Note>>,
    pub looped: bool,
    pub fade_in_bars: u32,
}

/// Cuts piece `kind` from the library tune `seed` picks. None when the
/// library is empty or the cut holds no notes.
pub fn piece(
    style: &Style,
    library: &[LibraryTune],
    kind: PieceKind,
    params: &ScreenParams,
    seed: u32,
) -> Option<Piece> {
    let p = params.of(kind);
    let entry = library.get(seed as usize % library.len().max(1))?;
    let realized = realize(style, entry);
    let tune = &realized.tune;
    let bpb = tune.beats_per_bar.max(1);
    let (notes, bars): (Vec<Note>, u32) = if p.stinger {
        let s = &realized.stinger;
        (s.notes.clone(), s.bars)
    } else {
        let n = tune.form.len() as i32;
        if n == 0 {
            return None;
        }
        let at = if p.form_at < 0 {
            n + p.form_at
        } else {
            p.form_at
        };
        let section = tune.form[at.clamp(0, n - 1) as usize];
        let notes = realized
            .parts
            .iter()
            .filter(|(lp, fs)| {
                lp.section == section
                    && lp.layer == p.layer
                    && (p.roles.is_empty() || p.roles.contains(&lp.role))
                    && !fs.is_empty()
            })
            // The best variant of each part.
            .flat_map(|(_, fs)| fs[0].notes.iter().cloned())
            .collect();
        (notes, tune.sections[section].bars)
    };
    let bars = if p.max_bars > 0 {
        bars.min(p.max_bars)
    } else {
        bars
    }
    .max(1);
    let mut out = vec![Vec::new(); bars as usize];
    for mut n in notes {
        let bar = (n.start / bpb as f32).floor();
        if bar < 0.0 || bar as u32 >= bars {
            continue;
        }
        n.start -= bar * bpb as f32;
        n.velocity = (n.velocity * p.velocity).clamp(0.0, 1.0);
        out[bar as usize].push(n);
    }
    if out.iter().all(Vec::is_empty) {
        return None;
    }
    for bar in &mut out {
        bar.sort_by(|a, b| a.start.total_cmp(&b.start));
    }
    let bpm = (tune.bpm * p.tempo_scale).max(1.0);
    Some(Piece {
        kind,
        bpm,
        swing: style.swing.ratio(bpm),
        beats_per_bar: bpb,
        bars: out,
        looped: p.looped,
        fade_in_bars: p.fade_in_bars,
    })
}

/// A gain that steps from one level to another over whole bars: bar
/// `start` is the first that moves, and from bar `start + bars` on it holds
/// the new level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    pub start: i64,
    pub bars: u32,
    pub from: f32,
    pub to: f32,
}

impl Fade {
    /// A level that never moves.
    pub fn steady(level: f32) -> Self {
        Self {
            start: i64::MIN,
            bars: 0,
            from: level,
            to: level,
        }
    }

    /// From wherever `self` is at bar `start`, to `to` over `bars` bars.
    pub fn towards(&self, start: i64, bars: u32, to: f32) -> Self {
        Self {
            start,
            bars,
            from: self.gain(start.saturating_sub(1)),
            to,
        }
    }

    pub fn gain(&self, bar: i64) -> f32 {
        if bar < self.start {
            return self.from;
        }
        let t = (bar.saturating_sub(self.start).saturating_add(1) as f32 / (self.bars + 1) as f32)
            .min(1.0);
        self.from + (self.to - self.from) * t
    }

    /// Silent from `bar` on.
    pub fn silent_from(&self, bar: i64) -> bool {
        self.to <= 0.0 && (self.gain(bar) <= 0.0)
    }
}

/// A piece laid on the music timeline (in sim ticks), from tick `start`.
#[derive(Clone, Debug)]
pub struct PiecePlayer {
    pub piece: Piece,
    pub start: f64,
    ticks_per_second: f64,
    next: i64,
    pub fade: Fade,
}

impl PiecePlayer {
    pub fn new(piece: Piece, start: f64, ticks_per_second: f32) -> Self {
        let fade = Fade::steady(0.0).towards(0, piece.fade_in_bars, 1.0);
        Self {
            piece,
            start,
            ticks_per_second: ticks_per_second as f64,
            next: 0,
            fade,
        }
    }

    fn ticks_per_beat(&self) -> f64 {
        60.0 / self.piece.bpm as f64 * self.ticks_per_second
    }

    /// The tick on which `beat` of the piece falls.
    pub fn tick_of_beat(&self, beat: f64) -> f64 {
        self.start + beat * self.ticks_per_beat()
    }

    /// The tick of bar line `bar`.
    pub fn bar_tick(&self, bar: i64) -> f64 {
        self.tick_of_beat((bar * self.piece.beats_per_bar as i64) as f64)
    }

    /// The next bar not yet handed out.
    pub fn next_bar(&self) -> i64 {
        self.next
    }

    fn ended(&self, bar: i64) -> bool {
        (!self.piece.looped && bar >= self.piece.bars.len() as i64) || self.fade.silent_from(bar)
    }

    /// Nothing more to play.
    pub fn done(&self) -> bool {
        self.ended(self.next)
    }

    /// Fades out over `bars` bars from the next bar not yet handed out.
    pub fn fade_out(&mut self, bars: u32) {
        self.fade = self.fade.towards(self.next, bars, 0.0);
    }

    /// Bars whose bar line is at or before `horizon`, with their gain: the
    /// bar index (its first beat is `bar * beats_per_bar`) and its notes.
    pub fn due(&mut self, horizon: f64) -> Vec<(i64, f32, &[Note])> {
        let first = self.next;
        while !self.ended(self.next) && self.bar_tick(self.next) <= horizon {
            self.next += 1;
        }
        let n = self.piece.bars.len() as i64;
        (first..self.next)
            .map(|bar| {
                let notes = &self.piece.bars[bar.rem_euclid(n) as usize];
                (bar, self.fade.gain(bar), notes.as_slice())
            })
            .collect()
    }
}

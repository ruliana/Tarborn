//! Music: one long-lived audio stream that plays notes scheduled against the
//! sim's beat clock.
//!
//! The sim is the master clock (see `tarborn_sim::beat`). Each frame the
//! game reads how many samples the audio device has taken, feeds that and
//! the sim tick to [`AudioSync`], and schedules what's due in the next
//! `lookahead` seconds at the matching stream sample. The stream itself only
//! mixes; it never decides timing, so a dropped buffer can't touch gameplay.
//!
//! The live director (`tarborn_music::director`) decides each bar ahead of
//! time; its notes are voiced and scheduled like everything else. A debug
//! click track (off by default, `click` in `assets/music.ron`) can play on
//! top, and the measured output latency is logged.
//!
//! Around play, each screen has its own piece cut from the library (an
//! intro on the start screen, a stinger between stages, a ballad fragment
//! on death; see `tarborn_music::pieces`). The music keeps its own time
//! ([`Timeline`]) while the sim is frozen, and a screen change hands over
//! on a bar line with bar-by-bar fades.
//!
//! Every number lives in `assets/music.ron`, reloaded when it changes.

use bevy::audio::{AddAudioSource, ChannelCount, Decodable, SampleRate, Source};
use bevy::prelude::*;
use std::collections::VecDeque;
use std::num::NonZero;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tarborn_music::director::{Bar, Director, Input, Voicer};
use tarborn_music::drummer::{Cue, Drummer, Moment};
use tarborn_music::mood::{Mood, MoodParams, Scene};
use tarborn_music::pieces::{Fade, PieceKind, PiecePlayer, piece};
use tarborn_music::search::LibraryTune;
use tarborn_music::stream::{AudioSync, LatencyMeter, Mixer, Scheduled, StreamParams, Timeline};
use tarborn_music::style::Style;
use tarborn_music::{MusicFile, PatchBank, from_ron};
use tarborn_sfx::soft_limit;
use tarborn_sim::{
    AttackPhase, BeatClock, BeatSnap, DT, Event, HitKind, Sim, State, TICKS_PER_SECOND,
};

/// The music file built into the game, used when the file on disk is missing.
const BUILT_IN: &str = include_str!("../assets/music.ron");
/// The same for the band's data, by path under `assets/music/`.
const BUILT_IN_DATA: [(&str, &str); 5] = [
    ("patches.ron", include_str!("../assets/music/patches.ron")),
    (
        "styles/industrial.ron",
        include_str!("../assets/music/styles/industrial.ron"),
    ),
    (
        "styles/forest.ron",
        include_str!("../assets/music/styles/forest.ron"),
    ),
    (
        "library/industrial.ron",
        include_str!("../assets/music/library/industrial.ron"),
    ),
    (
        "library/forest.ron",
        include_str!("../assets/music/library/forest.ron"),
    ),
];

/// Samples between two looks at the inbox from the audio thread.
const INBOX_EVERY: u64 = 64;

pub struct MusicPlugin;

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<MusicStream>()
            .init_resource::<MusicCues>()
            .add_systems(Startup, start_music)
            .add_systems(
                Update,
                (reload_music, follow_stage, toggle_mute, schedule_music).chain(),
            );
    }
}

/// What the music plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    /// The live band, following the game.
    Band,
    /// A screen piece.
    Piece(PieceKind),
}

/// What each screen plays; None keeps what is playing (while loading).
pub fn program_for(flow: &crate::flow::Flow) -> Option<Program> {
    use crate::flow::Screen;
    Some(match flow.screen {
        Screen::Start | Screen::Chunks => Program::Piece(PieceKind::Intro),
        Screen::Loading => return None,
        Screen::Playing | Screen::Paused => Program::Band,
        Screen::Transition => Program::Piece(PieceKind::Stinger),
        Screen::RunOver if flow.stats.won => Program::Piece(PieceKind::Intro),
        Screen::RunOver => Program::Piece(PieceKind::Ballad),
    })
}

/// What the fixed tick tells the music: where the sim's clock is.
#[derive(Resource, Default)]
pub struct MusicCues {
    tick: u64,
    clock: BeatClock,
    /// The sim went back in time (a new room): forget the schedule.
    restarted: bool,
    /// Tempo and meter the music wants the clock at.
    tempo: Option<(f32, u32)>,
    /// How enemy windups line up with the beat.
    beat_snap: BeatSnap,
    /// How tense the moment is, followed tick by tick from the sim.
    pub mood: Mood,
    mood_params: MoodParams,
    /// The player's moves since the music last looked: tick, move name and
    /// strength.
    moves: Vec<(u64, &'static str, Strength)>,
}

/// How strong a move was, as the sim reports it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Strength {
    Full,
    /// 0..1, such as a launch's charge.
    Power(f32),
    /// Metres per second, such as a slam's.
    Speed(f32),
}

/// The move a sim event is, as the kits in the style files name them.
pub fn move_of(event: &Event) -> Option<(&'static str, Strength)> {
    Some(match *event {
        Event::Jumped { .. } => ("jump", Strength::Full),
        Event::Launched { power, .. } => ("launch", Strength::Power(power)),
        Event::Lunged { .. } => ("lunge", Strength::Full),
        Event::Slammed { speed, .. } => ("slam", Strength::Speed(speed)),
        Event::Bounced { .. } => ("bounce", Strength::Full),
        Event::Hit {
            kind: HitKind::Pogo,
            ..
        } => ("pogo", Strength::Full),
        Event::Hit { .. } => ("hit", Strength::Full),
        Event::Killed { .. } => ("kill", Strength::Full),
        Event::Grabbed { .. } => ("grab", Strength::Full),
        Event::Thrown { speed, .. } => ("throw", Strength::Speed(speed)),
        Event::Dodged { .. } => ("dodge", Strength::Full),
        _ => return None,
    })
}

/// The name a state goes by in `music.ron`'s `mood.states`.
pub fn state_name(state: State) -> &'static str {
    match state {
        State::Unaware => "unaware",
        State::Surprised => "surprised",
        State::Searching => "searching",
        State::Chasing => "chasing",
        State::Defending => "defending",
        State::Attacking(AttackPhase::Windup) => "windup",
        State::Attacking(AttackPhase::Strike) => "strike",
        State::Attacking(AttackPhase::Recover) => "recover",
        State::Dizzy => "dizzy",
        State::Prone => "prone",
        State::Fleeing => "fleeing",
        State::Dodging => "dodging",
    }
}

/// What the music sees of the sim: living enemies on screen, health, combo.
pub fn scene(sim: &Sim) -> Scene {
    let player = sim.blob.center();
    let on_screen = |p: Vec2| {
        let d = (p - player).abs();
        d.x <= sim.view.x && d.y <= sim.view.y
    };
    let near: Vec<State> = sim
        .enemies
        .iter()
        .filter(|e| e.alive() && on_screen(e.position))
        .map(|e| e.state)
        .collect();
    Scene {
        windups: near
            .iter()
            .filter(|s| **s == State::Attacking(AttackPhase::Windup))
            .count() as u32,
        enemies: near.into_iter().map(state_name).collect(),
        health: sim.health as f32 / sim.max_health.max(1) as f32,
        combo: sim.combo.count,
        dead: sim.dead,
    }
}

impl MusicCues {
    /// Call before each sim tick: sets the clock to the music's tempo.
    pub fn conduct(&self, sim: &mut Sim) {
        sim.beat_snap = self.beat_snap;
        if let Some((bpm, beats_per_bar)) = self.tempo {
            if sim.clock.bpm() != bpm {
                sim.clock.set_tempo(sim.tick, bpm);
            }
            if sim.clock.beats_per_bar() != beats_per_bar {
                sim.clock.set_beats_per_bar(beats_per_bar);
            }
        }
    }

    /// Call after each sim tick, with the tick's events.
    pub fn observe(&mut self, sim: &Sim, events: &[Event]) {
        for e in events {
            if let Some((name, strength)) = move_of(e) {
                self.moves.push((sim.tick, name, strength));
            }
        }
        if sim.tick < self.tick {
            self.restarted = true;
        }
        self.tick = sim.tick;
        self.clock = sim.clock;
        self.mood.update(&scene(sim), DT, &self.mood_params);
    }
}

/// State shared between the game and the audio thread.
#[derive(Default)]
struct Shared {
    /// Notes waiting to be picked up by the stream.
    inbox: Mutex<Vec<Scheduled>>,
    /// Samples the stream has produced: how far the device has taken it.
    taken: AtomicU64,
    /// Sizes of the bursts the device took samples in, since last read.
    bursts: Mutex<Vec<u64>>,
    /// Settings the stream reads: gain, voice cap, burst gap.
    settings: Mutex<StreamSettings>,
}

#[derive(Clone, Copy)]
struct StreamSettings {
    volume: f32,
    max_voices: usize,
    burst_gap: Duration,
}

impl Default for StreamSettings {
    fn default() -> Self {
        Self::from(&StreamParams::default())
    }
}

impl From<&StreamParams> for StreamSettings {
    fn from(p: &StreamParams) -> Self {
        Self {
            volume: p.volume,
            max_voices: p.max_voices,
            burst_gap: Duration::from_secs_f32(p.burst_gap.max(0.0)),
        }
    }
}

/// The never-ending music source.
#[derive(Asset, TypePath, Clone)]
pub struct MusicStream {
    shared: Arc<Shared>,
    rate: u32,
}

impl Decodable for MusicStream {
    type Decoder = MusicDecoder;

    fn decoder(&self) -> MusicDecoder {
        let settings = *self.shared.settings.lock().expect("music settings");
        MusicDecoder {
            shared: self.shared.clone(),
            rate: self.rate,
            mixer: Mixer::new(settings.max_voices),
            settings,
            last_call: None,
            burst: 0,
        }
    }
}

pub struct MusicDecoder {
    shared: Arc<Shared>,
    rate: u32,
    mixer: Mixer,
    settings: StreamSettings,
    last_call: Option<Instant>,
    burst: u64,
}

impl MusicDecoder {
    /// Measures the device's bursts: a gap between two requests ends one.
    fn time_request(&mut self) {
        let now = Instant::now();
        let gap = self
            .last_call
            .is_none_or(|last| now.duration_since(last) > self.settings.burst_gap);
        if gap && self.burst > 0 {
            if let Ok(mut bursts) = self.shared.bursts.try_lock() {
                bursts.push(self.burst);
            }
            self.burst = 0;
        }
        self.burst += 1;
        self.last_call = Some(now);
    }

    /// Picks up new notes and settings without ever blocking the device.
    fn check_inbox(&mut self) {
        if let Ok(mut inbox) = self.shared.inbox.try_lock() {
            for note in inbox.drain(..) {
                self.mixer.schedule(note);
            }
        }
        if let Ok(settings) = self.shared.settings.try_lock() {
            self.settings = *settings;
            self.mixer.set_max_voices(settings.max_voices);
        }
    }
}

impl Iterator for MusicDecoder {
    type Item = bevy::audio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        self.time_request();
        if self.mixer.cursor().is_multiple_of(INBOX_EVERY) {
            self.check_inbox();
        }
        let s = soft_limit(self.mixer.next_sample() * self.settings.volume);
        self.shared
            .taken
            .store(self.mixer.cursor(), Ordering::Relaxed);
        Some(s as bevy::audio::Sample)
    }
}

impl Source for MusicDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        NonZero::<u16>::MIN
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(self.rate).expect("non-zero rate")
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[derive(Resource)]
struct Music {
    path: PathBuf,
    modified: Option<SystemTime>,
    since_check: f32,
    file: MusicFile,
    shared: Arc<Shared>,
    sync: AudioSync,
    /// Next beat of the click track to schedule.
    next_beat: Option<i64>,
    downbeat: Arc<[f32]>,
    beat: Arc<[f32]>,
    band: Option<Band>,
    /// The band's level, by band bar.
    band_fade: Fade,
    /// Screen pieces playing or fading out.
    pieces: Vec<PiecePlayer>,
    /// What the current screen plays, once one has asked.
    program: Option<Program>,
    /// Music time, which runs on through frozen screens.
    timeline: Timeline,
    /// The style the stage asks for, once one has.
    stage_style: Option<String>,
    meter: LatencyMeter,
    /// Latency measured over the last log window, in seconds.
    latency: f32,
    since_log: f32,
    muted: bool,
}

fn music_file() -> PathBuf {
    std::env::var_os("TARBORN_MUSIC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/music.ron")))
}

/// The director, the player's drummer and what voices their notes.
struct Band {
    director: Director,
    /// The style's library, for the screen pieces.
    library: Vec<LibraryTune>,
    drummer: Drummer,
    voicer: Voicer,
    /// Next bar to decide.
    next_bar: Option<i64>,
    /// Recently decided bars by clock bar, for the harmony under a hit.
    bars: VecDeque<(i64, Bar)>,
}

/// Where the music is at `tick`, from the bars the director decided.
fn moment<'a>(
    director: &Director,
    bars: &'a VecDeque<(i64, Bar)>,
    clock: &BeatClock,
    tick: f64,
) -> Moment<'a> {
    let bpb = clock.beats_per_bar().max(1) as f64;
    let beats = clock.beats_at(tick);
    let bar = (beats / bpb).floor() as i64;
    let in_bar = (beats - bar as f64 * bpb) as f32;
    let from_bar_line = if in_bar > bpb as f32 / 2.0 {
        in_bar - bpb as f32
    } else {
        in_bar
    };
    let decided = bars.iter().find(|(b, _)| *b == bar).map(|(_, b)| b);
    Moment {
        key: decided.map_or(director.key(), |b| b.key),
        chord: decided.and_then(|b| b.chord_at(in_bar)).map(|c| &c.chord),
        from_bar_line,
    }
}

/// Reads a band data file from `assets/music/`, or the built-in copy.
fn music_data<T: serde::de::DeserializeOwned>(name: &str) -> Result<T, String> {
    let dir = music_file().with_file_name("music");
    let path = dir.join(name);
    if let Ok(text) = std::fs::read_to_string(&path) {
        match from_ron(&text) {
            Ok(value) => return Ok(value),
            Err(e) => warn!("{}: {e}; using the built-in copy", path.display()),
        }
    }
    let (_, text) = BUILT_IN_DATA
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| format!("no music data named {name}"))?;
    from_ron(text).map_err(|e| format!("built-in {name}: {e}"))
}

fn run_seed() -> u32 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(1, |d| d.subsec_nanos() ^ d.as_secs() as u32)
}

impl Band {
    fn new(file: &MusicFile, name: &str) -> Result<Self, String> {
        let style: Style = music_data(&format!("styles/{name}.ron"))?;
        let library: Vec<LibraryTune> = music_data(&format!("library/{name}.ron"))?;
        let bank: PatchBank = music_data("patches.ron")?;
        let seed = run_seed();
        let director = Director::new(style, library.clone(), file.director.clone(), seed)
            .ok_or_else(|| format!("the {name} music library is empty"))?;
        info!(
            "music: {name} at {:.0} BPM, run seed {seed}",
            director.bpm()
        );
        Ok(Self {
            drummer: Drummer::new(director.style().kit.clone()),
            director,
            library,
            voicer: Voicer::new(bank, file.render.clone(), seed),
            next_bar: None,
            bars: VecDeque::new(),
        })
    }
}

fn modified(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Music {
    fn install(&mut self, file: MusicFile, cues: &mut MusicCues) {
        let rate = self.file.render.sample_rate;
        self.downbeat = file.click.downbeat.render(rate).into();
        self.beat = file.click.beat.render(rate).into();
        if let Ok(mut settings) = self.shared.settings.lock() {
            *settings = StreamSettings::from(&file.stream);
        }
        // A new director only when its own settings change, so a tweak to
        // the mix doesn't restart the tune.
        let mut render = file.render.clone();
        render.sample_rate = rate;
        match &mut self.band {
            Some(band) if file.director == self.file.director => band.voicer.set_settings(render),
            _ => {
                let mut file = file.clone();
                file.render = render;
                let style = self
                    .stage_style
                    .clone()
                    .unwrap_or(file.director.style.clone());
                self.band = Band::new(&file, &style)
                    .inspect_err(|e| warn!("music: {e}; playing no band"))
                    .ok();
            }
        }
        cues.mood_params = file.mood.clone();
        cues.beat_snap = BeatSnap {
            enabled: file.beat_snap.enabled,
            max_shift: file.beat_snap.max_shift,
            grid: file.beat_snap.grid,
        };
        self.file = file;
        self.set_tempo(cues);
    }

    /// Hands over to `to` on the next bar line not yet scheduled: what plays
    /// fades out over `fade_bars` while `to` fades in.
    fn segue(&mut self, to: Program, clock: &BeatClock) {
        let p = self.file.screens.clone();
        let (now, offset) = (self.timeline.now(), self.timeline.offset());
        let bpb = clock.beats_per_bar().max(1) as i64;
        let band_bar =
            |music_tick: f64| (clock.beats_at(music_tick - offset) / bpb as f64).ceil() as i64;
        let first = self.program.is_none();
        self.program = Some(to);
        let mut at = now;
        if let Some(bar) = self.band.as_ref().and_then(|b| b.next_bar)
            && !self.band_fade.silent_from(bar)
        {
            at = at.max(clock.tick_of_beat((bar * bpb) as f64) + offset);
        }
        for piece in self.pieces.iter().filter(|p| !p.done()) {
            at = at.max(piece.bar_tick(piece.next_bar()));
        }
        for piece in &mut self.pieces {
            piece.fade_out(p.fade_bars);
        }
        let Some(band) = &mut self.band else {
            return;
        };
        let bar = band.next_bar.unwrap_or_else(|| band_bar(now));
        self.band_fade = self.band_fade.towards(bar, p.fade_bars, 0.0);
        match to {
            Program::Band => {
                let bar = band_bar(at);
                band.next_bar = Some(bar);
                self.band_fade = if first {
                    Fade::steady(1.0)
                } else {
                    Fade::steady(0.0).towards(bar, p.band_fade_in_bars, 1.0)
                };
            }
            Program::Piece(kind) => {
                match piece(band.director.style(), &band.library, kind, &p, run_seed()) {
                    Some(piece) => self
                        .pieces
                        .push(PiecePlayer::new(piece, at, TICKS_PER_SECOND)),
                    None => warn!(
                        "music: no {kind:?} in the {} library",
                        band.director.style().name
                    ),
                }
            }
        }
    }

    fn set_tempo(&self, cues: &mut MusicCues) {
        cues.tempo = Some(match &self.band {
            Some(band) => (band.director.bpm(), band.director.beats_per_bar()),
            None => (self.file.click.bpm, self.file.click.beats_per_bar),
        });
    }
}

impl Music {
    /// The music for `file`, and what it shares with its stream.
    fn new(path: PathBuf, file: MusicFile, cues: &mut MusicCues) -> (Self, Arc<Shared>) {
        // The sample rate is fixed for the life of the stream; edits to it apply
        // on the next launch.
        let rate = file.render.sample_rate;
        let shared = Arc::new(Shared::default());
        let mut music = Music {
            modified: modified(&path),
            path,
            since_check: 0.0,
            file: MusicFile::default(),
            shared: shared.clone(),
            sync: AudioSync::new(rate, TICKS_PER_SECOND),
            next_beat: None,
            downbeat: Arc::from(Vec::new()),
            beat: Arc::from(Vec::new()),
            band: None,
            band_fade: Fade::steady(0.0),
            pieces: Vec::new(),
            program: None,
            timeline: Timeline::default(),
            stage_style: None,
            meter: LatencyMeter::default(),
            latency: 0.0,
            since_log: 0.0,
            muted: false,
        };
        music.file.render.sample_rate = rate;
        music.install(file, cues);
        (music, shared)
    }
}

fn start_music(
    mut commands: Commands,
    mut cues: ResMut<MusicCues>,
    mut streams: ResMut<Assets<MusicStream>>,
) {
    let path = music_file();
    let from_disk = std::fs::read_to_string(&path)
        .ok()
        .map(|text| from_ron::<MusicFile>(&text));
    let file = match from_disk {
        Some(Ok(file)) => {
            info!("music loaded from {}", path.display());
            file
        }
        other => {
            if let Some(Err(e)) = other {
                warn!("{}: {e}; using the built-in music settings", path.display());
            }
            from_ron(BUILT_IN).expect("the built-in music file parses")
        }
    };
    let (music, shared) = Music::new(path, file, &mut cues);
    let rate = music.file.render.sample_rate;
    let handle = streams.add(MusicStream { shared, rate });
    commands.spawn((AudioPlayer(handle), PlaybackSettings::ONCE));
    commands.insert_resource(music);
}

fn reload_music(time: Res<Time>, mut music: ResMut<Music>, mut cues: ResMut<MusicCues>) {
    music.since_check += time.delta_secs();
    if music.since_check < music.file.stream.reload_interval {
        return;
    }
    music.since_check = 0.0;
    let stamp = modified(&music.path);
    if stamp.is_none() || stamp == music.modified {
        return;
    }
    music.modified = stamp;
    let Ok(text) = std::fs::read_to_string(&music.path) else {
        return;
    };
    match from_ron::<MusicFile>(&text) {
        Ok(file) => {
            let rate = music.file.render.sample_rate;
            if file.render.sample_rate != rate {
                warn!("music sample rate changes apply on the next launch");
            }
            music.install(file, &mut cues);
            music.file.render.sample_rate = rate;
            info!("music reloaded from {}", music.path.display());
        }
        Err(e) => warn!("{}: {e}; keeping the previous music", music.path.display()),
    }
}

/// The music style a dressing set plays: its name in lower case ("Forest"
/// plays `styles/forest.ron`).
pub fn style_for_set(set: &str) -> String {
    set.trim().to_lowercase()
}

/// Each stage's dressing set names its music style: a new set, a new band.
fn follow_stage(
    dressing: Option<Res<crate::visuals::Dressing>>,
    mut music: ResMut<Music>,
    mut cues: ResMut<MusicCues>,
) {
    let Some(dressing) = dressing else {
        return;
    };
    let name = &style_for_set(&dressing.set().name);
    if music.stage_style.as_ref() == Some(name) {
        return;
    }
    music.stage_style = Some(name.clone());
    if music
        .band
        .as_ref()
        .is_some_and(|b| &b.director.style().name == name)
    {
        return;
    }
    match Band::new(&music.file, name) {
        Ok(band) => {
            music.band = Some(band);
            music.set_tempo(&mut cues);
            // Notes already queued belong to the old style.
            if let Ok(mut inbox) = music.shared.inbox.lock() {
                inbox.clear();
            }
        }
        Err(e) => warn!("music: no {name} style ({e}); keeping the current band"),
    }
}

/// M mutes and unmutes (the sound effects listen to it too).
fn toggle_mute(keys: Res<ButtonInput<KeyCode>>, mut music: ResMut<Music>) {
    if keys.just_pressed(KeyCode::KeyM) {
        music.muted = !music.muted;
    }
}

fn schedule_music(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    flow: Option<Res<crate::screens::RunFlow>>,
    mut music: ResMut<Music>,
    mut cues: ResMut<MusicCues>,
) {
    let music = &mut *music;
    let p = music.file.stream.clone();
    let rate = music.file.render.sample_rate;

    // Measure the device.
    if let Ok(mut bursts) = music.shared.bursts.lock() {
        for b in bursts.drain(..) {
            music.meter.burst(b);
        }
    }
    music.since_log += time.delta_secs();
    if music.since_log >= p.log_interval && music.meter.bursts() > 0 {
        music.since_log = 0.0;
        music.latency = music.meter.latency(rate, &p);
        info!("{}", music.meter.summary(rate, &p));
        music.meter = LatencyMeter::default();
    }

    let taken = music.shared.taken.load(Ordering::Relaxed);
    let notes = music.schedule(
        &mut cues,
        flow.as_ref().and_then(|f| program_for(&f.0)),
        time.delta_secs_f64(),
        fixed.overstep_fraction() as f64,
        taken,
        time.delta_secs(),
    );
    if !notes.is_empty()
        && let Ok(mut inbox) = music.shared.inbox.lock()
    {
        inbox.extend(notes);
    }
}

impl Music {
    /// One frame of music: follows the sim (or keeps time while it is
    /// frozen), hands over to the program the screen `wants`, and returns
    /// what is due in the next `lookahead` seconds. `taken` is how many
    /// samples the device has taken.
    fn schedule(
        &mut self,
        cues: &mut MusicCues,
        wants: Option<Program>,
        delta: f64,
        overstep: f64,
        taken: u64,
        delta_secs: f32,
    ) -> Vec<Scheduled> {
        let music = self;
        let p = music.file.stream.clone();
        // Follow the sim, or keep time on its own while it is frozen.
        let tps = TICKS_PER_SECOND as f64;
        let rebased = music.timeline.advance(
            cues.tick,
            overstep,
            delta * tps,
            p.frozen_after as f64 * tps,
        ) | std::mem::take(&mut cues.restarted);
        let now = music.timeline.now();
        let offset = music.timeline.offset();
        let clock = cues.clock;
        let bpb = clock.beats_per_bar().max(1) as i64;
        // The first band bar line after music time `t`.
        let band_bar = |t: f64| (clock.beats_at(t - offset) / bpb as f64).floor() as i64 + 1;
        if rebased {
            // Bars placed in sim time are placed again from the next bar line,
            // and a fade under way starts over there.
            music.next_beat = None;
            if let Some(band) = &mut music.band {
                band.next_bar = None;
                band.bars.clear();
            }
            let f = music.band_fade;
            if f.from != f.to {
                music.band_fade = Fade {
                    start: band_bar(now),
                    ..f
                };
            }
        }
        music.sync.observe(now, taken, &p);

        // A new screen, a new program.
        if let Some(to) = wants
            && music.program != Some(to)
        {
            music.segue(to, &clock);
        }
        if music.program.is_none() {
            // No screens (a test app): just the band.
            music.segue(Program::Band, &clock);
        }
        let compensation = p.latency_compensation
            + if p.auto_compensate {
                music.latency
            } else {
                0.0
            };
        let horizon = now + p.lookahead as f64 * TICKS_PER_SECOND as f64;
        let mut notes = Vec::new();

        // The band, a bar at a time, and the player's own drumming.
        let moves = std::mem::take(&mut cues.moves);
        if let Some(band) = &mut music.band {
            band.drummer.advance(delta_secs);
            for (tick, name, strength) in moves {
                let strength = match strength {
                    Strength::Full => 1.0,
                    Strength::Power(p) => p,
                    Strength::Speed(v) => v / band.drummer.kit().full_speed.max(1e-3),
                };
                let moment = moment(&band.director, &band.bars, &clock, tick as f64);
                let (strike, answers) = band.drummer.hear(name, strength, moment);
                for cue in answers {
                    match cue {
                        Cue::Fill => band.director.request_fill(),
                        Cue::NextSection => band.director.request_next_section(),
                    }
                }
                if music.muted {
                    continue;
                }
                let Some(strike) = strike else {
                    continue;
                };
                let Some((samples, gain)) =
                    band.voicer
                        .strike(&strike.patch, strike.pitch, strike.velocity)
                else {
                    continue;
                };
                if let Some(at) = music
                    .sync
                    .sample_for_tick(tick as f64 + offset, compensation)
                {
                    notes.push(Scheduled { at, samples, gain });
                }
            }
            let duck = band.drummer.duck();
            let fade = &mut music.band_fade;
            if music.muted {
                band.next_bar = None;
            } else {
                // Start (or resume) on the next bar line, never in the past.
                let first_due = band_bar(now);
                let mut bar = band.next_bar.map_or(first_due, |b| b.max(first_due));
                while clock.tick_of_beat((bar * bpb) as f64) + offset <= horizon
                    && music.sync.sample_for_tick(now, compensation).is_some()
                {
                    if fade.silent_from(bar) {
                        break;
                    }
                    if bar >= fade.start.saturating_add(fade.bars as i64) {
                        *fade = Fade::steady(fade.to);
                    }
                    let level = fade.gain(bar);
                    let plan = band.director.next_bar(Input {
                        intensity: cues.mood.intensity,
                        duck,
                    });
                    let (bpm, swing) = (band.director.bpm(), band.director.swing());
                    for note in &plan.notes {
                        let Some(voice) = band.voicer.voice(note, (bar * bpb) as f64, bpm, swing)
                        else {
                            continue;
                        };
                        let tick = clock.tick_of_beat(voice.beat) + offset;
                        if let Some(at) = music.sync.sample_for_tick(tick, compensation) {
                            notes.push(Scheduled {
                                at,
                                samples: voice.samples,
                                gain: voice.gain * level,
                            });
                        }
                    }
                    band.bars.push_back((bar, plan));
                    if band.bars.len() > 8 {
                        band.bars.pop_front();
                    }
                    bar += 1;
                }
                band.next_bar = Some(bar);
            }

            // The screen pieces, voiced by the band's instruments.
            for player in &mut music.pieces {
                let (bpm, swing) = (player.piece.bpm, player.piece.swing);
                let pbpb = player.piece.beats_per_bar.max(1) as i64;
                let bars: Vec<(i64, f32, Vec<tarborn_music::Note>)> = player
                    .due(horizon)
                    .into_iter()
                    .map(|(bar, gain, notes)| (bar, gain, notes.to_vec()))
                    .collect();
                if music.muted {
                    continue;
                }
                for (bar, level, bar_notes) in bars {
                    for note in &bar_notes {
                        let Some(voice) = band.voicer.voice(note, (bar * pbpb) as f64, bpm, swing)
                        else {
                            continue;
                        };
                        let tick = player.tick_of_beat(voice.beat);
                        if let Some(at) = music.sync.sample_for_tick(tick, compensation) {
                            notes.push(Scheduled {
                                at,
                                samples: voice.samples,
                                gain: voice.gain * level,
                            });
                        }
                    }
                }
            }
            music.pieces.retain(|p| !p.done());
        }

        // The click track.
        if !music.file.click.enabled || music.muted {
            music.next_beat = None;
            return notes;
        }
        // Start (or resume after a pause) on the next beat, never in the past.
        let first_due = clock.beats_at(now - offset).floor() as i64 + 1;
        let mut beat = music.next_beat.map_or(first_due, |b| b.max(first_due));
        while clock.tick_of_beat(beat as f64) + offset <= horizon {
            let tick = clock.tick_of_beat(beat as f64) + offset;
            let Some(at) = music.sync.sample_for_tick(tick, compensation) else {
                break;
            };
            let downbeat = beat.rem_euclid(clock.beats_per_bar() as i64) == 0;
            notes.push(Scheduled {
                at,
                samples: if downbeat {
                    music.downbeat.clone()
                } else {
                    music.beat.clone()
                },
                gain: music.file.click.volume,
            });
            beat += 1;
        }
        music.next_beat = Some(beat);
        notes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_music_file_parses() {
        let file: MusicFile = from_ron(BUILT_IN).expect("music.ron parses");
        assert!(file.click.bpm > 0.0);
        assert!(file.stream.lookahead > 0.0);
        assert!(!file.click.downbeat.layers.is_empty());
        assert!(file.beat_snap.enabled && file.beat_snap.max_shift > 0.0);
    }

    /// A room with one guard near the player and one far off screen.
    fn room() -> Sim {
        let mut sim = Sim::new(tarborn_sim::Level::test_room(), Vec2::ZERO);
        sim.enemies = vec![
            tarborn_sim::Enemy::guard(Vec2::new(3.0, 0.6)),
            tarborn_sim::Enemy::guard(Vec2::new(200.0, 0.6)),
        ];
        sim
    }

    fn settle(sim: &Sim, seconds: f32) -> Mood {
        let p: MusicFile = from_ron(BUILT_IN).unwrap();
        let mut mood = Mood::default();
        for _ in 0..(seconds / DT) as usize {
            mood.update(&scene(sim), DT, &p.mood);
        }
        mood
    }

    fn layer(intensity: f32) -> usize {
        let p: MusicFile = from_ron(BUILT_IN).unwrap();
        p.director
            .layer_at
            .iter()
            .filter(|&&at| intensity >= at)
            .count()
    }

    #[test]
    fn a_calm_room_is_calm() {
        let sim = room();
        assert_eq!(scene(&sim).enemies, vec!["unaware"]);
        let mood = settle(&sim, 5.0);
        assert_eq!(layer(mood.intensity), 0, "{mood:?}");
        assert_eq!(mood.danger, 0.0);
    }

    #[test]
    fn being_spotted_brings_the_band_in() {
        let mut sim = room();
        sim.enemies[0].state = State::Defending;
        let mood = settle(&sim, 2.0);
        assert_eq!(layer(mood.intensity), 1, "{mood:?}");
    }

    #[test]
    fn a_fight_is_full_band_and_lingers() {
        let mut sim = room();
        sim.enemies[0].state = State::Attacking(AttackPhase::Strike);
        let mood = settle(&sim, 1.0);
        assert_eq!(layer(mood.intensity), 2, "{mood:?}");
        // A combo keeps it up after the enemy goes down.
        sim.enemies[0].state = State::Prone;
        sim.combo.count = 3;
        assert_eq!(layer(settle(&sim, 3.0).intensity), 2);
        // The far guard doesn't count, even attacking.
        let mut far = room();
        far.enemies[1].state = State::Attacking(AttackPhase::Strike);
        assert_eq!(layer(settle(&far, 2.0).intensity), 0);
    }

    #[test]
    fn low_health_and_windups_are_danger() {
        let mut sim = room();
        sim.health = 1;
        let hurt = settle(&sim, 2.0);
        assert!(hurt.danger > 0.3, "{hurt:?}");
        sim.enemies[0].state = State::Attacking(AttackPhase::Windup);
        let windup = settle(&sim, 2.0);
        assert!(windup.danger > hurt.danger, "{windup:?}");
        assert_eq!(scene(&sim).windups, 1);
        sim.dead = true;
        assert_eq!(settle(&sim, 1.0).intensity, 0.0);
    }

    #[test]
    fn the_mood_rises_fast_and_falls_slowly() {
        let p: MusicFile = from_ron(BUILT_IN).unwrap();
        let mut sim = room();
        sim.enemies[0].state = State::Attacking(AttackPhase::Strike);
        let mut mood = Mood::default();
        for _ in 0..(1.0 / DT) as usize {
            mood.update(&scene(&sim), DT, &p.mood);
        }
        let peak = mood.intensity;
        sim.enemies[0].state = State::Unaware;
        for _ in 0..(1.0 / DT) as usize {
            mood.update(&scene(&sim), DT, &p.mood);
        }
        assert!(mood.intensity > peak * 0.6, "{peak} -> {}", mood.intensity);
    }

    #[test]
    fn every_state_has_a_name_the_mood_knows() {
        let p: MusicFile = from_ron(BUILT_IN).unwrap();
        for phase in [
            AttackPhase::Windup,
            AttackPhase::Strike,
            AttackPhase::Recover,
        ] {
            assert!(
                p.mood
                    .states
                    .contains_key(state_name(State::Attacking(phase)))
            );
        }
        for s in [
            State::Surprised,
            State::Searching,
            State::Chasing,
            State::Defending,
            State::Dizzy,
            State::Prone,
            State::Fleeing,
            State::Dodging,
        ] {
            assert!(p.mood.states.contains_key(state_name(s)), "{s:?}");
        }
    }

    /// Replays a bot tape in the training chunk and feeds every event to
    /// the built-in style's drummer, as the game does.
    fn drum(style: &str, tape: &tarborn_pcg::Tape, ticks: u64) -> Vec<(&'static str, String)> {
        use tarborn_pcg::{BeatParams, Chunk, ChunkParams, chunk_dir, play};
        let chunk = Chunk::load(
            &chunk_dir("rooms").join("training.ron"),
            &ChunkParams::default(),
        )
        .expect("the training chunk loads");
        let mut sim = chunk.sim();
        let run = play(&mut sim, tape, ticks, &BeatParams::default());
        let style: Style = music_data(&format!("styles/{style}.ron")).unwrap();
        let mut drummer = Drummer::new(style.kit.clone());
        let (key, _) = tarborn_music::theory::Pc::parse("C").unwrap();
        let mut hits = Vec::new();
        for (_, event) in &run.events {
            let Some((name, _)) = move_of(event) else {
                continue;
            };
            let moment = Moment {
                key,
                chord: None,
                from_bar_line: 2.0,
            };
            if let (Some(strike), _) = drummer.hear(name, 1.0, moment) {
                hits.push((name, strike.patch));
            }
            drummer.advance(DT);
        }
        hits
    }

    #[test]
    fn bot_tapes_drum_on_the_right_moves() {
        use tarborn_pcg::{Buttons, Tape};
        let right = Buttons::dir(Vec2::X);
        // Lunge at the grub and keep pressing Jump until it dies.
        let fight = Tape::new()
            .at(0, right.with_jump())
            .at(8, right)
            .at(13, Buttons::NONE)
            .at(45, Buttons::NONE.with_jump())
            .at(53, Buttons::NONE)
            .at(65, Buttons::NONE.with_jump())
            .at(73, Buttons::NONE);
        for (style, kick, snare, crash) in [
            ("industrial", "ind_kick", "ind_snare", "ind_crash"),
            ("forest", "for_kick", "for_rim", "for_crash"),
        ] {
            let hits = drum(style, &fight, 300);
            let names: Vec<&str> = hits.iter().map(|(n, _)| *n).collect();
            for (name, patch) in &hits {
                let expected = match *name {
                    "jump" | "launch" | "slam" => kick,
                    "lunge" | "hit" | "throw" => snare,
                    "kill" => crash,
                    _ => continue,
                };
                assert_eq!(patch, expected, "{style}: {name}");
            }
            for needed in ["lunge", "hit", "kill"] {
                assert!(names.contains(&needed), "{style}: no {needed} in {names:?}");
            }
        }
        // A slam lands on the kick.
        let slam = Tape::new()
            .at(60, Buttons::NONE.with_jump())
            .at(70, Buttons::NONE)
            .at(110, Buttons::dir(Vec2::NEG_Y).with_jump())
            .at(120, Buttons::NONE);
        let hits = drum("industrial", &slam, 300);
        assert!(hits.contains(&("slam", "ind_kick".into())), "{hits:?}");
    }

    #[test]
    fn a_drumming_player_ducks_the_band() {
        let file: MusicFile = from_ron(BUILT_IN).unwrap();
        let mut band = Band::new(&file, "industrial").unwrap();
        let mut quiet = Band::new(&file, "industrial").unwrap();
        let (key, _) = tarborn_music::theory::Pc::parse("C").unwrap();
        for _ in 0..8 {
            let at = Moment {
                key,
                chord: None,
                from_bar_line: 2.0,
            };
            band.drummer.hear("hit", 1.0, at);
            band.drummer.advance(0.1);
        }
        let duck = band.drummer.duck();
        assert!(duck > 0.9, "{duck}");
        let ride = |b: &Bar| -> f32 {
            b.notes
                .iter()
                .filter(|n| n.patch == "ride")
                .map(|n| n.velocity)
                .sum()
        };
        // Same seed for both, so only the ducking differs.
        let mut loud = 0.0;
        let mut ducked = 0.0;
        let mut a = Director::new(
            band.director.style().clone(),
            music_data("library/industrial.ron").unwrap(),
            file.director.clone(),
            3,
        )
        .unwrap();
        let mut b = Director::new(
            quiet.director.style().clone(),
            music_data("library/industrial.ron").unwrap(),
            file.director.clone(),
            3,
        )
        .unwrap();
        for _ in 0..8 {
            loud += ride(&a.next_bar(Input {
                intensity: 1.0,
                duck: 0.0,
            }));
            ducked += ride(&b.next_bar(Input {
                intensity: 1.0,
                duck,
            }));
        }
        quiet.drummer.advance(1.0);
        assert_eq!(quiet.drummer.duck(), 0.0);
        let measured = 1.0 - ducked / loud;
        info!("band drummer ducked by {:.0}%", measured * 100.0);
        assert!(
            measured > 0.5 * file.director.duck_depth,
            "ducked by {measured}"
        );
    }

    #[test]
    fn every_dressing_set_has_a_music_style() {
        let file: MusicFile = from_ron(BUILT_IN).unwrap();
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/dressing");
        let mut sets = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "ron") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let name = text
                .lines()
                .find_map(|l| l.trim().strip_prefix("name:"))
                .and_then(|v| v.split('"').nth(1))
                .unwrap_or_else(|| panic!("{} has no name", path.display()));
            let style = style_for_set(name);
            let band = Band::new(&file, &style).unwrap_or_else(|e| panic!("set {name}: {e}"));
            assert_eq!(band.director.style().name, style);
            sets += 1;
        }
        assert!(sets >= 2, "found {sets} dressing sets");
        assert_eq!(style_for_set("Industrial"), "industrial");
    }

    #[test]
    fn the_built_in_band_starts() {
        let file: MusicFile = from_ron(BUILT_IN).expect("music.ron parses");
        let band = Band::new(&file, &file.director.style).expect("the band starts");
        assert!(band.director.bpm() > 0.0);
        for (name, _) in BUILT_IN_DATA {
            if name.starts_with("styles/") {
                let style = name.trim_start_matches("styles/").trim_end_matches(".ron");
                Band::new(&file, style).unwrap_or_else(|e| panic!("{style}: {e}"));
            }
        }
    }

    /// The music heard frame by frame (60 fps) through a run's screens,
    /// without a device: the device takes samples in step with wall time,
    /// and the sim ticks only while playing.
    struct Tour {
        music: Music,
        cues: MusicCues,
        wall: f64,
    }

    impl Tour {
        fn new() -> Self {
            let file: MusicFile = from_ron(BUILT_IN).expect("music.ron parses");
            let mut cues = MusicCues::default();
            let (music, _) = Music::new(music_file(), file, &mut cues);
            let (bpm, bpb) = cues.tempo.expect("a tempo");
            cues.clock = BeatClock::new(bpm, bpb);
            Self {
                music,
                cues,
                wall: 0.0,
            }
        }

        /// `seconds` on a screen that `wants` a program: the scheduled
        /// notes' gains.
        fn screen(&mut self, wants: Option<Program>, sim_runs: bool, seconds: f32) -> Vec<f32> {
            let frame = 1.0 / 60.0;
            let rate = self.music.file.render.sample_rate as f64;
            let mut gains = Vec::new();
            for _ in 0..(seconds * 60.0).round() as usize {
                self.wall += frame;
                if sim_runs {
                    self.cues.tick += 2;
                }
                let taken = (self.wall * rate) as u64;
                let notes =
                    self.music
                        .schedule(&mut self.cues, wants, frame, 0.0, taken, frame as f32);
                gains.extend(notes.iter().map(|n| n.gain));
            }
            gains
        }
    }

    #[test]
    fn every_screen_plays_its_piece() {
        let mut t = Tour::new();
        let intro = Some(Program::Piece(PieceKind::Intro));
        // The start screen: the sim is frozen, the intro plays (and loops).
        assert!(!t.screen(intro, false, 20.0).is_empty(), "no intro");
        assert!(
            t.music
                .pieces
                .iter()
                .any(|p| p.piece.kind == PieceKind::Intro)
        );
        // Loading keeps it; play brings the band in.
        t.screen(None, false, 1.0);
        assert!(
            !t.screen(Some(Program::Band), true, 10.0).is_empty(),
            "no band"
        );
        assert!(t.music.pieces.is_empty(), "the intro never ended");
        // A pause freezes the sim but not the band.
        assert!(
            !t.screen(Some(Program::Band), false, 3.0).is_empty(),
            "silent pause"
        );
        assert!(!t.screen(Some(Program::Band), true, 3.0).is_empty());
        // The exit: a stinger.
        let stinger = Some(Program::Piece(PieceKind::Stinger));
        assert!(!t.screen(stinger, false, 4.0).is_empty(), "no stinger");
        t.screen(None, false, 1.0);
        t.cues.restarted = true;
        t.cues.tick = 0;
        assert!(!t.screen(Some(Program::Band), true, 10.0).is_empty());
        // Death: the ballad plays once, and then the music rests.
        let ballad = Some(Program::Piece(PieceKind::Ballad));
        assert!(!t.screen(ballad, false, 2.0).is_empty(), "no ballad");
        assert!(
            t.music
                .pieces
                .iter()
                .any(|p| p.piece.kind == PieceKind::Ballad)
        );
        t.screen(ballad, false, 30.0);
        assert!(
            t.screen(ballad, false, 5.0).is_empty(),
            "the ballad never ends"
        );
    }

    #[test]
    fn screens_hand_over_on_a_bar_line() {
        let mut t = Tour::new();
        let fade_bars = t.music.file.screens.fade_bars;
        t.screen(Some(Program::Band), true, 5.0);
        // Band to stinger: the stinger starts on the band's next bar line,
        // and the band fades out from that bar.
        let bar = t.music.band.as_ref().unwrap().next_bar.unwrap();
        let bpb = t.cues.clock.beats_per_bar() as i64;
        let line = t.cues.clock.tick_of_beat((bar * bpb) as f64) + t.music.timeline.offset();
        t.screen(Some(Program::Piece(PieceKind::Stinger)), false, 1.0 / 60.0);
        let stinger = t.music.pieces.last().expect("a stinger");
        assert!(
            (stinger.start - line).abs() < 1e-6,
            "{} vs {line}",
            stinger.start
        );
        let fade = t.music.band_fade;
        assert_eq!(fade.gain(bar - 1), 1.0);
        assert!(fade.gain(bar) < 1.0 && fade.gain(bar) > 0.0);
        assert!(fade.silent_from(bar + fade_bars as i64));
        // The band stops once faded.
        // The band stays out once faded, and the stinger plays only once.
        t.screen(Some(Program::Piece(PieceKind::Stinger)), false, 15.0);
        let after = t.screen(Some(Program::Piece(PieceKind::Stinger)), false, 5.0);
        assert!(after.is_empty(), "{} notes after the stinger", after.len());
        // Stinger to intro: the intro starts on one of the stinger's bar
        // lines, or right away once the stinger is over.
        t.screen(Some(Program::Piece(PieceKind::Intro)), false, 1.0 / 60.0);
        let intro = t.music.pieces.last().unwrap();
        assert_eq!(intro.piece.kind, PieceKind::Intro);
        assert!(intro.start >= t.music.timeline.now() - 1e-6);
        // Intro to band: the intro fades out from its next bar, and the
        // band comes in on a bar line no earlier than that.
        t.screen(Some(Program::Piece(PieceKind::Intro)), false, 8.0);
        let intro = t.music.pieces.last().unwrap();
        let from = intro.bar_tick(intro.next_bar());
        t.screen(Some(Program::Band), false, 1.0 / 60.0);
        let intro = t.music.pieces.last().unwrap();
        assert_eq!(intro.fade.to, 0.0);
        let band_bar = t.music.band.as_ref().unwrap().next_bar.unwrap();
        let band_line =
            t.cues.clock.tick_of_beat((band_bar * bpb) as f64) + t.music.timeline.offset();
        assert!(band_line >= from - 1e-6, "{band_line} < {from}");
        assert_eq!(t.music.band_fade.gain(band_bar - 1), 0.0);
        assert!(t.music.band_fade.gain(band_bar) > 0.0);
    }

    #[test]
    fn each_screen_asks_for_its_program() {
        use crate::flow::{Flow, RunParams, Screen};
        let mut flow = Flow::new(RunParams::default(), 1);
        let mut at = |screen, won| {
            flow.screen = screen;
            flow.stats.won = won;
            program_for(&flow)
        };
        let piece = |k| Some(Program::Piece(k));
        assert_eq!(at(Screen::Start, false), piece(PieceKind::Intro));
        assert_eq!(at(Screen::Chunks, false), piece(PieceKind::Intro));
        assert_eq!(at(Screen::Loading, false), None);
        assert_eq!(at(Screen::Playing, false), Some(Program::Band));
        assert_eq!(at(Screen::Paused, false), Some(Program::Band));
        assert_eq!(at(Screen::Transition, false), piece(PieceKind::Stinger));
        assert_eq!(at(Screen::RunOver, false), piece(PieceKind::Ballad));
        assert_eq!(at(Screen::RunOver, true), piece(PieceKind::Intro));
    }

    #[test]
    fn cues_notice_a_new_room() {
        let mut cues = MusicCues::default();
        let mut sim = Sim::training_room(Vec2::ZERO);
        for _ in 0..10 {
            sim.step(Default::default());
        }
        cues.observe(&sim, &[]);
        assert!(!cues.restarted);
        let fresh = Sim::training_room(Vec2::ZERO);
        cues.observe(&fresh, &[]);
        assert!(cues.restarted);
    }

    #[test]
    fn conduct_sets_the_sim_clock() {
        let cues = MusicCues {
            tempo: Some((200.0, 3)),
            ..MusicCues::default()
        };
        let mut sim = Sim::training_room(Vec2::ZERO);
        cues.conduct(&mut sim);
        assert_eq!(sim.clock.bpm(), 200.0);
        assert_eq!(sim.beat_snap, BeatSnap::default());
        assert_eq!(sim.clock.beats_per_bar(), 3);
    }
}

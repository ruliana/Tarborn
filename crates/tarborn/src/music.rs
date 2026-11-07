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
//! Every number lives in `assets/music.ron`, reloaded when it changes.

use bevy::audio::{AddAudioSource, ChannelCount, Decodable, SampleRate, Source};
use bevy::prelude::*;
use std::num::NonZero;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tarborn_music::director::{Director, Input, Voicer};
use tarborn_music::search::LibraryTune;
use tarborn_music::stream::{AudioSync, LatencyMeter, Mixer, Scheduled, StreamParams};
use tarborn_music::style::Style;
use tarborn_music::{MusicFile, PatchBank, from_ron};
use tarborn_sfx::soft_limit;
use tarborn_sim::{BeatClock, Sim, TICKS_PER_SECOND};

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
            .add_systems(Update, (reload_music, toggle_mute, schedule_music).chain());
    }
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
    /// How intense the moment is, 0 calm .. 1 full fight. Set by the game
    /// (step 9); calm until then.
    pub intensity: f32,
    /// How much the player is drumming, 0 .. 1, so the band ducks (step 10).
    pub duck: f32,
}

impl MusicCues {
    /// Call before each sim tick: sets the clock to the music's tempo.
    pub fn conduct(&self, sim: &mut Sim) {
        if let Some((bpm, beats_per_bar)) = self.tempo {
            if sim.clock.bpm() != bpm {
                sim.clock.set_tempo(sim.tick, bpm);
            }
            if sim.clock.beats_per_bar() != beats_per_bar {
                sim.clock.set_beats_per_bar(beats_per_bar);
            }
        }
    }

    /// Call after each sim tick.
    pub fn observe(&mut self, sim: &Sim) {
        if sim.tick < self.tick {
            self.restarted = true;
        }
        self.tick = sim.tick;
        self.clock = sim.clock;
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

/// The director and what voices its notes.
struct Band {
    director: Director,
    voicer: Voicer,
    /// Next bar to decide.
    next_bar: Option<i64>,
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
    fn new(file: &MusicFile) -> Result<Self, String> {
        let name = &file.director.style;
        let style: Style = music_data(&format!("styles/{name}.ron"))?;
        let library: Vec<LibraryTune> = music_data(&format!("library/{name}.ron"))?;
        let bank: PatchBank = music_data("patches.ron")?;
        let seed = run_seed();
        let director = Director::new(style, library, file.director.clone(), seed)
            .ok_or_else(|| format!("the {name} music library is empty"))?;
        info!(
            "music: {name} at {:.0} BPM, run seed {seed}",
            director.bpm()
        );
        Ok(Self {
            director,
            voicer: Voicer::new(bank, file.render.clone(), seed),
            next_bar: None,
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
                self.band = Band::new(&file)
                    .inspect_err(|e| warn!("music: {e}; playing no band"))
                    .ok();
            }
        }
        cues.tempo = Some(match &self.band {
            Some(band) => (band.director.bpm(), band.director.beats_per_bar()),
            None => (file.click.bpm, file.click.beats_per_bar),
        });
        self.file = file;
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
        meter: LatencyMeter::default(),
        latency: 0.0,
        since_log: 0.0,
        muted: false,
    };
    music.file.render.sample_rate = rate;
    music.install(file, &mut cues);
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

/// M mutes and unmutes (the sound effects listen to it too).
fn toggle_mute(keys: Res<ButtonInput<KeyCode>>, mut music: ResMut<Music>) {
    if keys.just_pressed(KeyCode::KeyM) {
        music.muted = !music.muted;
    }
}

fn schedule_music(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
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

    // Follow the sim.
    if std::mem::take(&mut cues.restarted) {
        music.sync.reset();
        music.next_beat = None;
        if let Some(band) = &mut music.band {
            band.next_bar = None;
        }
    }
    let taken = music.shared.taken.load(Ordering::Relaxed);
    let now = cues.tick as f64 + fixed.overstep_fraction() as f64;
    music.sync.observe(now, taken, &p);

    let clock = cues.clock;
    let compensation = p.latency_compensation
        + if p.auto_compensate {
            music.latency
        } else {
            0.0
        };
    let horizon = now + p.lookahead as f64 * TICKS_PER_SECOND as f64;
    let mut notes = Vec::new();

    // The band, a bar at a time.
    if let Some(band) = &mut music.band {
        if music.muted {
            band.next_bar = None;
        } else {
            let bpb = clock.beats_per_bar().max(1) as i64;
            // Start (or resume) on the next bar line, never in the past.
            let first_due = (clock.beats_at(now) / bpb as f64).floor() as i64 + 1;
            let mut bar = band.next_bar.map_or(first_due, |b| b.max(first_due));
            while clock.tick_of_beat((bar * bpb) as f64) <= horizon
                && music.sync.sample_for_tick(now, compensation).is_some()
            {
                let plan = band.director.next_bar(Input {
                    intensity: cues.intensity,
                    duck: cues.duck,
                });
                let (bpm, swing) = (band.director.bpm(), band.director.swing());
                for note in &plan.notes {
                    let Some(voice) = band.voicer.voice(note, (bar * bpb) as f64, bpm, swing)
                    else {
                        continue;
                    };
                    let tick = clock.tick_of_beat(voice.beat);
                    if let Some(at) = music.sync.sample_for_tick(tick, compensation) {
                        notes.push(Scheduled {
                            at,
                            samples: voice.samples,
                            gain: voice.gain,
                        });
                    }
                }
                bar += 1;
            }
            band.next_bar = Some(bar);
        }
    }

    // The click track.
    if !music.file.click.enabled || music.muted {
        music.next_beat = None;
        if !notes.is_empty()
            && let Ok(mut inbox) = music.shared.inbox.lock()
        {
            inbox.extend(notes);
        }
        return;
    }
    // Start (or resume after a pause) on the next beat, never in the past.
    let first_due = clock.beats_at(now).floor() as i64 + 1;
    let mut beat = music.next_beat.map_or(first_due, |b| b.max(first_due));
    while clock.tick_of_beat(beat as f64) <= horizon {
        let tick = clock.tick_of_beat(beat as f64);
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
    if !notes.is_empty()
        && let Ok(mut inbox) = music.shared.inbox.lock()
    {
        inbox.extend(notes);
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
    }

    #[test]
    fn the_built_in_band_starts() {
        let file: MusicFile = from_ron(BUILT_IN).expect("music.ron parses");
        let band = Band::new(&file).expect("the band starts");
        assert!(band.director.bpm() > 0.0);
        for (name, _) in BUILT_IN_DATA {
            if name.starts_with("styles/") {
                let style = name.trim_start_matches("styles/").trim_end_matches(".ron");
                let mut f = file.clone();
                f.director.style = style.into();
                Band::new(&f).unwrap_or_else(|e| panic!("{style}: {e}"));
            }
        }
    }

    #[test]
    fn cues_notice_a_new_room() {
        let mut cues = MusicCues::default();
        let mut sim = Sim::training_room(Vec2::ZERO);
        for _ in 0..10 {
            sim.step(Default::default());
        }
        cues.observe(&sim);
        assert!(!cues.restarted);
        let fresh = Sim::training_room(Vec2::ZERO);
        cues.observe(&fresh);
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
        assert_eq!(sim.clock.beats_per_bar(), 3);
    }
}

//! Sound: turns what happens in the simulation into procedural sound effects.
//!
//! Two halves:
//! - [`CueTracker`] watches the simulation each tick (its events, plus state
//!   changes it has no event for, like landing, sticking, and enemy windups)
//!   and emits [`Cue`]s. It is plain logic, unit tested without Bevy.
//! - [`SoundPlugin`] renders every sound in `assets/sfx.ron` with
//!   `tarborn_sfx`, plays cues with per-play pitch and volume variation, and
//!   reloads the file whenever it changes on disk.
//!
//! Every number lives in `assets/sfx.ron`; nothing here is tuned in code.

use bevy::audio::{AddAudioSource, ChannelCount, Decodable, SampleRate, Source, Volume};
use bevy::prelude::*;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::num::NonZero;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tarborn_sfx::{Library, SAMPLE_RATE, Sfx};
use tarborn_sim::{AttackPhase, DT, Enemy, Event, HitKind, Sim, State};

/// The sound file built into the game, used when the file on disk is missing.
const BUILT_IN: &str = include_str!("../assets/sfx.ron");

/// When sounds fire and how loud. Read from the `cues` section of the sound
/// file; see the comments there.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CueParams {
    pub master_volume: f32,
    pub land_min_speed: f32,
    pub land_full_speed: f32,
    pub land_min_air_time: f32,
    pub stick_min_intensity: f32,
    pub stick_full_speed: f32,
    pub stick_min_air_time: f32,
    pub stick_wall_normal_y: f32,
    pub impact_damage_full: f32,
    pub shockwave_intensity: f32,
    pub struck_intensity: f32,
    pub slam_full_speed: f32,
    pub bounce_full_speed: f32,
    pub throw_full_speed: f32,
    pub hearing_near: f32,
    pub hearing_far: f32,
    pub far_volume: f32,
    pub telegraph_cut_fade: f32,
    pub reload_interval: f32,
}

#[derive(Deserialize)]
struct CueFile {
    cues: CueParams,
}

impl CueParams {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str::<CueFile>(text).map(|f| f.cues)
    }

    /// Volume for a sound made `distance` metres from the blob.
    fn hearing(&self, distance: f32) -> f32 {
        let span = (self.hearing_far - self.hearing_near).max(1e-3);
        let u = ((distance - self.hearing_near) / span).clamp(0.0, 1.0);
        1.0 + (self.far_volume - 1.0) * u
    }
}

/// A request to play a sound.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    /// The sound's name in the sound file.
    pub sound: String,
    /// 0..=1: how hard. Each sound maps it to volume and pitch.
    pub intensity: f32,
    /// Extra volume factor, from distance to the blob.
    pub gain: f32,
    /// The enemy whose windup this announces, so it can be cut if
    /// interrupted.
    pub owner: Option<usize>,
}

/// Watches the simulation tick by tick and decides which sounds to play.
#[derive(Debug, Default)]
pub struct CueTracker {
    last_tick: u64,
    touching: bool,
    on_wall: bool,
    air_time: f32,
    /// Blob velocity at the end of the previous tick: the speed it hit with.
    velocity: Vec2,
    states: Vec<State>,
    /// Cues waiting to be played.
    pub cues: Vec<Cue>,
    /// Enemies whose windup ended: cut their telegraph sound.
    pub cuts: Vec<usize>,
    /// The room restarted: cut every telegraph.
    pub cut_all: bool,
}

fn ramp(value: f32, from: f32, to: f32) -> f32 {
    ((value - from) / (to - from).max(1e-3)).clamp(0.0, 1.0)
}

impl CueTracker {
    /// Call once after every simulation tick, with the events it returned.
    pub fn observe(&mut self, sim: &Sim, events: &[Event], p: &CueParams) {
        let blob = &sim.blob;
        let center = blob.center();
        let shrunk = sim.player_shrunk();
        let touching = blob.touching_count() > 0;
        let on_wall = shrunk
            && blob
                .contacts
                .iter()
                .flatten()
                .any(|n| n.y < p.stick_wall_normal_y);

        if sim.tick <= self.last_tick {
            // A new room: start over without sounding everything that differs.
            *self = Self {
                cues: std::mem::take(&mut self.cues),
                cut_all: true,
                ..Self::default()
            };
            self.remember(sim, touching, on_wall);
            return;
        }

        let mut play = |sound: &str, intensity: f32, at: Option<Vec2>, owner: Option<usize>| {
            self.cues.push(Cue {
                sound: sound.to_owned(),
                intensity: intensity.clamp(0.0, 1.0),
                gain: at.map_or(1.0, |at| p.hearing(at.distance(center))),
                owner,
            });
        };
        let enemy_at = |i: usize| sim.enemies.get(i).map(|e| e.position);

        let mut landed_by_move = false;
        for event in events {
            match *event {
                Event::Jumped { .. } => play("jump", 1.0, None, None),
                Event::Launched { power, .. } => play("launch", power, None, None),
                Event::Lunged { .. } => play("lunge", 1.0, None, None),
                Event::SlamStarted => play("slam_start", 1.0, None, None),
                Event::Shrank => play("shrink", 1.0, None, None),
                Event::Slammed { speed, .. } => {
                    landed_by_move = true;
                    play("slam", speed / p.slam_full_speed, None, None);
                }
                Event::Bounced { .. } => {
                    landed_by_move = true;
                    play(
                        "bounce",
                        self.velocity.length() / p.bounce_full_speed,
                        None,
                        None,
                    );
                }
                Event::Hit {
                    kind, damage, at, ..
                } => {
                    let k = damage / p.impact_damage_full;
                    match kind {
                        HitKind::Lunge | HitKind::Impact => play("impact", k, None, None),
                        HitKind::Slam => play("impact_heavy", k, None, None),
                        HitKind::Shockwave => {
                            play("impact", k * p.shockwave_intensity, Some(at), None)
                        }
                        HitKind::Pogo => play("pogo", k, None, None),
                    }
                }
                Event::Struck { damage, at, .. } => play(
                    "impact",
                    damage / p.impact_damage_full * p.struck_intensity,
                    Some(at),
                    None,
                ),
                Event::Killed { at, .. } => play("kill", 1.0, Some(at), None),
                Event::Blocked { at, .. } => play("block", 1.0, Some(at), None),
                Event::Grabbed { .. } => play("grab", 1.0, None, None),
                Event::Thrown { speed, .. } => {
                    play("throw", speed / p.throw_full_speed, None, None)
                }
                Event::Dropped { .. } => play("drop", 1.0, None, None),
                Event::Dodged { .. } => play("dodge", 1.0, None, None),
                Event::Evaded { at, .. } => play("evade", 1.0, Some(at), None),
                Event::Quake { at, .. } => play("quake", 1.0, Some(at), None),
                Event::Eaten { enemy, .. } => play("eaten", 1.0, enemy_at(enemy), None),
                Event::WaveStarted { .. } => play("wave", 1.0, None, None),
                Event::ArenaCleared => play("cleared", 1.0, None, None),
                Event::PlayerHurt { .. } => play("hurt", 1.0, None, None),
                Event::PlayerDied => play("death", 1.0, None, None),
                Event::Healed { .. } => play("heal", 1.0, None, None),
            }
        }

        // Touching down. A slam or bounce already made its own sound.
        if touching && !self.touching && !landed_by_move {
            if shrunk {
                if self.air_time >= p.stick_min_air_time {
                    let k =
                        (self.velocity.length() / p.stick_full_speed).max(p.stick_min_intensity);
                    play("stick", k, None, None);
                }
            } else if self.air_time >= p.land_min_air_time {
                let fall = -self.velocity.y;
                if fall >= p.land_min_speed {
                    play(
                        "land",
                        ramp(fall, p.land_min_speed, p.land_full_speed),
                        None,
                        None,
                    );
                }
            }
        } else if on_wall && !self.on_wall && self.touching {
            // Rolled from the floor onto a wall or ceiling and gripped it.
            let k = (self.velocity.length() / p.stick_full_speed).max(p.stick_min_intensity);
            play("stick", k, None, None);
        }

        // Enemy telegraphs and alerts, from state changes.
        for (i, enemy) in sim.enemies.iter().enumerate() {
            let Some(&before) = self.states.get(i) else {
                continue;
            };
            if before == enemy.state || !enemy.alive() {
                continue;
            }
            let kind = kind_name(enemy);
            let windup = State::Attacking(AttackPhase::Windup);
            if before == windup {
                self.cuts.push(i);
            }
            match enemy.state {
                State::Attacking(AttackPhase::Windup) => play(
                    &format!("{kind}_windup"),
                    1.0,
                    Some(enemy.position),
                    Some(i),
                ),
                State::Attacking(AttackPhase::Strike) => {
                    play(&format!("{kind}_strike"), 1.0, Some(enemy.position), None)
                }
                State::Surprised => play("surprised", 1.0, Some(enemy.position), None),
                _ => {}
            }
        }

        self.air_time = if touching { 0.0 } else { self.air_time + DT };
        self.remember(sim, touching, on_wall);
    }

    fn remember(&mut self, sim: &Sim, touching: bool, on_wall: bool) {
        self.last_tick = sim.tick;
        self.touching = touching;
        self.on_wall = on_wall;
        self.velocity = sim.blob.velocity();
        self.states.clear();
        self.states.extend(sim.enemies.iter().map(|e| e.state));
    }
}

fn kind_name(enemy: &Enemy) -> String {
    format!("{:?}", enemy.kind).to_lowercase()
}

// ---- Bevy side --------------------------------------------------------------

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<Synth>()
            .init_resource::<SoundCues>()
            .add_systems(Startup, load_bank)
            .add_systems(
                Update,
                (reload_bank, toggle_mute, play_cues, fade_voices).chain(),
            );
    }
}

/// Filled by the fixed tick (see `observe`), drained by the sound systems.
#[derive(Resource, Default)]
pub struct SoundCues {
    tracker: CueTracker,
    params: Option<CueParams>,
}

impl SoundCues {
    /// Call after each simulation tick with the events it returned.
    pub fn observe(&mut self, sim: &Sim, events: &[Event]) {
        if let Some(params) = &self.params {
            self.tracker.observe(sim, events, params);
        }
    }
}

/// A rendered sound, played straight from memory.
#[derive(Asset, TypePath, Clone)]
pub struct Synth {
    samples: Arc<[f32]>,
}

pub struct SynthDecoder {
    samples: Arc<[f32]>,
    next: usize,
}

impl Iterator for SynthDecoder {
    type Item = bevy::audio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        let s = self.samples.get(self.next)?;
        self.next += 1;
        Some(*s as bevy::audio::Sample)
    }
}

impl Source for SynthDecoder {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.samples.len() - self.next)
    }

    fn channels(&self) -> ChannelCount {
        NonZero::<u16>::MIN
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(SAMPLE_RATE).expect("non-zero rate")
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(
            self.samples.len() as f64 / SAMPLE_RATE as f64,
        ))
    }
}

impl Decodable for Synth {
    type Decoder = SynthDecoder;

    fn decoder(&self) -> SynthDecoder {
        SynthDecoder {
            samples: self.samples.clone(),
            next: 0,
        }
    }
}

struct Loaded {
    sfx: Sfx,
    handle: Handle<Synth>,
}

#[derive(Resource)]
struct SoundBank {
    path: PathBuf,
    modified: Option<SystemTime>,
    since_check: f32,
    sounds: HashMap<String, Loaded>,
    last_played: HashMap<String, f32>,
    missing: HashSet<String>,
    muted: bool,
    rng: u32,
}

/// Plays one sound; it fades out instead of stopping dead when cut.
#[derive(Component)]
struct Voice {
    sound: String,
    owner: Option<usize>,
    volume: f32,
    fade: Option<(f32, f32)>,
}

fn sound_file() -> PathBuf {
    std::env::var_os("TARBORN_SFX")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sfx.ron")))
}

fn parse(text: &str) -> Result<(CueParams, Library), String> {
    let cues = CueParams::from_ron(text).map_err(|e| e.to_string())?;
    let library = Library::from_ron(text).map_err(|e| e.to_string())?;
    Ok((cues, library))
}

fn modified(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn install(
    text: &str,
    bank: &mut SoundBank,
    cues: &mut SoundCues,
    assets: &mut Assets<Synth>,
) -> Result<(), String> {
    let (params, library) = parse(text)?;
    for (_, old) in bank.sounds.drain() {
        assets.remove(&old.handle);
    }
    for (name, sfx) in library.sounds {
        let samples: Arc<[f32]> = sfx.render(SAMPLE_RATE).into();
        let handle = assets.add(Synth { samples });
        bank.sounds.insert(name, Loaded { sfx, handle });
    }
    bank.missing.clear();
    cues.params = Some(params);
    Ok(())
}

fn load_bank(
    mut commands: Commands,
    mut cues: ResMut<SoundCues>,
    mut assets: ResMut<Assets<Synth>>,
) {
    let path = sound_file();
    let mut bank = SoundBank {
        modified: modified(&path),
        path,
        since_check: 0.0,
        sounds: HashMap::new(),
        last_played: HashMap::new(),
        missing: HashSet::new(),
        muted: false,
        rng: 0x1234_5678,
    };
    let from_disk = std::fs::read_to_string(&bank.path).ok();
    let loaded = from_disk
        .as_deref()
        .map(|text| install(text, &mut bank, &mut cues, &mut assets));
    match loaded {
        Some(Ok(())) => info!("sounds loaded from {}", bank.path.display()),
        other => {
            if let Some(Err(e)) = other {
                warn!("{}: {e}; using the built-in sounds", bank.path.display());
            }
            install(BUILT_IN, &mut bank, &mut cues, &mut assets)
                .expect("the built-in sound file parses");
        }
    }
    commands.insert_resource(bank);
}

fn reload_bank(
    time: Res<Time>,
    mut bank: ResMut<SoundBank>,
    mut cues: ResMut<SoundCues>,
    mut assets: ResMut<Assets<Synth>>,
) {
    let interval = cues.params.as_ref().map_or(0.5, |p| p.reload_interval);
    bank.since_check += time.delta_secs();
    if bank.since_check < interval {
        return;
    }
    bank.since_check = 0.0;
    let stamp = modified(&bank.path);
    if stamp.is_none() || stamp == bank.modified {
        return;
    }
    bank.modified = stamp;
    let Ok(text) = std::fs::read_to_string(&bank.path) else {
        return;
    };
    match install(&text, &mut bank, &mut cues, &mut assets) {
        Ok(()) => info!("sounds reloaded from {}", bank.path.display()),
        Err(e) => warn!("{}: {e}; keeping the previous sounds", bank.path.display()),
    }
}

/// M mutes and unmutes.
fn toggle_mute(keys: Res<ButtonInput<KeyCode>>, mut bank: ResMut<SoundBank>) {
    if keys.just_pressed(KeyCode::KeyM) {
        bank.muted = !bank.muted;
    }
}

impl SoundBank {
    fn random(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x as f32 / u32::MAX as f32
    }
}

fn play_cues(
    mut commands: Commands,
    time: Res<Time>,
    mut bank: ResMut<SoundBank>,
    mut cues: ResMut<SoundCues>,
    mut voices: Query<(Entity, &mut Voice)>,
) {
    let Some(params) = cues.params.clone() else {
        return;
    };
    let fade = params.telegraph_cut_fade.max(1e-3);
    let cut_all = std::mem::take(&mut cues.tracker.cut_all);
    let cuts = std::mem::take(&mut cues.tracker.cuts);
    for (_, mut voice) in &mut voices {
        if voice.fade.is_none() && voice.owner.is_some_and(|o| cut_all || cuts.contains(&o)) {
            voice.fade = Some((fade, fade));
        }
    }

    let now = time.elapsed_secs();
    for cue in std::mem::take(&mut cues.tracker.cues) {
        if bank.muted {
            continue;
        }
        let Some(loaded) = bank.sounds.get(&cue.sound) else {
            if bank.missing.insert(cue.sound.clone()) {
                debug!("no sound named {:?} in the sound file", cue.sound);
            }
            continue;
        };
        let sfx = &loaded.sfx;
        let handle = loaded.handle.clone();
        let (cooldown, max_voices) = (sfx.cooldown, sfx.max_voices as usize);
        let last = bank.last_played.get(&cue.sound).copied();
        if last.is_some_and(|t| now - t < cooldown) {
            continue;
        }
        let playing = voices
            .iter()
            .filter(|(_, v)| v.sound == cue.sound && v.fade.is_none())
            .count();
        if playing >= max_voices {
            continue;
        }
        let (r1, r2) = (bank.random(), bank.random());
        let sfx = &bank.sounds[&cue.sound].sfx;
        let (volume, pitch) = sfx.variation(cue.intensity, r1, r2);
        let volume = volume * cue.gain * params.master_volume;
        bank.last_played.insert(cue.sound.clone(), now);
        commands.spawn((
            AudioPlayer(handle),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(volume))
                .with_speed(pitch),
            Voice {
                sound: cue.sound,
                owner: cue.owner,
                volume,
                fade: None,
            },
        ));
    }
}

fn fade_voices(
    mut commands: Commands,
    time: Res<Time>,
    mut voices: Query<(Entity, &mut Voice, Option<&mut AudioSink>)>,
) {
    for (entity, mut voice, sink) in &mut voices {
        let Some((left, total)) = voice.fade else {
            continue;
        };
        let left = left - time.delta_secs();
        match sink {
            Some(mut sink) if left > 0.0 => {
                sink.set_volume(Volume::Linear(voice.volume * left / total));
                voice.fade = Some((left, total));
            }
            _ => commands.entity(entity).despawn(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tarborn_sim::{Enemy, Intent, Level, Solid, TarModes};

    fn params() -> CueParams {
        CueParams::from_ron(BUILT_IN).expect("the sound file parses")
    }

    fn floor() -> Level {
        Level {
            solids: vec![Solid::rect(Vec2::new(-100.0, -2.0), Vec2::new(100.0, 0.0))],
        }
    }

    /// Runs the sim for `seconds` and returns every cue fired.
    fn run(sim: &mut Sim, tracker: &mut CueTracker, intent: Intent, seconds: f32) -> Vec<Cue> {
        let p = params();
        for _ in 0..(seconds / DT).round() as usize {
            let events = sim.step(intent);
            tracker.observe(sim, &events, &p);
        }
        std::mem::take(&mut tracker.cues)
    }

    fn names(cues: &[Cue]) -> Vec<&str> {
        cues.iter().map(|c| c.sound.as_str()).collect()
    }

    fn grip() -> Intent {
        Intent {
            modes: TarModes {
                sticky: true,
                bouncy: false,
            },
            ..Intent::default()
        }
    }

    #[test]
    fn the_cue_section_parses() {
        let p = params();
        assert!(p.land_full_speed > p.land_min_speed);
        assert!(p.hearing_far > p.hearing_near);
    }

    #[test]
    fn every_cue_the_game_can_fire_has_a_sound() {
        let library = Library::from_ron(BUILT_IN).unwrap();
        let mut expected: Vec<String> = [
            "jump",
            "launch",
            "lunge",
            "slam_start",
            "shrink",
            "slam",
            "bounce",
            "impact",
            "impact_heavy",
            "pogo",
            "kill",
            "block",
            "grab",
            "throw",
            "drop",
            "dodge",
            "evade",
            "quake",
            "eaten",
            "wave",
            "cleared",
            "hurt",
            "death",
            "heal",
            "land",
            "stick",
            "surprised",
        ]
        .map(String::from)
        .to_vec();
        // The brute's strike is its quake, which has its own sound.
        for kind in ["guard", "spitter", "skitter", "brute"] {
            expected.push(format!("{kind}_windup"));
        }
        for kind in ["guard", "spitter", "skitter"] {
            expected.push(format!("{kind}_strike"));
        }
        for name in expected {
            assert!(library.sounds.contains_key(&name), "no sound for {name}");
        }
    }

    #[test]
    fn telegraphs_last_about_as_long_as_the_windup() {
        use tarborn_sim::enemy::{BRUTE_WINDUP, GUARD_WINDUP, SKITTER_WINDUP, SPIT_AIM};
        let library = Library::from_ron(BUILT_IN).unwrap();
        for (name, windup) in [
            ("guard_windup", GUARD_WINDUP),
            ("spitter_windup", SPIT_AIM),
            ("skitter_windup", SKITTER_WINDUP),
            ("brute_windup", BRUTE_WINDUP),
        ] {
            let len = library.sounds[name].length();
            assert!(
                (len - windup).abs() < 0.1,
                "{name} lasts {len} s for a {windup} s windup"
            );
        }
    }

    #[test]
    fn falling_onto_the_floor_lands_harder_from_higher() {
        let landing = |height: f32| {
            let mut sim = Sim::new(floor(), Vec2::new(0.0, height));
            let mut tracker = CueTracker::default();
            let cues = run(&mut sim, &mut tracker, Intent::default(), 2.0);
            cues.into_iter()
                .filter(|c| c.sound == "land")
                .collect::<Vec<_>>()
        };
        let low = landing(1.5);
        let high = landing(8.0);
        assert_eq!(low.len(), 1, "{low:?}");
        // A big fall rebounds, and the rebound lands again more softly.
        assert!(!high.is_empty() && high.len() <= 2, "{high:?}");
        assert!(high[0].intensity > low[0].intensity);
        assert!(high.windows(2).all(|w| w[1].intensity < w[0].intensity));
    }

    #[test]
    fn resting_and_rolling_on_the_floor_are_quiet() {
        let mut sim = Sim::new(floor(), Vec2::new(0.0, 0.6));
        let mut tracker = CueTracker::default();
        run(&mut sim, &mut tracker, Intent::default(), 1.0);
        let roll = Intent {
            roll: 1.0,
            ..Intent::default()
        };
        let cues = run(&mut sim, &mut tracker, roll, 2.0);
        assert!(cues.is_empty(), "{:?}", names(&cues));
    }

    #[test]
    fn jumping_plays_jump_then_land() {
        let mut sim = Sim::new(floor(), Vec2::new(0.0, 0.6));
        let mut tracker = CueTracker::default();
        run(&mut sim, &mut tracker, Intent::default(), 1.0);
        let press = Intent {
            jump_pressed: true,
            jump_held: false,
            ..Intent::default()
        };
        let mut cues = run(&mut sim, &mut tracker, press, DT);
        cues.extend(run(&mut sim, &mut tracker, Intent::default(), 2.0));
        assert_eq!(names(&cues), ["jump", "land"]);
    }

    #[test]
    fn falling_while_gripping_sticks_instead_of_landing() {
        let mut sim = Sim::new(floor(), Vec2::new(0.0, 4.0));
        let mut tracker = CueTracker::default();
        let cues = run(&mut sim, &mut tracker, grip(), 2.0);
        let names = names(&cues);
        assert!(names.contains(&"stick"), "{names:?}");
        assert!(!names.contains(&"land"), "{names:?}");
    }

    #[test]
    fn rolling_into_a_wall_while_gripping_sticks() {
        let mut level = floor();
        level
            .solids
            .push(Solid::rect(Vec2::new(3.0, 0.0), Vec2::new(5.0, 6.0)));
        let mut sim = Sim::new(level, Vec2::new(0.0, 0.6));
        let mut tracker = CueTracker::default();
        run(&mut sim, &mut tracker, Intent::default(), 1.0);
        let cues = run(
            &mut sim,
            &mut tracker,
            Intent {
                roll: 1.0,
                ..grip()
            },
            3.0,
        );
        assert!(names(&cues).contains(&"stick"), "{:?}", names(&cues));
    }

    #[test]
    fn a_guard_telegraphs_its_swing_and_far_sounds_are_quieter() {
        let mut sim = Sim::new(floor(), Vec2::new(0.0, 0.6));
        sim.enemies.push(Enemy::guard(Vec2::new(2.5, 0.6)));
        let mut tracker = CueTracker::default();
        let cues = run(&mut sim, &mut tracker, Intent::default(), 4.0);
        let windup = cues
            .iter()
            .find(|c| c.sound == "guard_windup")
            .unwrap_or_else(|| panic!("no telegraph in {:?}", names(&cues)));
        assert_eq!(windup.owner, Some(0));
        assert!(names(&cues).contains(&"guard_strike"));
        let p = params();
        assert_eq!(p.hearing(0.0), 1.0);
        assert!((p.hearing(1000.0) - p.far_volume).abs() < 1e-6);
    }

    #[test]
    fn hitting_a_windup_cuts_its_telegraph() {
        let mut sim = Sim::new(floor(), Vec2::new(0.0, 0.6));
        sim.enemies.push(Enemy::guard(Vec2::new(2.5, 0.6)));
        let mut tracker = CueTracker::default();
        let p = params();
        let mut saw_windup = false;
        for _ in 0..(4.0 / DT) as usize {
            let events = sim.step(Intent::default());
            tracker.observe(&sim, &events, &p);
            if sim.enemies[0].state == State::Attacking(AttackPhase::Windup) {
                saw_windup = true;
                // Interrupt it.
                sim.enemies[0].state = State::Dizzy;
                let events = sim.step(Intent::default());
                tracker.observe(&sim, &events, &p);
                break;
            }
        }
        assert!(saw_windup);
        assert_eq!(tracker.cuts, [0]);
    }

    #[test]
    fn a_new_room_resets_quietly_and_cuts_telegraphs() {
        let p = params();
        let mut tracker = CueTracker::default();
        let mut sim = Sim::training_room(Vec2::new(0.0, 2.0));
        for _ in 0..60 {
            let events = sim.step(Intent::default());
            tracker.observe(&sim, &events, &p);
        }
        tracker.cues.clear();
        let mut sim = Sim::arena_room();
        let events = sim.step(Intent::default());
        tracker.observe(&sim, &events, &p);
        assert!(tracker.cut_all);
        assert!(tracker.cues.is_empty());
    }

    #[test]
    fn the_plugin_renders_the_bank_and_plays_cues() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::audio::AudioPlugin::default(),
            bevy::input::InputPlugin,
            SoundPlugin,
        ));
        app.update();
        let count = app.world().resource::<SoundBank>().sounds.len();
        assert_eq!(count, Library::from_ron(BUILT_IN).unwrap().sounds.len());

        let mut sim = Sim::new(floor(), Vec2::new(0.0, 0.6));
        sim.enemies.push(Enemy::guard(Vec2::new(2.5, 0.6)));
        for _ in 0..(4.0 / DT) as usize {
            let events = sim.step(Intent::default());
            app.world_mut()
                .resource_mut::<SoundCues>()
                .observe(&sim, &events);
        }
        app.update();
        let world = app.world_mut();
        let voices: Vec<String> = world
            .query::<&Voice>()
            .iter(world)
            .map(|v| v.sound.clone())
            .collect();
        assert!(voices.iter().any(|v| v == "guard_windup"), "{voices:?}");
        assert!(world.resource::<SoundCues>().tracker.cues.is_empty());
    }

    #[test]
    fn the_decoder_streams_every_sample_once() {
        let synth = Synth {
            samples: vec![0.0, 0.5, -0.5].into(),
        };
        let decoder = synth.decoder();
        assert_eq!(decoder.channels().get(), 1);
        assert_eq!(decoder.sample_rate().get(), SAMPLE_RATE);
        assert_eq!(decoder.current_span_len(), Some(3));
        let out: Vec<f32> = decoder.collect();
        assert_eq!(out, [0.0, 0.5, -0.5]);
    }
}

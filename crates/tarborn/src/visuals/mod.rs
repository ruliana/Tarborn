//! The look: filled geometry, a glossy tar shader, generated level dressing
//! (see [`dressing`]) and a full-screen finish. V cycles between looks so they can be compared
//! in play. Every number comes from `visuals.ron` (see [`tuning`]).

mod dressing;
mod face;
mod fill;
mod stains;
mod tuning;
pub mod wfc;

use bevy::asset::embedded_asset;
use bevy::core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::core_pipeline::{Core2d, Core2dSystems};
use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::BoxedSystem;
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::RenderApp;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};
use tarborn_sim::combat::SPIT_RADIUS;
use tarborn_sim::{AttackPhase, Enemy, EnemyKind, Event, HitKind, Sim, State};

use crate::{Game, InputLatch, PIXELS_PER_METER, Room};
use face::{Eye, Mood, Pose};
use fill::Fill;
use stains::Stains;
pub use tuning::VisualTuning;
use tuning::{linear, srgb};

const TAR_SHADER: &str = "embedded://tarborn/visuals/shaders/tar.wesl";
const POST_SHADER: &str = "embedded://tarborn/visuals/shaders/post.wesl";
/// The tar shader holds this many outline points.
const MAX_TAR_POINTS: usize = 32;

const LEVEL_Z: f32 = -10.0;
/// Above the dressing's hanging parts, below the actors.
const STAINS_Z: f32 = -8.5;
const ACTORS_Z: f32 = 1.0;
const TAR_Z: f32 = 2.0;
/// Covers everything in the dark room, except the eyes above it.
const DARK_Z: f32 = 3.0;
const EYES_Z: f32 = 3.5;

pub struct VisualsPlugin;

impl Plugin for VisualsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/tar.wesl");
        embedded_asset!(app, "shaders/post.wesl");
        app.add_plugins((
            Material2dPlugin::<TarMaterial>::default(),
            FullscreenMaterialPlugin::<PostEffect>::default(),
            dressing::DressingPlugin,
        ))
        .init_resource::<VisualTuning>()
        .init_resource::<Look>()
        .init_resource::<Stains>()
        .init_resource::<Faces>()
        .add_systems(Startup, spawn_layers)
        .add_systems(
            Update,
            (
                tuning::reload,
                cycle_look,
                apply_look,
                dressing::systems(),
                build_level,
                observe_events,
                build_actors,
                update_dark,
                update_tar,
                update_post,
            )
                .chain()
                .after(crate::follow_camera),
        );
    }
}

/// Which look is showing.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Look {
    /// The original outline drawing.
    Lines,
    /// Filled shapes, glossy tar, grain and vignette.
    #[default]
    Pitch,
    /// Pitch, dithered down to four colors.
    PitchDither,
}

impl Look {
    pub fn filled(self) -> bool {
        self != Look::Lines
    }
}

/// Darkens outline and mark colors so they read on the pale background in
/// the filled looks, keeping their hue. A no-op for the line look.
#[derive(Clone, Copy)]
pub struct Ink {
    max_lightness: Option<f32>,
}

impl Ink {
    pub fn new(look: Look, tuning: &VisualTuning) -> Self {
        Self {
            max_lightness: look.filled().then_some(tuning.enemy.ink_max_lightness),
        }
    }

    pub fn apply(self, color: Color) -> Color {
        let Some(max) = self.max_lightness else {
            return color;
        };
        let mut c = Oklaba::from(color);
        c.lightness = c.lightness.min(max);
        c.into()
    }
}

fn cycle_look(keys: Res<ButtonInput<KeyCode>>, mut look: ResMut<Look>) {
    if keys.just_pressed(KeyCode::KeyV) {
        *look = match *look {
            Look::Lines => Look::Pitch,
            Look::Pitch => Look::PitchDither,
            Look::PitchDither => Look::Lines,
        };
        info!("look: {:?}", *look);
    }
}

type AnyLayer = Or<(
    With<LevelLayer>,
    With<ActorsLayer>,
    With<TarLayer>,
    With<StainsLayer>,
    With<EyesLayer>,
)>;

#[derive(Component)]
struct LevelLayer;
#[derive(Component)]
struct ActorsLayer;
#[derive(Component)]
struct TarLayer;
#[derive(Component)]
struct StainsLayer;
#[derive(Component)]
struct EyesLayer;
#[derive(Component)]
struct DarkLayer;

fn spawn_layers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut colors: ResMut<Assets<ColorMaterial>>,
    mut tars: ResMut<Assets<TarMaterial>>,
) {
    let plain = colors.add(ColorMaterial::default());
    let blend = colors.add(ColorMaterial {
        alpha_mode: AlphaMode2d::Blend,
        ..default()
    });
    commands.spawn((
        StainsLayer,
        Mesh2d(meshes.add(Fill::default().into_mesh())),
        MeshMaterial2d(blend.clone()),
        Transform::from_xyz(0.0, 0.0, STAINS_Z),
    ));
    commands.spawn((
        EyesLayer,
        Mesh2d(meshes.add(Fill::default().into_mesh())),
        MeshMaterial2d(blend),
        Transform::from_xyz(0.0, 0.0, EYES_Z),
    ));
    commands.spawn((
        DarkLayer,
        Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial2d(colors.add(ColorMaterial {
            color: Color::BLACK,
            alpha_mode: AlphaMode2d::Blend,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.0, DARK_Z),
        Visibility::Hidden,
    ));
    commands.spawn((
        LevelLayer,
        Mesh2d(meshes.add(Fill::default().into_mesh())),
        MeshMaterial2d(plain.clone()),
        Transform::from_xyz(0.0, 0.0, LEVEL_Z),
    ));
    commands.spawn((
        ActorsLayer,
        Mesh2d(meshes.add(Fill::default().into_mesh())),
        MeshMaterial2d(plain),
        Transform::from_xyz(0.0, 0.0, ACTORS_Z),
    ));
    commands.spawn((
        TarLayer,
        Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial2d(tars.add(TarMaterial::default())),
        Transform::from_xyz(0.0, 0.0, TAR_Z),
    ));
}

/// Shows or hides the filled layers and the finish to match the look.
fn apply_look(
    look: Res<Look>,
    tuning: Res<VisualTuning>,
    mut clear: ResMut<ClearColor>,
    mut layers: Query<&mut Visibility, AnyLayer>,
    camera: Single<(Entity, Has<PostEffect>), With<Camera2d>>,
    mut commands: Commands,
    mut lines_clear: Local<Option<Color>>,
) {
    if !look.is_changed() && !tuning.is_changed() {
        return;
    }
    // The filled looks take the background from the dressing set.
    let lines = *lines_clear.get_or_insert(clear.0);
    if !look.filled() {
        clear.0 = lines;
    }
    for mut visibility in &mut layers {
        *visibility = if look.filled() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    let (camera, has_post) = *camera;
    if look.filled() && !has_post {
        commands.entity(camera).insert(PostEffect::default());
    } else if !look.filled() && has_post {
        commands.entity(camera).remove::<PostEffect>();
    }
}

// ---------------------------------------------------------------- level

/// The solids the level mesh was last built from, and the tuning it used.
#[derive(Default)]
struct BuiltLevel {
    solids: Vec<Vec<Vec2>>,
    tuning: Option<VisualTuning>,
}

fn build_level(
    game: Res<Game>,
    tuning: Res<VisualTuning>,
    look: Res<Look>,
    layer: Single<&Mesh2d, With<LevelLayer>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut built: Local<BuiltLevel>,
) {
    if !look.filled() {
        return;
    }
    let solids: Vec<Vec<Vec2>> = game
        .sim
        .level
        .solids
        .iter()
        .map(|s| s.vertices.clone())
        .collect();
    if built.solids == solids && built.tuning.as_ref() == Some(&*tuning) {
        return;
    }
    let mesh = level_fill(&game.sim, &tuning).into_mesh();
    if let Some(mut handle) = meshes.get_mut(&layer.0) {
        *handle = mesh;
    }
    built.solids = solids;
    built.tuning = Some(tuning.clone());
}

/// The spikes; the solids themselves are drawn by the dressing.
fn level_fill(sim: &Sim, tuning: &VisualTuning) -> Fill {
    let look = &tuning.level;
    let mut fill = Fill::default();
    // Spikes as filled teeth.
    let hazard = srgb(look.hazard);
    for spikes in &sim.hazards {
        let (lo, hi) = spikes
            .vertices
            .iter()
            .fold((Vec2::MAX, Vec2::MIN), |(lo, hi), v| {
                (lo.min(*v), hi.max(*v))
            });
        let (lo, hi) = (lo * PIXELS_PER_METER, hi * PIXELS_PER_METER);
        let teeth = ((hi.x - lo.x) / look.spike_width).ceil().max(1.0) as usize;
        let w = (hi.x - lo.x) / teeth as f32;
        let top = lo.y.max(hi.y - look.spike_height).max(lo.y + 1.0);
        for k in 0..teeth {
            let x = lo.x + w * k as f32;
            fill.polygon(
                &[
                    Vec2::new(x, lo.y),
                    Vec2::new(x + w, lo.y),
                    Vec2::new(x + w * 0.5, hi.y.max(top)),
                ],
                hazard,
            );
        }
    }
    fill
}

// ---------------------------------------------------------------- actors

/// What faces remember between frames, so they blend instead of snapping.
#[derive(Resource, Default)]
struct Faces {
    enemies: Vec<Mood>,
    gaze: Vec<Vec2>,
    blob: Option<BlobMemory>,
    /// Seconds of wide eyes left on the blob.
    wide_left: f32,
    last_tick: u64,
}

struct BlobMemory {
    mood: Mood,
    gaze: Vec2,
    /// Where the eyes sit (world px) and how fast they move: they lag the
    /// body on a spring.
    eyes: Vec2,
    velocity: Vec2,
}

/// The dark room's opacity, or 0 elsewhere.
fn darkness(game: &Game, tuning: &VisualTuning) -> f32 {
    if game.room == Room::Dark {
        tuning.face.dark.alpha
    } else {
        0.0
    }
}

/// Feeds this frame's simulation events to the stains and the blob's face.
fn observe_events(
    mut game: ResMut<Game>,
    tuning: Res<VisualTuning>,
    time: Res<Time>,
    mut stains: ResMut<Stains>,
    mut faces: ResMut<Faces>,
) {
    let events = std::mem::take(&mut game.visual_events);
    stains.observe(&game.sim, &events, time.elapsed_secs(), &tuning.stains);
    let wide = events.iter().any(|e| {
        matches!(
            e,
            Event::Hit {
                kind: HitKind::Pogo | HitKind::Lunge,
                ..
            } | Event::Bounced { .. }
        )
    });
    if wide {
        faces.wide_left = tuning.face.blob.wide_time;
    }
    faces.wide_left = (faces.wide_left - time.delta_secs()).max(0.0);
}

type CameraOnly = (With<Camera2d>, Without<DarkLayer>);

/// Keeps the dark over the view in the dark room.
fn update_dark(
    game: Res<Game>,
    tuning: Res<VisualTuning>,
    look: Res<Look>,
    camera: Single<(&Transform, &Projection), CameraOnly>,
    window: Single<&Window>,
    layer: Single<
        (
            &MeshMaterial2d<ColorMaterial>,
            &mut Transform,
            &mut Visibility,
        ),
        With<DarkLayer>,
    >,
    mut colors: ResMut<Assets<ColorMaterial>>,
) {
    let (material, mut transform, mut visibility) = layer.into_inner();
    let alpha = darkness(&game, &tuning);
    if alpha <= 0.0 || !look.filled() {
        *visibility = Visibility::Hidden;
        return;
    }
    *visibility = Visibility::Inherited;
    let (camera, projection) = *camera;
    let zoom = match projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => 1.0,
    };
    // Generous, so camera shake and rotation never show an edge.
    let size = Vec2::new(window.width(), window.height()) * zoom * 2.0;
    transform.translation = camera.translation.truncate().extend(DARK_Z);
    transform.scale = size.extend(1.0);
    if let Some(mut m) = colors.get_mut(&material.0) {
        m.color = srgb(tuning.face.dark.color).with_alpha(alpha);
    }
}

#[allow(clippy::too_many_arguments)]
fn build_actors(
    game: Res<Game>,
    latch: Res<InputLatch>,
    tuning: Res<VisualTuning>,
    look: Res<Look>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    stains: Res<Stains>,
    mut faces: ResMut<Faces>,
    actors: Single<&Mesh2d, With<ActorsLayer>>,
    eyes_layer: Single<&Mesh2d, With<EyesLayer>>,
    stains_layer: Single<&Mesh2d, With<StainsLayer>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if !look.filled() {
        return;
    }
    let alpha = fixed.overstep_fraction();
    let now = time.elapsed_secs();
    let blend = 1.0 - (-tuning.face.blend * time.delta_secs()).exp();
    let sim = &game.sim;
    let t = &tuning.enemy;
    let f = &tuning.face;
    let dark = darkness(&game, &tuning);
    let dark_color = srgb(f.dark.color);
    // Anything drawn above the dark that isn't meant to glow is darkened with it.
    let shade = |c: Color| c.mix(&dark_color, dark);
    let ink = Ink::new(*look, &tuning);
    let mut fill = Fill::default();
    let mut eyes = Fill::default();

    if sim.tick < faces.last_tick {
        *faces = Faces::default();
    }
    faces.last_tick = sim.tick;
    faces.enemies.resize(sim.enemies.len(), f.moods.unaware);
    faces.gaze.resize(sim.enemies.len(), Vec2::ZERO);
    let player = sim.blob.center() * PIXELS_PER_METER;

    for (i, enemy) in sim.enemies.iter().enumerate() {
        if !enemy.alive() {
            faces.enemies[i] = f.moods.unaware;
            continue;
        }
        let at = game.enemy_position(i, alpha);
        let r = enemy.radius * PIXELS_PER_METER;
        let phase = i as f32 * 1.37;
        let carried = enemy.held || enemy.thrown;
        let target = if carried {
            f.moods.dizzy
        } else {
            f.moods.of(enemy.state)
        };
        let mood = faces.enemies[i].lerp(target, blend);
        faces.enemies[i] = mood;
        let pose = Pose::new(at - Vec2::Y * r, &mood, enemy.facing, now, phase);

        if carried {
            fill.circle(pose.apply(at), r + t.coat_width, 20, srgb(t.coat));
        }
        let body: Vec<Vec2> = enemy_body(enemy, at)
            .into_iter()
            .map(|p| pose.apply(p))
            .collect();
        let color = enemy_fill(enemy, t);
        fill.polygon(
            &face::grow(&body, f.outline_width),
            ink.apply(crate::kind_color(enemy.kind)),
        );
        fill.polygon(&body, color);
        stains.draw_enemy(&mut fill, i, at, r, enemy.facing, &pose, &tuning.stains);

        // Where it looks.
        let set = f.eyes(enemy.kind);
        let head = at + Vec2::Y * body_lift(enemy, r);
        let to_player = (player - head).normalize_or_zero();
        let gaze = match enemy.state {
            _ if carried => Vec2::from_angle(now * f.spin_hz * std::f32::consts::TAU + phase),
            State::Unaware => Vec2::new(enemy.facing * 0.5, (now * 0.5 + phase).sin() * 0.4),
            State::Searching => Vec2::new(
                (now * f.sweep_hz * std::f32::consts::TAU + phase).sin(),
                -0.1,
            ),
            State::Dizzy => Vec2::from_angle(now * f.spin_hz * std::f32::consts::TAU + phase),
            State::Attacking(AttackPhase::Recover) => Vec2::new(enemy.facing * 0.3, -0.8),
            State::Dodging => enemy
                .target
                .map(|p| (p * PIXELS_PER_METER - head).normalize_or_zero())
                .unwrap_or(to_player),
            _ => to_player,
        };
        let gaze = faces.gaze[i].lerp(gaze, blend);
        faces.gaze[i] = gaze;

        let prone = enemy.state == State::Prone;
        let attacking = matches!(
            enemy.state,
            State::Attacking(AttackPhase::Windup | AttackPhase::Strike)
        );
        let blinks = matches!(
            enemy.state,
            State::Unaware | State::Searching | State::Chasing | State::Defending | State::Fleeing
        );
        let shut = if blinks {
            face::blink(now, phase, f.blink_every, f.blink_time)
        } else {
            0.0
        };
        let (mut center, stalk) = if prone {
            (
                at - Vec2::Y * r * 0.5 + Vec2::X * enemy.facing * r * 0.3,
                false,
            )
        } else {
            (
                head + Vec2::new(set.offset.0 * enemy.facing, set.offset.1) * r,
                set.stalk > 0.0,
            )
        };
        if stalk {
            // The stalk sways with the body and sticks out of its top.
            let base = pose.apply(head + Vec2::Y * r * 0.6);
            let sway = (now * 1.3 + phase).sin() * r * 0.15 + mood.lean * enemy.facing * r;
            center = base + Vec2::new(set.offset.0 * enemy.facing * r + sway, set.stalk * r);
            let bulb = set.size.0.max(set.size.1) * r * mood.scale * 0.65;
            eyes.segment(base, center, bulb * 0.5, shade(color));
            eyes.circle(
                center,
                bulb + f.outline_width,
                14,
                shade(ink.apply(crate::kind_color(enemy.kind))),
            );
            eyes.circle(center, bulb, 14, shade(color));
        }
        let size = Vec2::new(set.size.0, set.size.1) * r * mood.scale;
        let iris = if attacking {
            srgb(f.danger)
        } else {
            srgb(f.glint)
        };
        for off in face::eye_offsets(set) {
            let local = Vec2::new(off.x * enemy.facing, off.y) * r;
            let pos = if stalk {
                center + local
            } else {
                pose.apply(center + local)
            };
            let side = if local.x.abs() > 1e-3 {
                local.x.signum()
            } else {
                -enemy.facing
            };
            let slant = mood.slant * side;
            if dark > 0.0 && !prone {
                face::draw_glow(
                    &mut eyes,
                    pos,
                    size.min_element() * f.dark.glow_radius,
                    iris,
                    f.dark.glow_alpha,
                );
            }
            face::draw_eye(
                &mut eyes,
                &Eye {
                    center: pos,
                    size,
                    open: mood.open * (1.0 - shut),
                    slant,
                    gaze,
                    pupil: set.pupil * size.min_element() * mood.pupil,
                    white: shade(srgb(f.socket)),
                    iris,
                    lid: shade(color),
                    crossed: prone,
                    x_width: f.x_width,
                    border: 0.0,
                    border_color: Color::NONE,
                },
            );
            if set.brow > 0.0 && !prone {
                let dir = Vec2::from_angle(slant);
                let at = pos + Vec2::Y * (size.y * 0.5 + set.brow * r * 0.6);
                eyes.segment(
                    at - dir * size.x * 0.7,
                    at + dir * size.x * 0.7,
                    set.brow * r,
                    shade(srgb(f.socket)),
                );
            }
        }

        // Shared marks: a ring bursting out on surprise, dots circling when dizzy.
        let mark = shade(srgb(f.socket));
        if enemy.state == State::Surprised && enemy.state_time < f.ping_time {
            let k = enemy.state_time / f.ping_time;
            face::ring(
                &mut eyes,
                pose.apply(at),
                r * (1.0 + f.ping_radius * k),
                f.ping_width * (1.0 - k),
                mark,
            );
        }
        if enemy.state == State::Dizzy || carried {
            let top = pose.apply(head + Vec2::Y * r) + Vec2::Y * 8.0;
            for k in 0..f.dizzy_dots {
                let a = now * f.dizzy_hz * std::f32::consts::TAU
                    + k as f32 * std::f32::consts::TAU / f.dizzy_dots as f32;
                eyes.circle(
                    top + Vec2::new(a.cos() * r * 0.6, a.sin() * r * 0.2),
                    f.dizzy_dot_radius,
                    8,
                    mark,
                );
            }
        }
    }

    let danger = srgb(t.danger);
    for shot in &sim.projectiles {
        fill.circle(
            shot.position * PIXELS_PER_METER,
            SPIT_RADIUS * PIXELS_PER_METER * t.spit_radius_scale,
            14,
            danger,
        );
    }

    let tar = srgb(tuning.tar.body);
    let shine = srgb(tuning.tar.highlight);
    let r = tuning.tar.pickup_radius;
    for pickup in &sim.pickups {
        let at = *pickup * PIXELS_PER_METER;
        fill.circle(at, r, 16, tar);
        fill.circle(at + Vec2::new(-0.3, 0.35) * r, r * 0.22, 8, shine);
    }

    blob_eyes(
        &mut eyes,
        &game,
        &latch,
        &tuning,
        &mut faces,
        alpha,
        now,
        time.delta_secs(),
        blend,
        dark,
    );

    let mut walls = Fill::default();
    stains.draw_walls(&mut walls, now, &tuning.stains);

    for (layer, mesh) in [
        (&actors.0, fill),
        (&eyes_layer.0, eyes),
        (&stains_layer.0, walls),
    ] {
        if let Some(mut handle) = meshes.get_mut(layer) {
            *handle = mesh.into_mesh();
        }
    }
}

/// The tar blob's eyes: they slosh on a spring, glance at enemies nearby,
/// squint when shrunk and pop wide after a pogo.
#[allow(clippy::too_many_arguments)]
fn blob_eyes(
    eyes: &mut Fill,
    game: &Game,
    latch: &InputLatch,
    tuning: &VisualTuning,
    faces: &mut Faces,
    alpha: f32,
    now: f32,
    dt: f32,
    blend: f32,
    dark: f32,
) {
    let sim = &game.sim;
    let b = &tuning.face.blob;
    let outline = game.blob_outline(alpha);
    if outline.len() < 3 {
        return;
    }
    let center = outline.iter().copied().sum::<Vec2>() / outline.len() as f32;
    // Radius of a disc with the outline's area.
    let area = outline
        .iter()
        .zip(outline.iter().cycle().skip(1))
        .map(|(a, b)| a.perp_dot(*b))
        .sum::<f32>()
        .abs()
        * 0.5;
    let radius = (area / std::f32::consts::PI).sqrt().max(1.0);
    let facing = sim.controller.facing();
    let home = center + Vec2::new(b.eyes.offset.0 * facing, b.eyes.offset.1) * radius;

    let hurt = sim.player_invulnerable() && !sim.controller.invulnerable();
    let target = if hurt {
        b.hurt
    } else if faces.wide_left > 0.0 {
        b.wide
    } else if sim.controller.is_shrunk() {
        b.shrunk
    } else {
        b.idle
    };

    let range = b.glance_range * PIXELS_PER_METER;
    let nearest = sim
        .enemies
        .iter()
        .filter(|e| e.alive() && !e.held)
        .map(|e| e.position * PIXELS_PER_METER - center)
        .filter(|d| d.length() <= range)
        .min_by(|a, b| a.length().total_cmp(&b.length()));
    let velocity = sim.blob.velocity();
    let gaze = if sim.controller.is_lunging() || sim.controller.is_slamming() {
        velocity.normalize_or_zero()
    } else if let Some(d) = nearest {
        d.normalize_or_zero()
    } else if latch.aim != Vec2::ZERO {
        latch.aim.normalize_or_zero()
    } else if velocity.length() > 1.0 {
        velocity.normalize_or_zero() * 0.6
    } else {
        Vec2::new(facing * 0.4, (now * 0.4).sin() * 0.3)
    };

    let mem = faces.blob.get_or_insert(BlobMemory {
        mood: b.idle,
        gaze: Vec2::ZERO,
        eyes: home,
        velocity: Vec2::ZERO,
    });
    mem.mood = mem.mood.lerp(target, blend);
    mem.gaze = mem.gaze.lerp(gaze, blend);
    // A spring pulls the eyes home; they lag behind sudden moves.
    if mem.eyes.distance(home) > radius * 3.0 {
        mem.eyes = home;
        mem.velocity = Vec2::ZERO;
    }
    mem.velocity += (home - mem.eyes) * b.slosh_stiffness * dt;
    mem.velocity *= (-b.slosh_damping * dt).exp();
    mem.eyes += mem.velocity * dt;
    let lag = (mem.eyes - home).clamp_length_max(b.slosh_max * radius);
    mem.eyes = home + lag;

    let mood = mem.mood;
    let shut = if target == b.idle {
        face::blink(now, 0.71, tuning.face.blink_every, tuning.face.blink_time)
    } else {
        0.0
    };
    let size = Vec2::new(b.eyes.size.0, b.eyes.size.1) * radius * mood.scale;
    let white = srgb(b.white);
    for off in face::eye_offsets(&b.eyes) {
        let local = Vec2::new(off.x * facing, off.y) * radius;
        let pos = mem.eyes + local + mem.gaze.clamp_length_max(1.0) * b.gaze_shift * radius;
        let side = if local.x.abs() > 1e-3 {
            local.x.signum()
        } else {
            -facing
        };
        if dark > 0.0 {
            face::draw_glow(
                eyes,
                pos,
                size.min_element() * tuning.face.dark.glow_radius,
                white,
                tuning.face.dark.glow_alpha,
            );
        }
        face::draw_eye(
            eyes,
            &Eye {
                center: pos,
                size,
                open: mood.open * (1.0 - shut),
                slant: mood.slant * side,
                gaze: mem.gaze,
                pupil: b.eyes.pupil * size.min_element() * mood.pupil,
                white,
                iris: srgb(b.pupil),
                lid: srgb(tuning.tar.body),
                crossed: sim.dead,
                x_width: tuning.face.x_width,
                border: b.border,
                border_color: srgb(b.border_color),
            },
        );
    }
}

/// How far a brute's body rises as it winds up its slam, px.
fn body_lift(enemy: &Enemy, r: f32) -> f32 {
    if enemy.kind == EnemyKind::Brute && enemy.state == State::Attacking(AttackPhase::Windup) {
        (enemy.state_time / tarborn_sim::enemy::BRUTE_WINDUP).min(1.0) * r * 0.4
    } else {
        0.0
    }
}

fn enemy_fill(enemy: &Enemy, t: &tuning::EnemyLook) -> Color {
    match enemy.state {
        _ if enemy.hit_cooldown > 0.0 => srgb(t.flash),
        State::Attacking(AttackPhase::Windup | AttackPhase::Strike) => srgb(t.danger),
        State::Attacking(AttackPhase::Recover) | State::Dizzy | State::Prone => srgb(t.open),
        _ => srgb(t.fill),
    }
}

/// The body outline the line look draws, as a filled polygon (world pixels).
fn enemy_body(enemy: &Enemy, at: Vec2) -> Vec<Vec2> {
    let r = enemy.radius * PIXELS_PER_METER;
    let facing = Vec2::X * enemy.facing;
    let rect = |c: Vec2, size: Vec2| {
        let h = size * 0.5;
        vec![
            c + Vec2::new(-h.x, -h.y),
            c + Vec2::new(h.x, -h.y),
            c + h,
            c + Vec2::new(-h.x, h.y),
        ]
    };
    let circle = |c: Vec2, r: f32| {
        (0..24)
            .map(|k| c + Vec2::from_angle(k as f32 / 24.0 * std::f32::consts::TAU) * r)
            .collect()
    };
    match (enemy.kind, enemy.state) {
        (_, State::Prone) => rect(at - Vec2::Y * r * 0.5, Vec2::new(r * 2.4, r)),
        (EnemyKind::Guard, _) => rect(at, Vec2::new(r * 1.6, r * 2.0)),
        (EnemyKind::Dummy | EnemyKind::Grub, _) => circle(at, r),
        (EnemyKind::Spitter, _) => [Vec2::X, Vec2::Y, Vec2::NEG_X, Vec2::NEG_Y]
            .iter()
            .map(|c| at + *c * r * 1.2)
            .collect(),
        (EnemyKind::Skitter, _) => {
            let tip = at + facing * r * 1.3;
            let up = at - facing * r + Vec2::Y * r;
            let down = at - facing * r - Vec2::Y * r;
            // Keep counter-clockwise whichever way it faces.
            if enemy.facing >= 0.0 {
                vec![down, tip, up]
            } else {
                vec![up, tip, down]
            }
        }
        (EnemyKind::Brute, _) => rect(at + Vec2::Y * body_lift(enemy, r), Vec2::splat(r * 2.0)),
    }
}

// ---------------------------------------------------------------- tar

#[derive(ShaderType, Clone, Debug, Default)]
pub struct TarUniform {
    points: [Vec4; MAX_TAR_POINTS / 2],
    count: u32,
    round: f32,
    bevel: f32,
    aa: f32,
    body: Vec4,
    highlight: Vec4,
    tint: Vec4,
    light: Vec4,
    strength: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, Default)]
pub struct TarMaterial {
    #[uniform(0)]
    tar: TarUniform,
}

impl Material2d for TarMaterial {
    fn fragment_shader() -> ShaderRef {
        TAR_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

#[allow(clippy::too_many_arguments)]
fn update_tar(
    game: Res<Game>,
    latch: Res<InputLatch>,
    tuning: Res<VisualTuning>,
    look: Res<Look>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    layer: Single<(&MeshMaterial2d<TarMaterial>, &mut Transform), With<TarLayer>>,
    mut materials: ResMut<Assets<TarMaterial>>,
) {
    if !look.filled() {
        return;
    }
    let (handle, mut transform) = layer.into_inner();
    let Some(mut material) = materials.get_mut(&handle.0) else {
        return;
    };
    let t = &tuning.tar;
    let outline = game.blob_outline(fixed.overstep_fraction());
    let count = outline.len().min(MAX_TAR_POINTS);
    let u = &mut material.tar;
    u.points = [Vec4::ZERO; MAX_TAR_POINTS / 2];
    for (i, p) in outline.iter().take(count).enumerate() {
        let v = &mut u.points[i / 2];
        if i % 2 == 0 {
            v.x = p.x;
            v.y = p.y;
        } else {
            v.z = p.x;
            v.w = p.y;
        }
    }
    u.count = count as u32;
    u.round = t.round;
    u.bevel = t.bevel;
    u.aa = t.aa;
    u.body = linear(t.body);
    u.highlight = linear(t.highlight);
    u.light = Vec4::new(t.light.0, t.light.1, t.light.2, t.spec_power);
    u.strength = Vec4::new(t.spec_strength, t.rim_strength, t.rim_power, t.tint_body);
    u.tint = match crate::tar_state_color(&game, &latch, time.elapsed_secs()) {
        Some(color) => {
            let l = color.to_linear();
            Vec4::new(l.red, l.green, l.blue, 1.0)
        }
        None => Vec4::ZERO,
    };

    // Cover the outline plus the rounding and a margin for soft edges.
    let (lo, hi) = outline.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), v| {
        (lo.min(*v), hi.max(*v))
    });
    let pad = t.round + t.aa + 2.0;
    let (lo, hi) = (lo - pad, hi + pad);
    transform.translation = ((lo + hi) * 0.5).extend(TAR_Z);
    transform.scale = (hi - lo).extend(1.0);
}

// ---------------------------------------------------------------- finish

/// The full-screen pass. Field order must match `Post` in post.wesl.
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType, Default)]
#[extract_app(RenderApp)]
pub struct PostEffect {
    p0: Vec4,
    p1: Vec4,
    p2: Vec4,
    p3: Vec4,
    danger: Vec4,
    time: f32,
    grain: f32,
    grain_fps: f32,
    vignette: f32,
    vignette_start: f32,
    vignette_end: f32,
    dither: f32,
    dither_pixel: f32,
    dither_spread: f32,
    red_keep: f32,
}

impl FullscreenMaterial for PostEffect {
    fn fragment_shader() -> ShaderRef {
        POST_SHADER.into()
    }

    fn schedule() -> impl bevy::ecs::schedule::ScheduleLabel + Clone {
        Core2d
    }

    fn schedule_configs(system: ScheduleConfigs<BoxedSystem>) -> ScheduleConfigs<BoxedSystem> {
        system
            .in_set(Core2dSystems::PostProcess)
            .before(bevy::core_pipeline::tonemapping::tonemapping)
    }
}

fn update_post(
    tuning: Res<VisualTuning>,
    look: Res<Look>,
    time: Res<Time>,
    mut effects: Query<&mut PostEffect>,
) {
    let p = &tuning.post;
    for mut effect in &mut effects {
        *effect = PostEffect {
            p0: linear(p.palette[0]),
            p1: linear(p.palette[1]),
            p2: linear(p.palette[2]),
            p3: linear(p.palette[3]),
            danger: linear(p.danger),
            time: time.elapsed_secs(),
            grain: p.grain,
            grain_fps: p.grain_fps,
            vignette: p.vignette,
            vignette_start: p.vignette_start,
            vignette_end: p.vignette_end,
            dither: if *look == Look::PitchDither { 1.0 } else { 0.0 },
            dither_pixel: p.dither_pixel,
            dither_spread: p.dither_spread,
            red_keep: p.red_keep,
        };
    }
}

//! The look: filled geometry, a glossy tar shader, generated level dressing
//! (see [`dressing`]) and a full-screen finish. V cycles between looks so they can be compared
//! in play. Every number comes from `visuals.ron` (see [`tuning`]).

mod dressing;
mod fill;
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
use tarborn_sim::{AttackPhase, Enemy, EnemyKind, Sim, State};

use crate::{Game, InputLatch, PIXELS_PER_METER};
use fill::Fill;
pub use tuning::VisualTuning;
use tuning::{linear, srgb};

const TAR_SHADER: &str = "embedded://tarborn/visuals/shaders/tar.wesl";
const POST_SHADER: &str = "embedded://tarborn/visuals/shaders/post.wesl";
/// The tar shader holds this many outline points.
const MAX_TAR_POINTS: usize = 32;

const LEVEL_Z: f32 = -10.0;
const ACTORS_Z: f32 = 1.0;
const TAR_Z: f32 = 2.0;

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
        .add_systems(Startup, spawn_layers)
        .add_systems(
            Update,
            (
                tuning::reload,
                cycle_look,
                apply_look,
                dressing::systems(),
                build_level,
                build_actors,
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

type AnyLayer = Or<(With<LevelLayer>, With<ActorsLayer>, With<TarLayer>)>;

#[derive(Component)]
struct LevelLayer;
#[derive(Component)]
struct ActorsLayer;
#[derive(Component)]
struct TarLayer;

fn spawn_layers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut colors: ResMut<Assets<ColorMaterial>>,
    mut tars: ResMut<Assets<TarMaterial>>,
) {
    let plain = colors.add(ColorMaterial::default());
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

fn build_actors(
    game: Res<Game>,
    tuning: Res<VisualTuning>,
    look: Res<Look>,
    fixed: Res<Time<Fixed>>,
    layer: Single<&Mesh2d, With<ActorsLayer>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if !look.filled() {
        return;
    }
    let alpha = fixed.overstep_fraction();
    let sim = &game.sim;
    let t = &tuning.enemy;
    let mut fill = Fill::default();

    for (i, enemy) in sim.enemies.iter().enumerate() {
        if !enemy.alive() {
            continue;
        }
        let at = game.enemy_position(i, alpha);
        if enemy.held || enemy.thrown {
            fill.circle(
                at,
                enemy.radius * PIXELS_PER_METER + t.coat_width,
                20,
                srgb(t.coat),
            );
        }
        let color = enemy_fill(enemy, t);
        fill.polygon(&enemy_body(enemy, at), color);
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

    if let Some(mut mesh) = meshes.get_mut(&layer.0) {
        *mesh = fill.into_mesh();
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
        (EnemyKind::Brute, _) => {
            let lift = if enemy.state == State::Attacking(AttackPhase::Windup) {
                (enemy.state_time / tarborn_sim::enemy::BRUTE_WINDUP).min(1.0) * r * 0.4
            } else {
                0.0
            };
            rect(at + Vec2::Y * lift, Vec2::splat(r * 2.0))
        }
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

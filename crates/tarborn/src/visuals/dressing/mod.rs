//! Generated level dressing, swappable per stage. The passes:
//!
//! 1. [`shape`] reads the level: a raster, a distance field, and the outline
//!    split into labelled runs (floor, slope, wall, ceiling, arcs) and corners.
//! 2. The body shader colors each solid by depth: trim band, seam, panels.
//! 3. Edge strips, corner pieces and the interior fill ([`build`]).
//! 4. Hanging parts, planned once and swayed every frame.
//! 5. A skyline of parallax layers behind the room.
//!
//! What the pieces are and every number about them is a [`set::DressingSet`]
//! in `assets/dressing/<name>.ron`; B cycles the sets, and edits are picked
//! up while the game runs.

mod build;
pub mod set;
pub mod shape;

use std::path::PathBuf;
use std::time::SystemTime;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::ScheduleSystem;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{Material2d, Material2dPlugin};

use super::Look;
use super::fill::Fill;
use super::tuning::{VisualTuning, linear, srgb};
use super::wfc::Rng;
use crate::{Game, PIXELS_PER_METER};
use set::DressingSet;
use shape::Shape;

const BODY_SHADER: &str = "embedded://tarborn/visuals/dressing/body.wesl";
const SKY_Z: f32 = -30.0;
const BODY_Z: f32 = -12.0;
const DETAIL_Z: f32 = -11.0;
const HANGING_Z: f32 = -9.0;
/// Seconds between checks for edited set files.
const POLL: f32 = 0.5;

pub struct DressingPlugin;

impl Plugin for DressingPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "body.wesl");
        app.add_plugins(Material2dPlugin::<BodyMaterial>::default())
            .init_resource::<Dressing>()
            .init_resource::<Built>()
            .add_systems(Startup, spawn);
    }
}

/// The dressing systems, in order; they run right after the look is applied.
pub fn systems() -> ScheduleConfigs<ScheduleSystem> {
    (reload_sets, cycle_set, rebuild, animate)
        .chain()
        .into_configs()
}

/// The loaded sets and which one is showing.
#[derive(Resource)]
pub struct Dressing {
    pub sets: Vec<DressingSet>,
    pub current: usize,
    seen: Vec<Option<SystemTime>>,
    /// The tuning's default set when it was last applied.
    default: Option<String>,
}

impl Default for Dressing {
    fn default() -> Self {
        let sets: Vec<DressingSet> = set::BUILT_IN
            .iter()
            .map(|(name, text)| {
                ron::from_str(text).unwrap_or_else(|err| panic!("built-in dressing {name}: {err}"))
            })
            .collect();
        Self {
            seen: vec![None; sets.len()],
            sets,
            current: 0,
            default: None,
        }
    }
}

impl Dressing {
    pub fn set(&self) -> &DressingSet {
        &self.sets[self.current]
    }
}

fn set_path(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/dressing"))
        .join(format!("{name}.ron"))
}

/// Re-reads set files that changed. A broken edit is reported and the last
/// good set stays.
fn reload_sets(
    time: Res<Time>,
    tuning: Res<VisualTuning>,
    mut dressing: ResMut<Dressing>,
    mut since: Local<f32>,
) {
    let wanted = &tuning.dressing.default_set;
    if dressing.default.as_ref() != Some(wanted) {
        dressing.default = Some(wanted.clone());
        match set::BUILT_IN.iter().position(|(name, _)| name == wanted) {
            Some(i) => dressing.current = i,
            None => warn!("dressing set {wanted:?} not found"),
        }
    }
    *since += time.delta_secs();
    if *since < POLL {
        return;
    }
    *since = 0.0;
    for (i, (name, _)) in set::BUILT_IN.iter().enumerate() {
        let path = set_path(name);
        let Ok(modified) = std::fs::metadata(&path).and_then(|m| m.modified()) else {
            continue;
        };
        if dressing.seen[i] == Some(modified) {
            continue;
        }
        let first = dressing.seen[i].is_none();
        dressing.seen[i] = Some(modified);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => {
                warn!("{name}.ron: {err}");
                continue;
            }
        };
        match ron::from_str::<DressingSet>(&text) {
            Ok(new) if new != dressing.sets[i] => {
                if !first {
                    info!("dressing {name}.ron reloaded");
                }
                dressing.sets[i] = new;
            }
            Ok(_) => {}
            Err(err) => warn!("{name}.ron: {err}"),
        }
    }
}

fn cycle_set(keys: Res<ButtonInput<KeyCode>>, mut dressing: ResMut<Dressing>) {
    if keys.just_pressed(KeyCode::KeyB) {
        dressing.current = (dressing.current + 1) % dressing.sets.len();
        info!("dressing: {}", dressing.set().name);
    }
}

// ---------------------------------------------------------------- entities

type AnyDressing = Or<(
    With<BodyLayer>,
    With<DetailLayer>,
    With<HangingLayer>,
    With<SkyLayer>,
)>;

#[derive(Component)]
struct BodyLayer;
#[derive(Component)]
struct DetailLayer;
#[derive(Component)]
struct HangingLayer;
/// One skyline layer; it follows the camera by this fraction.
#[derive(Component)]
struct SkyLayer {
    parallax: f32,
}

fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut colors: ResMut<Assets<ColorMaterial>>,
    mut bodies: ResMut<Assets<BodyMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let plain = colors.add(ColorMaterial::default());
    commands.spawn((
        BodyLayer,
        Mesh2d(meshes.add(Fill::default().into_mesh())),
        MeshMaterial2d(bodies.add(BodyMaterial {
            body: BodyUniform::default(),
            depth: images.add(depth_image(&[0.0], 1, 1, 1.0)),
        })),
        Transform::from_xyz(0.0, 0.0, BODY_Z),
    ));
    commands.spawn((
        DetailLayer,
        Mesh2d(meshes.add(Fill::default().into_mesh())),
        MeshMaterial2d(plain.clone()),
        Transform::from_xyz(0.0, 0.0, DETAIL_Z),
    ));
    commands.spawn((
        HangingLayer,
        Mesh2d(meshes.add(Fill::default().into_mesh())),
        MeshMaterial2d(plain),
        Transform::from_xyz(0.0, 0.0, HANGING_Z),
    ));
}

/// What the dressing was last built from, and the parts kept between frames.
#[derive(Resource, Default)]
struct Built {
    key: Option<(Vec<Vec<Vec2>>, DressingSet, VisualTuning)>,
    shape: Option<Shape>,
    hangers: Vec<build::Hanger>,
}

fn solids_px(game: &Game) -> Vec<Vec<Vec2>> {
    game.sim
        .level
        .solids
        .iter()
        .map(|s| s.vertices.iter().map(|v| *v * PIXELS_PER_METER).collect())
        .collect()
}

/// Rebuilds everything that only changes with the level, the set or the tuning.
#[allow(clippy::too_many_arguments)]
fn rebuild(
    game: Res<Game>,
    tuning: Res<VisualTuning>,
    dressing: Res<Dressing>,
    look: Res<Look>,
    mut clear: ResMut<ClearColor>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut colors: ResMut<Assets<ColorMaterial>>,
    mut bodies: ResMut<Assets<BodyMaterial>>,
    mut images: ResMut<Assets<Image>>,
    body: Single<(&Mesh2d, &MeshMaterial2d<BodyMaterial>), With<BodyLayer>>,
    detail: Single<&Mesh2d, (With<DetailLayer>, Without<BodyLayer>)>,
    sky: Query<Entity, With<SkyLayer>>,
    mut layers: Query<&mut Visibility, AnyDressing>,
    mut built: ResMut<Built>,
) {
    if look.is_changed() {
        let shown = if look.filled() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        for mut visibility in &mut layers {
            *visibility = shown;
        }
    }
    if !look.filled() {
        return;
    }
    let set = dressing.set();
    if look.is_changed() || dressing.is_changed() {
        clear.0 = srgb(set.background);
    }
    let solids = solids_px(&game);
    let unchanged = built
        .key
        .as_ref()
        .is_some_and(|(s, d, t)| *s == solids && d == set && *t == *tuning);
    if unchanged {
        return;
    }
    let d = &tuning.dressing;
    let shape = Shape::read(&solids, &d.shape);
    let seed = d.seed;

    // Body: the solids, colored by depth in the shader.
    let (body_mesh, body_material) = *body;
    let mut fill = Fill::default();
    for poly in &solids {
        fill.polygon(poly, Color::WHITE);
    }
    // Everything outside the level is solid too, out past any zoom.
    let (lo, hi) = solids
        .iter()
        .flatten()
        .fold((Vec2::MAX, Vec2::MIN), |(lo, hi), v| {
            (lo.min(*v), hi.max(*v))
        });
    let far = d.surround;
    for (a, b) in [
        (
            Vec2::new(lo.x - far, lo.y - far),
            Vec2::new(hi.x + far, lo.y),
        ),
        (
            Vec2::new(lo.x - far, hi.y),
            Vec2::new(hi.x + far, hi.y + far),
        ),
        (Vec2::new(lo.x - far, lo.y), Vec2::new(lo.x, hi.y)),
        (Vec2::new(hi.x, lo.y), Vec2::new(hi.x + far, hi.y)),
    ] {
        fill.rect((a + b) * 0.5, b - a, Color::WHITE);
    }
    if let Some(mut mesh) = meshes.get_mut(&body_mesh.0) {
        *mesh = fill.into_mesh();
    }
    if let Some(mut material) = bodies.get_mut(&body_material.0) {
        let b = &set.body;
        let size = Vec2::new(shape.width as f32, shape.height as f32) * shape.cell;
        material.body = BodyUniform {
            area: Vec4::new(shape.origin.x, shape.origin.y, size.x, size.y),
            fill: linear(b.fill),
            trim: linear(b.trim),
            seam: linear(b.seam),
            band: Vec4::new(b.trim_depth, b.seam_width, b.wobble, b.wobble_scale),
            panel: Vec4::new(b.panel.0, b.panel.1, b.panel_variation, b.grain),
            shade: Vec4::new(b.grain_scale, b.depth_darken, b.depth_range, d.depth_max),
        };
        material.depth = images.add(depth_image(
            &shape.depth,
            shape.width,
            shape.height,
            d.depth_max,
        ));
    }

    // Details: strips, corners, interior.
    let mut rng = Rng::new(seed);
    let mut fill = Fill::default();
    build::interior(&shape, set, &mut rng, &mut fill);
    build::strips(&shape, set, &mut rng, &mut fill);
    build::corners(&shape, &solids, set, &mut rng, &mut fill);
    if let Some(mut mesh) = meshes.get_mut(&detail.0) {
        *mesh = fill.into_mesh();
    }

    // Skyline: one entity per layer, back to front.
    for entity in &sky {
        commands.entity(entity).despawn();
    }
    let plain = colors.add(ColorMaterial::default());
    for (i, layer) in set.skyline.layers.iter().enumerate() {
        let mut rng = Rng::new(seed.wrapping_add(101 * (i as u64 + 1)));
        let fill = build::skyline(&shape, layer, &mut rng);
        commands.spawn((
            SkyLayer {
                parallax: layer.parallax,
            },
            Mesh2d(meshes.add(fill.into_mesh())),
            MeshMaterial2d(plain.clone()),
            Transform::from_xyz(0.0, 0.0, SKY_Z + i as f32 * 0.5),
        ));
    }

    let mut rng = Rng::new(seed ^ 0x4a4e);
    built.hangers = build::plan_hanging(&shape, set, &mut rng);
    built.shape = Some(shape);
    built.key = Some((solids, set.clone(), tuning.clone()));
}

/// Sways the hanging parts and slides the skyline layers with the camera.
#[allow(clippy::too_many_arguments)]
fn animate(
    time: Res<Time>,
    look: Res<Look>,
    tuning: Res<VisualTuning>,
    dressing: Res<Dressing>,
    built: Res<Built>,
    mut meshes: ResMut<Assets<Mesh>>,
    hanging: Single<&Mesh2d, With<HangingLayer>>,
    camera: Single<&Transform, With<Camera2d>>,
    mut sky: Query<(&SkyLayer, &mut Transform), Without<Camera2d>>,
) {
    if !look.filled() {
        return;
    }
    let Some(shape) = &built.shape else {
        return;
    };
    let mut fill = Fill::default();
    build::draw_hanging(
        &built.hangers,
        shape,
        dressing.set(),
        time.elapsed_secs(),
        tuning.dressing.seed,
        &mut fill,
    );
    if let Some(mut mesh) = meshes.get_mut(&hanging.0) {
        *mesh = fill.into_mesh();
    }
    let middle = (shape.open_min + shape.open_max) * 0.5;
    let offset = camera.translation.truncate() - middle;
    for (layer, mut transform) in &mut sky {
        let at = offset * layer.parallax;
        transform.translation.x = at.x;
        transform.translation.y = at.y;
    }
}

// ---------------------------------------------------------------- body

/// Field order must match `Body` in body.wesl.
#[derive(ShaderType, Clone, Debug, Default)]
pub struct BodyUniform {
    /// xy = raster origin, zw = raster size (world px).
    area: Vec4,
    fill: Vec4,
    trim: Vec4,
    seam: Vec4,
    /// trim depth, seam width, wobble, wobble scale.
    band: Vec4,
    /// panel width, panel height, panel variation, grain.
    panel: Vec4,
    /// grain scale, depth darken, depth range, depth max.
    shade: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct BodyMaterial {
    #[uniform(0)]
    body: BodyUniform,
    #[texture(1)]
    #[sampler(2)]
    depth: Handle<Image>,
}

impl Material2d for BodyMaterial {
    fn fragment_shader() -> ShaderRef {
        BODY_SHADER.into()
    }
}

/// The distance field as a one-channel texture, depth / `max` per texel, with
/// the first row at the top as images expect.
fn depth_image(depth: &[f32], width: usize, height: usize, max: f32) -> Image {
    let mut data = vec![0u8; width * height];
    for y in 0..height {
        let row = height - 1 - y;
        for x in 0..width {
            let d = depth[y * width + x] / max.max(1.0);
            data[row * width + x] = (d.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape::EdgeClass;

    fn room_px(level: &tarborn_sim::level::Level) -> Vec<Vec<Vec2>> {
        level
            .solids
            .iter()
            .map(|s| s.vertices.iter().map(|v| *v * PIXELS_PER_METER).collect())
            .collect()
    }

    #[test]
    fn the_dressing_room_reads_every_edge_class() {
        let tuning = VisualTuning::default();
        let solids = room_px(&tarborn_sim::level::Level::dressing_room());
        let shape = Shape::read(&solids, &tuning.dressing.shape);
        for class in [
            EdgeClass::Floor,
            EdgeClass::Slope,
            EdgeClass::Wall,
            EdgeClass::Ceiling,
            EdgeClass::ArcConcave,
            EdgeClass::ArcConvex,
        ] {
            assert!(
                shape.runs.iter().any(|r| r.class == class),
                "no {class:?} run"
            );
        }
        assert!(
            shape
                .corners
                .iter()
                .any(|c| c.kind == shape::CornerKind::Inner)
        );
        assert!(
            shape
                .corners
                .iter()
                .any(|c| c.kind == shape::CornerKind::Outer)
        );
    }

    #[test]
    fn every_set_dresses_every_room() {
        let tuning = VisualTuning::default();
        let dressing = Dressing::default();
        for level in [
            tarborn_sim::level::Level::dressing_room(),
            tarborn_sim::Sim::arena_room().level,
            tarborn_sim::Sim::training_room(Vec2::new(0.0, 2.0)).level,
        ] {
            let solids = room_px(&level);
            let shape = Shape::read(&solids, &tuning.dressing.shape);
            for set in &dressing.sets {
                let mut rng = Rng::new(1);
                let mut fill = Fill::default();
                build::interior(&shape, set, &mut rng, &mut fill);
                build::strips(&shape, set, &mut rng, &mut fill);
                build::corners(&shape, &solids, set, &mut rng, &mut fill);
                let hangers = build::plan_hanging(&shape, set, &mut rng);
                assert!(!hangers.is_empty(), "{}: nothing hangs", set.name);
                build::draw_hanging(&hangers, &shape, set, 1.0, 1, &mut fill);
                for layer in &set.skyline.layers {
                    let _ = build::skyline(&shape, layer, &mut rng);
                }
            }
        }
    }
}

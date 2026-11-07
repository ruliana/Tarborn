//! Chunks: self-contained pieces of level stored as data.
//!
//! A chunk holds convex solids, hazards, enemy spawn slots, reward slots and
//! typed sockets on its border, plus a score card for every route between
//! two sockets that the bots proved passable. It is one RON file in
//! `assets/chunks/<set>/<id>.ron`. Hand-made rooms (the training room, the
//! arena) are chunks too: they just have a spawn point and no sockets.

use std::fmt;
use std::path::Path;

use glam::Vec2;
use serde::{Deserialize, Serialize};
use tarborn_sim::arena::{Arena, Drop};
use tarborn_sim::{Enemy, EnemyKind, Level, Sim, Solid, State};

/// The built-in chunk parameters, used when the file on disk can't be read.
const BUILT_IN_PARAMS: &str = include_str!("../../tarborn/assets/pcg/chunk.ron");

/// Tunables for chunk geometry, from `assets/pcg/chunk.ron`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChunkParams {
    /// Grid cell size in metres. Chunk bounds and socket centres snap to it
    /// so assembly can line chunks up.
    pub cell: f32,
    /// The narrowest socket, in metres. The blob has to fit through at full
    /// size, since there is no squeeze form.
    pub min_socket_width: f32,
    /// How far a value may sit off the grid and still count as snapped.
    pub snap_epsilon: f32,
    /// How many points along a socket's opening are checked to be free of solids.
    pub socket_clear_samples: usize,
    /// How far inside the chunk the socket opening is checked, in metres.
    pub socket_clear_inset: f32,
}

impl Default for ChunkParams {
    fn default() -> Self {
        ron::from_str(BUILT_IN_PARAMS).expect("built-in assets/pcg/chunk.ron is valid")
    }
}

impl ChunkParams {
    /// Reads the parameters from a RON file.
    pub fn load(path: &Path) -> Result<Self, ChunkError> {
        let text = std::fs::read_to_string(path).map_err(|e| ChunkError::Io(e.to_string()))?;
        ron::from_str(&text).map_err(|e| ChunkError::Parse(e.to_string()))
    }
}

/// A convex polygon, written either as an axis-aligned box or as vertices.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    /// Minimum and maximum corners.
    Rect(Vec2, Vec2),
    /// Vertices in either winding; the polygon must be convex.
    Poly(Vec<Vec2>),
}

impl Shape {
    /// The simulation solid. Panics on a degenerate polygon; run
    /// [`Chunk::validate`] first on untrusted data.
    pub fn solid(&self) -> Solid {
        match self {
            Shape::Rect(min, max) => Solid::rect(*min, *max),
            Shape::Poly(vertices) => Solid::convex(vertices.clone()),
        }
    }

    /// The most compact shape that builds exactly `solid`.
    pub fn from_solid(solid: &Solid) -> Self {
        let v = &solid.vertices;
        if v.len() == 4 {
            let (min, max) = (v[0], v[2]);
            if min.x < max.x && min.y < max.y && Solid::rect(min, max) == *solid {
                return Shape::Rect(min, max);
            }
        }
        Shape::Poly(v.clone())
    }

    fn vertices(&self) -> Vec<Vec2> {
        match self {
            Shape::Rect(min, max) => Solid::rect(*min, *max).vertices,
            Shape::Poly(vertices) => vertices.clone(),
        }
    }
}

/// Which border of the chunk a socket sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

/// How the blob passes through a socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SocketKind {
    /// Rolling along a floor.
    Walk,
    /// Falling through an opening.
    Drop,
    /// Stuck to a wall or ceiling.
    Climb,
}

/// An opening on the chunk's border.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Socket {
    pub side: Side,
    /// The opening's centre along its side: y on the left and right, x on
    /// the top and bottom.
    pub at: f32,
    pub width: f32,
    pub kind: SocketKind,
}

impl Socket {
    /// The two ends of the opening, on the chunk border.
    pub fn ends(&self, bounds: (Vec2, Vec2)) -> (Vec2, Vec2) {
        let (min, max) = bounds;
        let half = self.width * 0.5;
        let v = Vec2::new;
        match self.side {
            Side::Left => (v(min.x, self.at - half), v(min.x, self.at + half)),
            Side::Right => (v(max.x, self.at - half), v(max.x, self.at + half)),
            Side::Bottom => (v(self.at - half, min.y), v(self.at + half, min.y)),
            Side::Top => (v(self.at - half, max.y), v(self.at + half, max.y)),
        }
    }

    /// The region just inside the opening, `depth` metres deep: the blob's
    /// centre in it has passed through the socket.
    pub fn region(&self, bounds: (Vec2, Vec2), depth: f32) -> (Vec2, Vec2) {
        let (a, b) = self.ends(bounds);
        let mid = (a + b) * 0.5 + self.inward() * depth * 0.5;
        let along = (b - a).abs() * 0.5;
        let across = self.inward().abs() * depth * 0.5;
        (mid, along + across)
    }

    /// A wall just outside the opening that closes it, `thickness` metres
    /// thick and overlapping the border that far at each end. A stage seals
    /// the sockets nothing joins onto, so the blob can't leave the level.
    pub fn plug(&self, bounds: (Vec2, Vec2), thickness: f32) -> Shape {
        let (a, b) = self.ends(bounds);
        let along = (b - a).normalize_or_zero() * thickness;
        let out = -self.inward() * thickness;
        let (p, q) = (a - along, b + along + out);
        Shape::Rect(p.min(q), p.max(q))
    }

    /// Unit vector pointing from the border into the chunk.
    pub fn inward(&self) -> Vec2 {
        match self.side {
            Side::Left => Vec2::X,
            Side::Right => Vec2::NEG_X,
            Side::Bottom => Vec2::Y,
            Side::Top => Vec2::NEG_Y,
        }
    }
}

/// An enemy placed in a chunk.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnemySlot {
    pub kind: EnemyKind,
    pub at: Vec2,
    /// 1 faces right, -1 faces left.
    #[serde(default = "right")]
    pub facing: f32,
    /// The state it starts in.
    #[serde(default = "unaware")]
    pub state: State,
}

fn right() -> f32 {
    1.0
}

fn unaware() -> State {
    State::Unaware
}

impl EnemySlot {
    pub fn enemy(&self) -> Enemy {
        let mut enemy = Enemy::new(self.kind, self.at);
        enemy.facing = self.facing;
        enemy.state = self.state;
        enemy
    }
}

/// What the bots measured on one route. Filled in by the metrics step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Scores {
    pub diversity: f32,
    pub complexity: f32,
    pub difficulty: f32,
    pub novelty: f32,
    /// Ticks taken by the easiest solution.
    pub length: f32,
}

/// A route the bots proved passable, from one socket to another. Routes are
/// directional: a climb can be hard upward and trivial downward.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Route {
    /// Index into the chunk's sockets.
    pub from: usize,
    pub to: usize,
    #[serde(default)]
    pub scores: Option<Scores>,
}

/// Where a chunk came from.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub seed: u64,
    /// The grammar derivation it was realised from, if generated.
    #[serde(default)]
    pub derivation: String,
    /// Which generator, and which version of it, wrote the chunk.
    #[serde(default)]
    pub generator: String,
    /// The dressing set it suits, such as `industrial` or `forest`.
    #[serde(default)]
    pub set: Option<String>,
}

/// A self-contained piece of level.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    #[serde(default)]
    pub meta: Meta,
    /// The chunk's extent: minimum and maximum corners.
    pub bounds: (Vec2, Vec2),
    pub solids: Vec<Shape>,
    /// Regions that hurt on touch, such as spikes.
    #[serde(default)]
    pub hazards: Vec<Shape>,
    /// Tar drops that heal.
    #[serde(default)]
    pub pickups: Vec<Vec2>,
    #[serde(default)]
    pub enemies: Vec<EnemySlot>,
    #[serde(default)]
    pub sockets: Vec<Socket>,
    /// Points where assembly may place a reward.
    #[serde(default)]
    pub reward_slots: Vec<Vec2>,
    #[serde(default)]
    pub routes: Vec<Route>,
    /// Where the blob starts, for a room played on its own.
    #[serde(default)]
    pub spawn: Option<Vec2>,
    /// Half the screen around the player, in metres, if not the default.
    #[serde(default)]
    pub view: Option<Vec2>,
    /// Waves of enemies dropping in, for an arena. Facing and state are
    /// ignored: wave enemies face the middle and start unaware.
    #[serde(default)]
    pub waves: Vec<Vec<EnemySlot>>,
}

/// Why a chunk could not be loaded or used.
#[derive(Clone, Debug, PartialEq)]
pub enum ChunkError {
    Io(String),
    Parse(String),
    Invalid(Vec<String>),
}

impl fmt::Display for ChunkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChunkError::Io(e) => write!(f, "can't read chunk: {e}"),
            ChunkError::Parse(e) => write!(f, "can't parse chunk: {e}"),
            ChunkError::Invalid(problems) => write!(f, "invalid chunk: {}", problems.join("; ")),
        }
    }
}

impl std::error::Error for ChunkError {}

impl Chunk {
    /// Parses a chunk from RON, without validating it.
    pub fn from_ron(text: &str) -> Result<Self, ChunkError> {
        ron::from_str(text).map_err(|e| ChunkError::Parse(e.to_string()))
    }

    /// Reads, parses and validates a chunk file.
    pub fn load(path: &Path, params: &ChunkParams) -> Result<Self, ChunkError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| ChunkError::Io(format!("{}: {e}", path.display())))?;
        let chunk = Self::from_ron(&text)?;
        chunk.validate(params)?;
        Ok(chunk)
    }

    /// Pretty RON, as written to `assets/chunks/`.
    pub fn to_ron(&self) -> String {
        let config = ron::ser::PrettyConfig::new()
            .depth_limit(3)
            .separate_tuple_members(false)
            .struct_names(false);
        let mut text = ron::ser::to_string_pretty(self, config).expect("chunks serialise");
        text.push('\n');
        text
    }

    /// Checks everything the loader and assembly rely on, and lists every
    /// problem found.
    pub fn validate(&self, params: &ChunkParams) -> Result<(), ChunkError> {
        let mut problems = Vec::new();
        let (min, max) = self.bounds;
        if !(min.x < max.x && min.y < max.y) {
            problems.push(format!("bounds {min} to {max} are empty"));
        }
        for (what, shapes) in [("solid", &self.solids), ("hazard", &self.hazards)] {
            for (i, shape) in shapes.iter().enumerate() {
                if let Err(e) = check_convex(&shape.vertices()) {
                    problems.push(format!("{what} {i}: {e}"));
                }
            }
        }
        // Only the solids that are valid can be used for the checks below.
        let solids: Vec<Solid> = self
            .solids
            .iter()
            .filter(|s| check_convex(&s.vertices()).is_ok())
            .map(Shape::solid)
            .collect();
        let inside_solid = |p: Vec2| solids.iter().any(|s| s.penetration(p, 0.0).is_some());
        let in_bounds = |p: Vec2| p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y;

        // Spawn, pickups and reward slots must be in open space. Enemies only
        // need to be inside the chunk: their bodies settle onto the ground
        // themselves (the training room's dummy starts in the slope).
        let mut points: Vec<(String, Vec2, bool)> = Vec::new();
        points.extend(self.spawn.map(|p| ("spawn".to_string(), p, true)));
        points.extend(
            self.pickups
                .iter()
                .enumerate()
                .map(|(i, &p)| (format!("pickup {i}"), p, true)),
        );
        points.extend(
            self.reward_slots
                .iter()
                .enumerate()
                .map(|(i, &p)| (format!("reward slot {i}"), p, true)),
        );
        points.extend(
            self.enemies
                .iter()
                .enumerate()
                .map(|(i, e)| (format!("enemy {i}"), e.at, false)),
        );
        for (w, wave) in self.waves.iter().enumerate() {
            points.extend(
                wave.iter()
                    .enumerate()
                    .map(|(i, e)| (format!("wave {w} enemy {i}"), e.at, false)),
            );
        }
        for (what, p, open) in points {
            if !in_bounds(p) {
                problems.push(format!("{what} at {p} is outside the bounds"));
            } else if open && inside_solid(p) {
                problems.push(format!("{what} at {p} is inside a solid"));
            }
        }

        let snapped = |x: f32| {
            let cells = x / params.cell;
            (cells - cells.round()).abs() * params.cell <= params.snap_epsilon
        };
        if !self.sockets.is_empty() && ![min.x, min.y, max.x, max.y].into_iter().all(snapped) {
            problems.push(format!(
                "bounds {min} to {max} are off the {} m grid",
                params.cell
            ));
        }
        for (i, socket) in self.sockets.iter().enumerate() {
            if socket.width < params.min_socket_width {
                problems.push(format!(
                    "socket {i} is {} m wide, narrower than {} m",
                    socket.width, params.min_socket_width
                ));
            }
            if !snapped(socket.at) {
                problems.push(format!("socket {i} at {} is off the grid", socket.at));
            }
            let (a, b) = socket.ends(self.bounds);
            if !in_bounds(a) || !in_bounds(b) {
                problems.push(format!("socket {i} runs past the chunk's corner"));
                continue;
            }
            let samples = params.socket_clear_samples.max(2);
            let blocked = (0..samples).any(|k| {
                let t = k as f32 / (samples - 1) as f32;
                inside_solid(a.lerp(b, t) + socket.inward() * params.socket_clear_inset)
            });
            if blocked {
                problems.push(format!("socket {i} is blocked by a solid"));
            }
        }
        for (i, route) in self.routes.iter().enumerate() {
            let n = self.sockets.len();
            if route.from >= n || route.to >= n {
                problems.push(format!("route {i} names a socket that doesn't exist"));
            } else if route.from == route.to {
                problems.push(format!("route {i} starts and ends at the same socket"));
            }
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(ChunkError::Invalid(problems))
        }
    }

    /// The static geometry.
    pub fn level(&self) -> Level {
        Level {
            solids: self.solids.iter().map(Shape::solid).collect(),
        }
    }

    /// This chunk with every socket closed by a [`Socket::plug`], played as a
    /// stage on its own.
    pub fn sealed(&self, thickness: f32) -> Chunk {
        let mut chunk = self.clone();
        chunk.solids.extend(
            self.sockets
                .iter()
                .map(|socket| socket.plug(self.bounds, thickness)),
        );
        chunk
    }

    /// A simulation of this chunk played on its own, starting at its spawn
    /// point (or the middle of its first socket, or its centre).
    pub fn sim(&self) -> Sim {
        let spawn = self.spawn.unwrap_or_else(|| match self.sockets.first() {
            Some(socket) => {
                let (a, b) = socket.ends(self.bounds);
                (a + b) * 0.5 + socket.inward()
            }
            None => (self.bounds.0 + self.bounds.1) * 0.5,
        });
        self.sim_at(spawn)
    }

    /// A simulation of this chunk with the blob starting at `spawn`.
    pub fn sim_at(&self, spawn: Vec2) -> Sim {
        let mut sim = Sim::new(self.level(), spawn);
        sim.enemies = self.enemies.iter().map(EnemySlot::enemy).collect();
        sim.hazards = self.hazards.iter().map(Shape::solid).collect();
        sim.pickups = self.pickups.clone();
        if let Some(view) = self.view {
            sim.view = view;
        }
        if !self.waves.is_empty() {
            let waves = self
                .waves
                .iter()
                .map(|wave| {
                    wave.iter()
                        .map(|e| Drop {
                            kind: e.kind,
                            at: e.at,
                        })
                        .collect()
                })
                .collect();
            sim.arena = Some(Arena::new(waves));
        }
        sim
    }

    /// Captures a hand-built room as a chunk: everything [`Chunk::sim_at`]
    /// needs to rebuild it.
    pub fn from_sim(id: &str, sim: &Sim, spawn: Vec2, bounds: (Vec2, Vec2)) -> Self {
        let slot = |e: &Enemy| EnemySlot {
            kind: e.kind,
            at: e.position,
            facing: e.facing,
            state: e.state,
        };
        let waves = sim
            .arena
            .as_ref()
            .map(|arena| {
                arena
                    .waves
                    .iter()
                    .map(|wave| {
                        wave.iter()
                            .map(|d| EnemySlot {
                                kind: d.kind,
                                at: d.at,
                                facing: 1.0,
                                state: State::Unaware,
                            })
                            .collect()
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            id: id.to_string(),
            meta: Meta {
                generator: "hand-built".to_string(),
                ..Meta::default()
            },
            bounds,
            solids: sim.level.solids.iter().map(Shape::from_solid).collect(),
            hazards: sim.hazards.iter().map(Shape::from_solid).collect(),
            pickups: sim.pickups.clone(),
            enemies: sim.enemies.iter().map(slot).collect(),
            sockets: Vec::new(),
            reward_slots: Vec::new(),
            routes: Vec::new(),
            spawn: Some(spawn),
            view: (sim.view != tarborn_sim::TRAINING_VIEW).then_some(sim.view),
            waves,
        }
    }
}

/// The smallest box around every solid.
pub fn solid_bounds(level: &Level) -> (Vec2, Vec2) {
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for v in level.solids.iter().flat_map(|s| &s.vertices) {
        min = min.min(*v);
        max = max.max(*v);
    }
    (min, max)
}

fn check_convex(vertices: &[Vec2]) -> Result<(), String> {
    let n = vertices.len();
    if n < 3 {
        return Err(format!("{n} vertices, needs at least three"));
    }
    let area: f32 = (0..n)
        .map(|i| vertices[i].perp_dot(vertices[(i + 1) % n]))
        .sum::<f32>()
        * 0.5;
    if area.abs() < 1e-6 {
        return Err("no area".to_string());
    }
    let sign = area.signum();
    for i in 0..n {
        let (a, b, c) = (vertices[i], vertices[(i + 1) % n], vertices[(i + 2) % n]);
        if (b - a).perp_dot(c - b) * sign < -1e-5 {
            return Err(format!("not convex at vertex {}", (i + 1) % n));
        }
    }
    Ok(())
}

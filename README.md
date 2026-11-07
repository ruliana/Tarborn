# Tarborn

A 2D platformer about a blob of living tar: a spiritual successor to Gish's
soft-body physics, with the snappy, forgiving controls of a modern action
platformer.

## Stack

- **Rust** (stable, 1.97.1 or newer) and **Bevy 0.20**.
- The tar blob is a custom position-based soft body, not an engine physics
  body. It lives in its own crate with no Bevy dependency, so it is
  deterministic and unit tested headlessly.

```
crates/
  tarborn_sim/   Simulation core: blob solver, level collision, controls feel.
                 Pure Rust + glam. All gameplay rules live here.
  tarborn_sfx/   Procedural sound synthesis (sfxr-style, layered). No engine.
  tarborn_pcg/   Level generation: chunk files, bots, metrics, generators.
                 Depends only on tarborn_sim, never on Bevy.
  tarborn/       The Bevy game: input, fixed 120 Hz tick, rendering, sound.
```

## Running

```sh
cargo run -p tarborn                 # play
cargo run -p tarborn --features dev  # faster rebuilds (dynamic linking)
cargo test -p tarborn_sim            # physics and feel tests, runs in well under a second
```

On Linux you need the usual Bevy system packages first:

```sh
sudo apt install libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev pkg-config
```

## Camera tuning

The camera (look-ahead, zoom out at speed, zoom in while gripping, screen
shake on impacts) is tuned in [`camera.ron`](camera.ron) at the repo root.
The game reloads it while running: edit, save, and the change shows within
half a second. A typo keeps the last good tuning and logs the error. Set
`TARBORN_CAMERA=path/to/other.ron` to try an alternate tuning.

## Level dressing

The filled looks dress the level from its shape alone, so any room gets
dressed without hand placement. The game reads the solids (a raster, a
distance field, and the outline labelled as floor, slope, wall, ceiling,
concave or convex arc, and outer or inner corners), then lays pieces on
it: a body shader that colors solids by depth (trim band, seam, panels),
strips along each kind of edge, corner pieces, a WFC fill deep inside,
hanging parts that sway, and a parallax skyline.

Which pieces, and every number about them, is a dressing set: one RON file
per stage in [`crates/tarborn/assets/dressing/`](crates/tarborn/assets/dressing/)
(`industrial.ron`, `forest.ron`). B cycles sets, and edits to a set file
show within half a second. How the shape is read is under `dressing` in
[`visuals.ron`](crates/tarborn/assets/visuals.ron). The **dressing room**
(T cycles rooms) has a ramp, rounded corners, an arch and a dome to see
every piece on.

## Controls

Two buttons, plus the stick:

| Action  | Keyboard               | Gamepad                         |
|---------|------------------------|---------------------------------|
| Move/aim| A / D / W / S, arrows  | Left stick, D-pad               |
| Jump    | Space or J             | South (A / Cross)               |
| Grip    | K or Left Shift (hold) | Either trigger, or West (X / Square) |
| Restart | R                      |                                 |
| Switch room (arena / training / dressing) | T |               |
| Mute    | M                      |                                 |
| Cycle look (lines / Pitch / dithered) | V |                    |
| Cycle level dressing (industrial / forest) | B |               |
| Camera effects on/off (A/B) | C |                            |

Two verbs: **Grip shrinks**, **Jump expands**.

- **Grip** (hold) makes the blob small and sticky. It clings to walls and
  ceilings and charges up over 0.6 s. Gripping a surface, directions follow
  the screen: up climbs a wall and down goes down it, pushing into a wall
  climbs it too (pulling away goes down), and left on a ceiling goes left.
  Pressing Grip a moment after bouncing off a surface (up to 0.15 s) still
  sticks: the blob snaps back onto it.
- **Pressing Grip is a dodge.** For 0.25 s after Grip goes down, nothing can
  hurt the blob (it flashes white). It forgives a late press too: an attack
  that lands up to 0.1 s before Grip goes down is still dodged. Shrinking
  again within 0.45 s of a dodge doesn't dodge, so time it rather than
  tapping. Staying small doesn't protect you.
- **Touching an enemy with Grip held grabs it**: landing on it, bumping into
  it in the air, or pressing Grip right beside it (within 0.2 s on a
  surface, so a shrunk blob doesn't grab whatever walks into it).
- **Jump** on a surface jumps instantly. Hold it to go higher.
- **Jump while shrunk** on a surface bursts: a lunge at a nearby enemy, or,
  with none near, a launch off the surface along the aim. Both are stronger
  with more charge (up to 2x).
- **Jump in the air** lunges at an enemy within 5 m (in the aimed 60° cone if
  you aim). With no enemy near, it does nothing extra. One per air time,
  refilled by touching a surface or by hitting something. You are
  invulnerable during a lunge.
- **Down + Jump in the air** slams straight down.
- **Holding Jump in the air** keeps the blob bouncy: hold through a landing to
  rebound, slam while holding to launch high. Release before landing to land
  soft.

What you hold when a lunge arrives decides how it ends:

| Holding    | Result                                                      |
|------------|-------------------------------------------------------------|
| nothing    | **Hit.** Full lunge damage.                                 |
| Jump       | **Bounce off** (pogo): less damage, launches you back up.   |
| Grip       | **Grab.** You hold the enemy; it is dizzy and helpless.     |

These timings and the rest of the controls' feel are in
[`assets/feel.ron`](crates/tarborn/assets/feel.ron), re-read while the game
runs.

While holding an enemy, **Jump throws it** along the aim (harder with more
charge), and **releasing Grip drops it**. A guard with its shield up can't be
grabbed from the front.

The blob flashes white while invulnerable.

### The run

The game opens on the **start menu**. Every screen that waits for you is a
menu: Up and Down (arrows, WASD, the d-pad or the stick) move the cursor,
Jump or Enter picks, and the mouse can point and click. Start run begins a
run: a short loading screen, then the stages one after another. Each stage
is walled in except for its exit (its last socket), drawn as a gold frame
with arrows sliding out through it. Reach it and the stage fades out and the
next one loads; health carries over. Dying or clearing the last stage shows
the **run-over** screen (seed, time, stages cleared, best combo) with Run
again and Quit to title.

On the start menu, the seed line takes Left and Right to step the seed,
typed digits (Backspace erases), or Jump for a random one, so a run can be
replayed. The menu also opens the debug rooms (training, arena, dressing,
dark), and **Play a chunk** lists every chunk file under
[`assets/chunks/`](crates/tarborn/assets/chunks/) except the rooms, read
fresh each time, to playtest one on its own: walled in, its last socket
the exit. Dying replays it, the exit goes back to the list, and the pause
menu has Restart chunk (which rereads the file, so edits show up) and Back
to chunks. A chunk that fails to load or validate says why on the list. **Esc** (or the pad's Start) pauses a stage or room: Resume, Restart
(the run from its first stage, or the room), or Quit to title. The two
stages are placeholders in
[`assets/chunks/stages/`](crates/tarborn/assets/chunks/stages/) until stages
are assembled from generated chunks. Timings, texts, the exit marker, the
seal thickness and the stage list are in
[`assets/run.ron`](crates/tarborn/assets/run.ron).

### Rooms

The **arena**: five waves that mix every enemy kind, more
and more. Clear a wave and the next drops in after a short break. Arena
enemies stay dead, but two grubs are always around to throw or to get eaten
by brutes. Kills drop tar that heals. Dying restarts the arena, and R
restarts any debug room.

- The boxes in the top right are the waves: yellow is the current one,
  green is beaten. Between waves, the next wave's number flashes in the
  middle.
- The number in the top middle is your **combo**: every hit on an enemy,
  including enemies hurting each other, adds one if it comes within 2 s of
  the last. Getting hurt drops it. The bar under it is the time left, and
  the small grey number is your best chain.

In a debug room, T switches to the **training room**, which has one of each enemy (they
respawn) and some terrain to try moves on. Press it again for the **dressing
room** (slopes and arcs, a grub and a guard), and again to return to the arena.

Every room is data: a chunk file in
[`crates/tarborn/assets/chunks/rooms/`](crates/tarborn/assets/chunks/rooms/)
holding its solids, hazards, enemies and waves. The game re-reads the file
each time a room starts (R or T), so edits show up without rebuilding. The
chunk geometry rules (grid cell, narrowest socket) are in
[`assets/pcg/chunk.ron`](crates/tarborn/assets/pcg/chunk.ron).
`cargo run -p tarborn_pcg --example export_rooms` rewrites the room files
from the rooms built in `tarborn_sim`, and `cargo test -p tarborn_pcg`
checks that the files still play tick for tick like those rooms.

Bots and telemetry read play as **beats**, the moves the two verbs produce:
roll, jump, burst, stick to a wall or ceiling, drop, bounce, lunge, pogo,
slam, grab-and-throw, and fights. `tarborn_pcg::tape` plays an input tape
(button and direction changes by tick) headlessly, and the beat detector
names what the blob actually did. Its thresholds are in
[`assets/pcg/beats.ron`](crates/tarborn/assets/pcg/beats.ron).

The **planner bot** (`tarborn_pcg::planner`) finds tapes that take the blob
to a goal. It is a Go-Explore style search over macro-actions (roll, tap or
hold Jump, Grip with a direction, burst, slam) in the full simulation: it
keeps the quickest state reaching each 0.5 m cell and expands the cells
closest to the goal plus a few at random. It keeps up to N solutions with
different beat sequences, and checks each one against K enemy variations
(enemies nudged sideways, random facing). Its knobs, macro list included,
are in [`assets/pcg/planner.ron`](crates/tarborn/assets/pcg/planner.ron).
A goal is a box the blob has to come to rest in, touching a surface.

**Route metrics** (`tarborn_pcg::metrics`) come from the bots' solutions:

- **length**: seconds the easiest solution takes;
- **complexity**: beats left after dropping every input the route works without;
- **difficulty**: each required input is moved earlier and later until the
  route fails; narrow windows are strict (0 relaxed, 1 frame-perfect),
  scored on the easiest solution;
- **diversity**: entropy of the beat kinds used;
- **novelty**: beat n-gram distance to the nearest other routes.

```sh
cargo run --release -p tarborn_pcg --bin report            # training room and arena
cargo run --release -p tarborn_pcg --bin report -- arena   # one room
```

The report measures the goal points listed in
[`assets/pcg/metrics.ron`](crates/tarborn/assets/pcg/metrics.ron), which also
holds every weight and threshold.

### Combat

The test room has a training dummy (grey circle with a cross), grubs (small
purple circles), guards (blue rectangles), a spitter (orange diamond), a
skitter (yellow triangle), a brute (big red square, on the plateau) and a
strip of spikes.

- Your body is the weapon. A lunge hits for a fixed amount (more from a
  charged burst), a slam hits whatever it lands on and sends out a shockwave, landing
  on an enemy with Jump held pogoes off it, and hitting anything at high
  speed (faster than you can roll, so being flung or thrown) hurts it.
- Every hit freezes the world for a moment, knocks the target back, and
  refreshes your lunge, so hits chain in the air.
- Damage ignores sides. A thrown enemy hurts whatever it hits (and itself, if
  it hits a wall hard), spit hits any enemy in its way, and a guard's swing
  hurts other enemies too.
- You have 5 health. Getting hurt costs one and makes you blink, invulnerable
  for a second. Kills drop tar (yellow) that heals one. At 0 the room restarts.

## How responsiveness is handled

- Gameplay runs at a fixed 120 Hz tick, independent of the frame rate.
  Rendering interpolates between ticks so motion stays smooth.
- Input is sampled after devices update and *before* the fixed ticks of the
  same frame, and a jump press is latched until a tick consumes it, so no
  press is lost or delayed by a frame.
- Jump buffering (120 ms), coyote time (100 ms) and variable jump height are
  part of the simulation and covered by tests in
  `crates/tarborn_sim/tests/feel.rs`.

## Enemies

Enemies share one set of states, but each kind uses only some of them. The
state decides how easy an enemy is to hurt, and its colour and the mark over
its head show it.

| State      | Mark                  | What it means for you                                   |
|------------|-----------------------|---------------------------------------------------------|
| Unaware    | none                  | Hasn't spotted you. First hit is a surprise: triple damage. |
| Surprised  | white `!`             | Frozen for a moment, triple damage.                     |
| Chasing    | none                  | Skitter and brute: sees you and is coming.              |
| Searching  | orange ring           | Saw you, then lost sight. Checking your last spot; gives up after 4 s. |
| Defending  | cyan shield line      | Guard only: blocks lunges and impacts from the front.   |
| Attacking  | flashing red, then swing circle or shot | Wind-up, then the attack. Open afterwards. Shrink as it lands to dodge. |
| Dizzy      | orbiting dots, grey   | Double damage. Caused by slams, pogos, shockwaves.      |
| Prone      | flattened, grey       | Double damage.                                          |
| Fleeing    | green                 | Running away from you or from something it fears.       |
| Dodging    | white dash            | Skitter only: hopping out of the way of a lunge.        |

- **Sight only**: an enemy on screen spots you if no terrain is in the way,
  in any direction. There is no hearing. Terrain always blocks sight, so the
  way to get a surprise hit is to come at an enemy from behind a wall, a
  ledge or the edge of the screen. Spotting you within 3 m startles it.
- **Guard**: patrols, blocks from the front even while winding up, and turns
  around only after you've been behind it for 0.4 s. Beat it from behind,
  from above (slam or pogo makes it dizzy), or right after its swing.
- **Grub**: harmless. Wanders, startles, and runs away.
- **Spitter** (orange diamond): keeps its distance, aims for 0.6 s (a line
  grows toward you), then spits a glob that flies straight. It backs away if
  you get within 3 m, and takes double damage while recovering from a shot.
- **Skitter** (yellow triangle): fast, and too twitchy to be surprised. It
  hops out of the way of any lunge or grab, then comes straight back with a
  pounce. Beat it with what it can't hop away from: a
  slam or its shockwave, landing on it with Jump held, something thrown at
  it, or a lunge while it recovers from a pounce (double damage).
- **Brute** (big red square): slow, with 200 health. It rises up for 0.9 s,
  then slams the ground, hurting everything on the ground within 3 m,
  friend or not. Be in the air when it lands, or shrink right then. It can't
  be grabbed, barely moves when hit, and shrugs off hits during its wind-up.
  It takes double damage while recovering, and a slam then knocks it prone.
- **Shrink to dodge**: press Grip as any attack lands (a little late is fine) (spit, a swing, a
  pounce, a brute's slam) and it passes you by. The combo is to shrink under
  the attack, then press Jump to burst into the enemy while it recovers.

### How enemies treat each other

- Skitters hunt grubs and run from brutes.
- Guards attack skitters on sight.
- Brutes walk over to grubs they see and eat them, healing 40.
- Grubs run from skitters and brutes.
- Every attack hurts whatever it hits: spit, swings, pounces, the brute's
  slam and thrown bodies.

## Sound

There are no recorded sounds. Every effect is synthesized at startup from
numbers in [`crates/tarborn/assets/sfx.ron`](crates/tarborn/assets/sfx.ron),
and the game reloads that file within half a second of saving it, so sounds
can be tuned while playing. A mistake in the file is logged and the previous
sounds stay. Set `TARBORN_SFX=/path/to/file.ron` to play from another file.

The synth follows [sfxr](https://www.drpetter.se/project_sfxr.html) and its
descendants ([jsfxr](https://sfxr.me/), bfxr, ChipTone): each voice is a
waveform (sine, triangle, square, saw or noise) with a pitch glide, an
envelope with punch, vibrato, an arpeggio step, and swept resonant low- and
high-pass filters. Unlike sfxr, a sound is several **layers** mixed together,
each with its own delay. That is how impacts are built: a bright transient
click, a pitched body thump that drops in pitch, and a filtered noise tail.

Each sound also says how it varies per play: random pitch and volume
spread, how intensity (hit damage, fall speed, charge) maps to volume and
pitch, a cooldown and a voice limit. The `cues` section tunes when sounds
fire: landing and sticking speeds, how far enemies can be heard, and so on.

- **Impact**, **stick** and **jump** come first. Landing while shrunk sticks
  (a wet, resonant squelch) instead of landing (a soft thud); gripping a wall
  after rolling into it sticks too.
- **Enemy telegraphs** play over each windup and last as long as it does
  (a test keeps them in step with the windup times). Hitting an enemy out of
  its windup cuts its telegraph short.
- Enemy sounds get quieter with distance from the blob.

To listen outside the game, render every sound to WAV files:

```sh
cargo run -p tarborn_sfx --example render -- crates/tarborn/assets/sfx.ron sfx_out/
```

## Look

The default look, Pitch, draws filled shapes: a glossy black tar blob on a
pale industrial level, with girders inside the walls generated by wave
function collapse. V cycles through the original line drawing, Pitch, and
Pitch dithered down to four colors. Every visual number lives in
`crates/tarborn/assets/visuals.ron`, which the running game re-reads when it changes.

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

## Controls

Two buttons, plus the stick:

| Action  | Keyboard               | Gamepad                         |
|---------|------------------------|---------------------------------|
| Move/aim| A / D / W / S, arrows  | Left stick, D-pad               |
| Jump    | Space or J             | South (A / Cross)               |
| Grip    | K or Left Shift (hold) | Either trigger, or West (X / Square) |
| Restart | R                      |                                 |
| Switch room (arena / training) | T |                          |
| Mute    | M                      |                                 |

Two verbs: **Grip shrinks**, **Jump expands**.

- **Grip** (hold) makes the blob small and sticky. It clings to walls and
  ceilings (roll to crawl along them) and charges up over 0.6 s.
- **The moment of shrinking is a dodge.** For 0.15 s while the blob goes
  from full size to small, nothing can hurt it (it flashes white). Shrinking
  again within 0.45 s of a dodge is instant and doesn't dodge, so time it
  rather than tapping. Staying small doesn't protect you.
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

While holding an enemy, **Jump throws it** along the aim (harder with more
charge), and **releasing Grip drops it**. A guard with its shield up can't be
grabbed from the front.

The blob flashes white while invulnerable.

### Rooms

The game starts in the **arena**: five waves that mix every enemy kind, more
and more. Clear a wave and the next drops in after a short break. Arena
enemies stay dead, but two grubs are always around to throw or to get eaten
by brutes. Kills drop tar that heals. Dying restarts the arena.

- The boxes in the top right are the waves: yellow is the current one,
  green is beaten. Between waves, the next wave's number flashes in the
  middle.
- The number in the top middle is your **combo**: every hit on an enemy,
  including enemies hurting each other, adds one if it comes within 2 s of
  the last. Getting hurt drops it. The bar under it is the time left, and
  the small grey number is your best chain.

Press T to switch to the **training room**, which has one of each enemy (they
respawn) and some terrain to try moves on.

### Combat

The test room has a training dummy (grey circle with a cross), grubs (small
purple circles), guards (blue rectangles), a spitter (orange diamond), a
skitter (yellow triangle), a brute (big red square, on the plateau) and a
strip of spikes.

- Your body is the weapon. A lunge hits for a fixed amount (more from a
  charged burst), a slam hits whatever it lands on and sends out a shockwave, landing
  on an enemy with Jump held pogoes off it, and hitting anything at high
  speed hurts it.
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
- **Shrink to dodge**: press Grip just as any attack lands (spit, a swing, a
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

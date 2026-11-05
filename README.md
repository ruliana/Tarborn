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
  tarborn/       The Bevy game: input, fixed 120 Hz tick, rendering.
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

Everything comes from those two:

- **Jump** on a surface jumps instantly. Hold it to go higher.
- **Jump in the air** lunges: a short, gravity-free burst in the aimed
  direction (or the way you face). You are invulnerable during it, and it
  snaps toward an enemy within 30°. One per air time, refilled by touching a
  surface or by hitting something. A press just before landing is a buffered
  jump instead.
- **Down + Jump in the air** slams straight down.
- **Holding Jump** keeps the blob bouncy: hold through a landing to rebound,
  land on an enemy to pogo off it, slam while holding to launch high.
  Release before landing to land soft.
- **Grip** clings to walls and ceilings; roll to crawl along them. It is
  silent, so it is also how you sneak.
- **Grip + hold Jump** on a surface coils: the blob shrinks, dodging some
  attacks. Release to launch, faster the longer you held (up to 2x a jump at
  0.6 s), along the aim if you point away from or along the surface. If an
  enemy is near where the launch is headed, it becomes a lunge at that enemy
  that hits harder the longer you coiled.

The blob flashes white while invulnerable.

### Combat

The test room has a training dummy (grey circle with a cross), grubs (small
purple circles), guards (blue rectangles), a spitter (orange diamond) and a
strip of spikes.

- Your body is the weapon. A lunge hits for a fixed amount (more from a
  coil), a slam hits whatever it lands on and sends out a shockwave, landing
  on an enemy with Jump held pogoes off it, and hitting anything at high
  speed hurts it.
- Every hit freezes the world for a moment, knocks the target back, and
  refreshes your lunge, so hits chain in the air. Lunges snap toward an enemy
  within 30° of where you aim.
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
| Unaware    | none                  | First hit is a surprise: triple damage.                 |
| Surprised  | white `!`             | Frozen for a moment, triple damage.                     |
| Suspicious | yellow ring           | Heard you. Turns to look after a short reaction.        |
| Searching  | orange ring           | Lost you, checking your last spot. Gives up after 4 s.  |
| Defending  | cyan shield line      | Guard only: blocks lunges and impacts from the front.   |
| Attacking  | flashing red, then swing circle or shot | Wind-up, then the attack. Open afterwards. Shrinking dodges it. |
| Dizzy      | orbiting dots, grey   | Double damage. Caused by slams, pogos, shockwaves.      |
| Prone      | flattened, grey       | Double damage.                                          |
| Fleeing    | green                 | Grub only: running away.                                |

- **Sight**: enemies see in front of them, not through walls. Guards see
  8 m, grubs 4 m. Spotting you within 3 m startles them.
- **Noise**: jumps, lunges, launches, bounces, hits and slams make noise of
  growing reach (the white rings). Fast rolling makes a little. Sticky
  movement makes none, so crawling is how you sneak.
- **Guard**: patrols, blocks from the front even while winding up, and turns
  around only after you've been behind it for 0.4 s. Beat it from behind,
  from above (slam or pogo makes it dizzy), or right after its swing.
- **Grub**: harmless. Wanders, startles, and runs away.
- **Spitter** (orange diamond): keeps its distance, aims for 0.6 s (a line
  grows toward you), then spits a glob that flies straight. It backs away if
  you get within 3 m, and takes double damage while recovering from a shot.
- **Shrink to dodge**: while you are coiling (Grip + hold Jump), spit and
  guard swings pass you by. The combo is to shrink under the attack, then
  release to launch into the enemy while it recovers. A coil can launch along
  the surface too, so on the floor you can aim straight at the enemy.

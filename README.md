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

| Action      | Keyboard              | Gamepad              |
|-------------|-----------------------|----------------------|
| Roll / aim  | A / D / W / S, arrows | Left stick, D-pad    |
| Jump        | Space                 | South (A / Cross)    |
| Lunge       | J or X                | West (X / Square)    |
| Sticky      | K, Left Shift (hold)  | Right trigger        |
| Bouncy      | L (hold)              | Left trigger         |
| Restart     | R                     |                      |

- **Lunge** is a short, gravity-free burst in the aimed direction (or the way
  you face). You are invulnerable during it. You get one in the air, refilled
  by touching any surface. Jumping cancels it and keeps its speed.
- **Slam** is a lunge aimed down while in the air: straight down, hard.
- **Sticky** clings to walls and ceilings; roll to crawl along them.
- **Coil**: while sticky and touching a surface, holding jump shrinks the
  blob instead of jumping. Release to launch off the surface: the longer the
  hold, the faster (up to 2x a jump at 0.6 s). Aim away from the surface to
  choose the direction. Plain jumps without sticky are still instant.
- **Bouncy** is springy and nearly frictionless: you rebound off whatever you
  hit. Slam with bouncy held to launch yourself high.

The blob flashes white while invulnerable.

### Combat

The test room has training dummies (purple circles) and a strip of spikes.

- Your body is the weapon. A lunge always hits for a fixed amount, a slam
  hits whatever it lands on and sends out a shockwave, landing on an enemy
  with bouncy held pogoes off it, and hitting anything at high speed hurts it.
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

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

| Action  | Keyboard            | Gamepad              |
|---------|---------------------|----------------------|
| Roll    | A / D, arrow keys   | Left stick, D-pad    |
| Jump    | Space, W, Up        | South (A / Cross)    |
| Sticky  | J, Left Shift       | Right trigger        |
| Slick   | K                   | Left trigger         |
| Heavy   | L, S, Down          | West (X / Square)    |
| Restart | R                   |                      |

Sticky lets you cling to walls and ceilings and roll up them. Slick removes
friction. Heavy falls harder and holds its shape. They combine.

## How responsiveness is handled

- Gameplay runs at a fixed 120 Hz tick, independent of the frame rate.
  Rendering interpolates between ticks so motion stays smooth.
- Input is sampled after devices update and *before* the fixed ticks of the
  same frame, and a jump press is latched until a tick consumes it, so no
  press is lost or delayed by a frame.
- Jump buffering (120 ms), coyote time (100 ms) and variable jump height are
  part of the simulation and covered by tests in
  `crates/tarborn_sim/tests/feel.rs`.

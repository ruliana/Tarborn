//! Hand-written tapes in the training room, one per beat type: the beat
//! detector names each move they perform.

use glam::Vec2;
use tarborn_pcg::{BeatKind, BeatParams, Buttons, Chunk, ChunkParams, Run, Tape, chunk_dir, play};
use tarborn_sim::EnemyKind;

const LEFT: Vec2 = Vec2::NEG_X;
const RIGHT: Vec2 = Vec2::X;

fn training() -> Chunk {
    Chunk::load(
        &chunk_dir("rooms").join("training.ron"),
        &ChunkParams::default(),
    )
    .unwrap()
}

fn run(tape: &Tape, ticks: u64) -> Run {
    let mut sim = training().sim();
    play(&mut sim, tape, ticks, &BeatParams::default())
}

fn kinds(run: &Run) -> Vec<BeatKind> {
    run.beats.iter().map(|b| b.kind).collect()
}

fn has(run: &Run, kind: BeatKind) -> bool {
    run.beats.iter().any(|b| b.kind == kind)
}

fn enemies_of(run: &Run, kind: BeatKind) -> Vec<EnemyKind> {
    run.beats
        .iter()
        .find(|b| b.kind == kind)
        .unwrap_or_else(|| panic!("no {kind:?} in {:?}", kinds(run)))
        .enemies
        .clone()
}

fn b(dir: Vec2) -> Buttons {
    Buttons::dir(dir)
}

/// Spawned in the air, roll right and press Jump at once: an air lunge at
/// the grub by the spawn point. `then` adds what happens on arrival.
fn lunge_at_the_grub(then: impl FnOnce(Tape) -> Tape) -> Tape {
    then(Tape::new().at(0, b(RIGHT).with_jump()).at(8, b(RIGHT)))
}

/// Roll left into the pillar with Grip held, so the blob climbs it.
fn climb_the_pillar() -> Tape {
    Tape::new().at(60, b(LEFT).with_grip())
}

#[test]
fn rolling() {
    let run = run(&Tape::new().at(60, b(RIGHT)), 240);
    assert!(has(&run, BeatKind::Roll), "{:?}", kinds(&run));
}

#[test]
fn jumping_without_a_drop() {
    let tape = Tape::new()
        .at(60, Buttons::NONE.with_jump())
        .at(70, Buttons::NONE);
    let run = run(&tape, 240);
    assert_eq!(kinds(&run), [BeatKind::Jump]);
}

#[test]
fn bursting_off_the_floor() {
    // Roll left, away from the grub, then charge in place and expand.
    let up = b(Vec2::Y).with_grip();
    let tape = Tape::new()
        .at(60, b(LEFT))
        .at(150, up)
        .at(250, up.with_jump())
        .at(260, Buttons::NONE);
    let run = run(&tape, 400);
    assert!(has(&run, BeatKind::Burst), "{:?}", kinds(&run));
}

#[test]
fn sticking_to_a_wall_then_dropping_off_its_top() {
    let run = run(&climb_the_pillar(), 400);
    let k = kinds(&run);
    let wall = k.iter().position(|&x| x == BeatKind::StickWall);
    let drop = k.iter().position(|&x| x == BeatKind::Drop);
    assert!(wall.is_some() && drop > wall, "{k:?}");
}

#[test]
fn sticking_to_the_ceiling() {
    // Climb the pillar, hold still on it, then burst up and right from it to
    // the ceiling, keeping Grip held.
    let aim = b(Vec2::new(0.5, 1.0)).with_grip();
    let tape = climb_the_pillar()
        .at(200, Buttons::NONE.with_grip())
        .at(280, aim.with_jump())
        .at(288, aim);
    let run = run(&tape, 500);
    assert!(has(&run, BeatKind::StickCeiling), "{:?}", kinds(&run));
}

#[test]
fn bouncing() {
    let run = run(&Tape::new().at(60, Buttons::NONE.with_jump()), 400);
    assert!(has(&run, BeatKind::Bounce), "{:?}", kinds(&run));
    assert!(!has(&run, BeatKind::Drop), "{:?}", kinds(&run));
}

#[test]
fn slamming() {
    let tape = Tape::new()
        .at(60, Buttons::NONE.with_jump())
        .at(70, Buttons::NONE)
        .at(110, b(Vec2::NEG_Y).with_jump())
        .at(120, Buttons::NONE);
    let run = run(&tape, 300);
    assert!(has(&run, BeatKind::Slam), "{:?}", kinds(&run));
}

#[test]
fn lunging_at_an_enemy_and_winning_the_fight() {
    // Two more presses: lunges at the grub until it dies.
    let tape = lunge_at_the_grub(|t| {
        t.at(13, Buttons::NONE)
            .at(45, Buttons::NONE.with_jump())
            .at(53, Buttons::NONE)
            .at(65, Buttons::NONE.with_jump())
            .at(73, Buttons::NONE)
    });
    let run = run(&tape, 400);
    assert_eq!(enemies_of(&run, BeatKind::Lunge), [EnemyKind::Grub]);
    assert_eq!(enemies_of(&run, BeatKind::Fight), [EnemyKind::Grub]);
}

#[test]
fn pogoing_off_an_enemy() {
    // Lunge again at the next grub with Jump held through the arrival.
    let tape = lunge_at_the_grub(|t| t.at(35, b(RIGHT).with_jump()));
    let run = run(&tape, 300);
    assert_eq!(enemies_of(&run, BeatKind::Pogo), [EnemyKind::Grub]);
}

#[test]
fn grabbing_and_throwing_an_enemy() {
    // Lunge, hold Grip as it arrives to grab, then Jump to throw.
    let back = b(Vec2::new(-1.0, 0.5));
    let tape = Tape::new()
        .at(0, b(RIGHT).with_jump())
        .at(8, b(RIGHT))
        .at(10, b(RIGHT).with_jump())
        .at(11, b(RIGHT).with_grip())
        .at(70, back.with_grip().with_jump())
        .at(80, Buttons::NONE);
    let run = run(&tape, 300);
    assert_eq!(enemies_of(&run, BeatKind::GrabThrow), [EnemyKind::Grub]);
}

#[test]
fn kills_by_other_enemies_are_not_the_players_fight() {
    // Standing still, the training room's guards still kill the skitter.
    let run = run(&Tape::new(), 600);
    assert!(
        run.events
            .iter()
            .any(|(_, e)| matches!(e, tarborn_sim::Event::Killed { .. })),
        "nothing died, so this test checks nothing"
    );
    assert!(!has(&run, BeatKind::Fight), "{:?}", kinds(&run));
}

#[test]
fn the_same_tape_gives_the_same_beats() {
    let tape = climb_the_pillar().at(200, b(Vec2::Y).with_grip());
    assert_eq!(run(&tape, 500), run(&tape, 500));
}

#[test]
fn tapes_round_trip_through_ron() {
    let tape = lunge_at_the_grub(|t| t.at(35, b(RIGHT).with_jump()));
    let text = ron::to_string(&tape).unwrap();
    assert_eq!(ron::from_str::<Tape>(&text).unwrap(), tape);
}

#[test]
fn the_tapes_above_cover_every_beat_kind() {
    // One beat kind added without a tape above fails here.
    let covered = [
        BeatKind::Roll,
        BeatKind::Jump,
        BeatKind::Burst,
        BeatKind::StickWall,
        BeatKind::StickCeiling,
        BeatKind::Drop,
        BeatKind::Bounce,
        BeatKind::Lunge,
        BeatKind::Pogo,
        BeatKind::Slam,
        BeatKind::GrabThrow,
        BeatKind::Fight,
    ];
    for kind in BeatKind::ALL {
        assert!(covered.contains(&kind), "no tape for {kind:?}");
    }
}

#[test]
fn jump_counts_as_pressed_only_on_the_tick_it_goes_down() {
    let tape = Tape::new().at(5, Buttons::NONE.with_jump());
    assert!(!tape.intent(4).jump_pressed);
    assert!(tape.intent(5).jump_pressed);
    assert!(!tape.intent(6).jump_pressed);
    assert!(tape.intent(6).jump_held);
}

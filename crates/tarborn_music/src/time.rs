//! Beats to seconds, with swing.

/// Swung position, in beats, of a straight position `beats`.
///
/// Each beat is split into two eighth notes. With `ratio` 0.5 they are even
/// (straight); at 0.62 the first eighth takes 62% of the beat and the
/// offbeat lands late, which is the jazz lilt. The mapping is piecewise
/// linear inside each beat, so downbeats never move and order is kept.
pub fn swing(beats: f32, ratio: f32) -> f32 {
    let r = ratio.clamp(0.01, 0.99);
    let whole = beats.floor();
    let p = beats - whole;
    let swung = if p < 0.5 {
        p * 2.0 * r
    } else {
        r + (p - 0.5) * 2.0 * (1.0 - r)
    };
    whole + swung
}

/// Seconds at `bpm` of a swung position given in straight beats.
pub fn beats_to_seconds(beats: f32, bpm: f32, swing_ratio: f32) -> f32 {
    swing(beats, swing_ratio) * 60.0 / bpm.max(1.0)
}

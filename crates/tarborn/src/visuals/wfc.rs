//! Wave function collapse, simple tiled model, for dressing the inside of solids.
//!
//! There are 16 tiles: one per combination of struts leaving the north, east,
//! south and west edges of a cell (bit 0 = N, 1 = E, 2 = S, 3 = W). Two
//! neighbouring cells must agree on their shared edge, and no strut may leave
//! the region. Because every edge combination exists as a tile, the solver can
//! never reach a contradiction; the weights alone decide the look.

/// Relative odds of each tile family. Zero bans a family.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct Weights {
    /// No strut in the cell.
    pub empty: f32,
    /// A strut that stops in the cell.
    pub end: f32,
    /// A strut passing straight through.
    pub straight: f32,
    /// A strut turning a corner.
    pub corner: f32,
    /// Three or four struts meeting.
    pub junction: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            empty: 3.0,
            end: 0.3,
            straight: 4.0,
            corner: 1.2,
            junction: 0.6,
        }
    }
}

pub const N: u8 = 1;
pub const E: u8 = 2;
pub const S: u8 = 4;
pub const W: u8 = 8;
const DIRS: [(u8, i32, i32, u8); 4] = [(N, 0, 1, S), (E, 1, 0, W), (S, 0, -1, N), (W, -1, 0, E)];

impl Weights {
    fn of(&self, tile: u8) -> f32 {
        match tile.count_ones() {
            0 => self.empty,
            1 => self.end,
            2 if tile == N | S || tile == E | W => self.straight,
            2 => self.corner,
            _ => self.junction,
        }
    }
}

/// A grid of cells; `inside[y * width + x]` says which take part. y grows upwards.
pub struct Grid {
    pub width: usize,
    pub height: usize,
    pub inside: Vec<bool>,
}

impl Grid {
    fn at(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height)
            .then(|| y as usize * self.width + x as usize)
            .filter(|&i| self.inside[i])
    }
}

/// Small deterministic generator so a seed always gives the same room.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_f32(&mut self) -> f32 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Collapses the grid. Returns one tile per cell; cells outside get `None`.
pub fn collapse(grid: &Grid, weights: &Weights, rng: &mut Rng) -> Vec<Option<u8>> {
    let n = grid.width * grid.height;
    // Possible tiles per cell as a 16-bit set.
    let mut possible = vec![0u16; n];
    // Tiles that fit the region, ignoring weights: the fallback when a ban
    // (a zero weight) leaves a cell with nothing that fits.
    let mut fits = vec![0u16; n];
    for y in 0..grid.height as i32 {
        for x in 0..grid.width as i32 {
            let Some(i) = grid.at(x, y) else { continue };
            for tile in 0..16u8 {
                let leaves = DIRS
                    .iter()
                    .any(|&(bit, dx, dy, _)| tile & bit != 0 && grid.at(x + dx, y + dy).is_none());
                if !leaves {
                    fits[i] |= 1 << tile;
                    if weights.of(tile) > 0.0 {
                        possible[i] |= 1 << tile;
                    }
                }
            }
            if possible[i] == 0 {
                possible[i] = fits[i];
            }
        }
    }

    let mut out: Vec<Option<u8>> = vec![None; n];
    loop {
        // Lowest entropy first, ties broken at random.
        let mut best = None;
        let mut best_score = f32::INFINITY;
        for i in 0..n {
            if !grid.inside[i] || out[i].is_some() {
                continue;
            }
            let score = possible[i].count_ones() as f32 + rng.next_f32() * 0.5;
            if score < best_score {
                best_score = score;
                best = Some(i);
            }
        }
        let Some(cell) = best else { break };

        let tiles: Vec<u8> = (0..16u8)
            .filter(|t| possible[cell] & (1 << t) != 0)
            .collect();
        let total: f32 = tiles.iter().map(|&t| weights.of(t).max(1e-4)).sum();
        let mut pick = rng.next_f32() * total;
        let mut chosen = *tiles.last().unwrap();
        for &t in &tiles {
            pick -= weights.of(t).max(1e-4);
            if pick <= 0.0 {
                chosen = t;
                break;
            }
        }
        out[cell] = Some(chosen);
        possible[cell] = 1 << chosen;

        // Propagate: a neighbour keeps only tiles that agree on the shared edge.
        let mut stack = vec![cell];
        while let Some(j) = stack.pop() {
            let (jx, jy) = ((j % grid.width) as i32, (j / grid.width) as i32);
            for &(bit, dx, dy, opposite) in &DIRS {
                let Some(k) = grid.at(jx + dx, jy + dy) else {
                    continue;
                };
                if out[k].is_some() {
                    continue;
                }
                let (mut with, mut without) = (false, false);
                for t in 0..16u8 {
                    if possible[j] & (1 << t) != 0 {
                        if t & bit != 0 {
                            with = true
                        } else {
                            without = true
                        }
                    }
                }
                let agree = |set: u16| {
                    (0..16u8)
                        .filter(|t| set & (1 << t) != 0)
                        .filter(|t| {
                            let has = t & opposite != 0;
                            (has && with) || (!has && without)
                        })
                        .fold(0u16, |acc, t| acc | (1 << t))
                };
                let mut next = agree(possible[k]);
                if next == 0 {
                    next = agree(fits[k]);
                }
                if next != possible[k] && next != 0 {
                    possible[k] = next;
                    stack.push(k);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(width: usize, height: usize) -> Grid {
        Grid {
            width,
            height,
            inside: vec![true; width * height],
        }
    }

    #[test]
    fn neighbours_agree_and_nothing_leaves_the_region() {
        let mut grid = block(12, 6);
        // Punch a hole so there are inner borders too.
        for y in 2..4 {
            for x in 4..8 {
                grid.inside[y * 12 + x] = false;
            }
        }
        for seed in 0..20 {
            let tiles = collapse(&grid, &Weights::default(), &mut Rng::new(seed));
            for y in 0..grid.height as i32 {
                for x in 0..grid.width as i32 {
                    let Some(i) = grid.at(x, y) else {
                        assert!(tiles[(y as usize) * 12 + x as usize].is_none());
                        continue;
                    };
                    let t = tiles[i].expect("every inside cell is collapsed");
                    for &(bit, dx, dy, opposite) in &DIRS {
                        match grid.at(x + dx, y + dy) {
                            None => assert_eq!(t & bit, 0, "strut leaves the region"),
                            Some(k) => assert_eq!(
                                t & bit != 0,
                                tiles[k].unwrap() & opposite != 0,
                                "neighbours disagree"
                            ),
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn same_seed_same_result() {
        let grid = block(10, 4);
        let a = collapse(&grid, &Weights::default(), &mut Rng::new(7));
        let b = collapse(&grid, &Weights::default(), &mut Rng::new(7));
        assert_eq!(a, b);
    }

    #[test]
    fn zero_weight_makes_a_family_rare() {
        let grid = block(30, 10);
        let weights = Weights {
            junction: 0.0,
            ..Weights::default()
        };
        let tiles = collapse(&grid, &weights, &mut Rng::new(3));
        let junctions = tiles
            .iter()
            .flatten()
            .filter(|t| t.count_ones() >= 3)
            .count();
        assert!(junctions * 20 < tiles.len(), "{junctions} junctions");
    }
}

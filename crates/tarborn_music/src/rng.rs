//! The small deterministic PRNG every generator here uses (xorshift32), so
//! a seed always means the same music on every machine.

#[derive(Clone, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        let mut r = Self(seed.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B | 1);
        // Warm up so nearby seeds diverge at once.
        for _ in 0..4 {
            r.next_u32();
        }
        r
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in 0..1.
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }

    /// Uniform in lo..=hi.
    pub fn range(&mut self, [lo, hi]: [f32; 2]) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    /// Uniform integer in lo..=hi.
    pub fn int(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u32() % (hi - lo + 1) as u32) as i32
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next_u32() as usize) % items.len()]
    }

    /// An index picked with the given weights (all zero picks the first).
    pub fn weighted(&mut self, weights: &[f32]) -> usize {
        let total: f32 = weights.iter().map(|w| w.max(0.0)).sum();
        if total <= 0.0 {
            return 0;
        }
        let mut x = self.unit() * total;
        for (i, w) in weights.iter().enumerate() {
            x -= w.max(0.0);
            if x < 0.0 {
                return i;
            }
        }
        weights.len() - 1
    }

    /// A child generator, so one part's choices never shift another's.
    pub fn fork(&mut self, salt: u32) -> Rng {
        Rng::new(self.next_u32() ^ salt.wrapping_mul(0x2545_F491))
    }
}

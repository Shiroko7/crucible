//! PCG32, written out rather than pulled in.
//!
//! Three reasons not to take a dependency here. The Monte Carlo path calls
//! this millions of times per evaluation, so it wants to be small enough to
//! inline and free of trait-object indirection. Reported results have to be
//! reproducible from a seed, which rules out anything reaching for OS entropy.
//! And on the Windows GNU toolchain the usual crate pulls in `getrandom` and a
//! DLL-linking chain that needs binutils on PATH, which is a lot of moving
//! parts to accept for sixty lines of arithmetic.
//!
//! PCG32 rather than a plain LCG or xorshift because the low bits of an LCG
//! are notoriously weak, and low bits are exactly what a `d20` samples.

/// Seedable, reproducible, and equal across platforms.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
    inc: u64,
}

const MULT: u64 = 6_364_136_223_846_793_005;

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self::with_stream(seed, 0xda3e_39cb_94b9_5bdb)
    }

    /// Independent streams, for parallel rollouts that must not correlate.
    ///
    /// Two generators with different `stream` values produce distinct
    /// sequences even from the same seed, which is what makes root-parallel
    /// work splittable without sharing state.
    pub fn with_stream(seed: u64, stream: u64) -> Self {
        let mut r = Self {
            state: 0,
            inc: (stream << 1) | 1,
        };
        r.next_u32();
        r.state = r.state.wrapping_add(seed);
        r.next_u32();
        r
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULT).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in `[0, n)`, with no modulo bias.
    ///
    /// Lemire's multiply-and-shift. The naive `next_u32() % n` is biased
    /// whenever `n` does not divide `2^32`, which for `n = 20` skews low rolls
    /// upward. Too small to notice in one roll and very much large enough to
    /// move a win probability over a million rollouts.
    #[inline]
    pub fn below(&mut self, n: u32) -> u32 {
        debug_assert!(n > 0, "below(0) has no uniform value");
        let mut x = self.next_u32();
        let mut m = u64::from(x) * u64::from(n);
        let mut low = m as u32;
        if low < n {
            let threshold = n.wrapping_neg() % n;
            while low < threshold {
                x = self.next_u32();
                m = u64::from(x) * u64::from(n);
                low = m as u32;
            }
        }
        (m >> 32) as u32
    }

    /// One die, in `[1, sides]`.
    #[inline]
    pub fn die(&mut self, sides: u32) -> i32 {
        self.below(sides) as i32 + 1
    }

    /// Uniform in `[0, 1)`.
    #[inline]
    pub fn unit(&mut self) -> f64 {
        f64::from(self.next_u32()) / 4_294_967_296.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_replays_exactly() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for i in 0..10_000 {
            assert_eq!(a.next_u32(), b.next_u32(), "diverged at draw {i}");
        }
    }

    #[test]
    fn different_streams_do_not_correlate() {
        let mut a = Rng::with_stream(1, 1);
        let mut b = Rng::with_stream(1, 2);
        let matches = (0..1_000).filter(|_| a.next_u32() == b.next_u32()).count();
        assert!(
            matches < 5,
            "{matches} collisions suggests a shared sequence"
        );
    }

    #[test]
    fn a_die_stays_on_its_faces_and_reaches_both_ends() {
        let mut r = Rng::new(7);
        let (mut lo, mut hi) = (false, false);
        for _ in 0..20_000 {
            let v = r.die(20);
            assert!((1..=20).contains(&v), "rolled {v} on a d20");
            lo |= v == 1;
            hi |= v == 20;
        }
        assert!(lo && hi, "both endpoints must be reachable");
    }

    /// Not a randomness test - a bias test. A d20 should land within a
    /// percent or so of 5% per face over this many samples.
    #[test]
    fn d20_faces_are_close_to_uniform() {
        let mut r = Rng::new(99);
        let mut counts = [0usize; 21];
        let n = 400_000;
        for _ in 0..n {
            counts[r.die(20) as usize] += 1;
        }
        for (face, &c) in counts.iter().enumerate().skip(1) {
            let share = c as f64 / n as f64;
            assert!(
                (share - 0.05).abs() < 0.002,
                "face {face} came up {share:.4} of the time"
            );
        }
    }
}

//! SplitMix64: a small seeded generator, so a run's order and an exam's
//! generated past are reproducible from their seeds alone, with no crate whose
//! next version could change the stream.

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n` (n > 0). The modulo bias is below 2^-40 for the
    /// small n an exam uses.
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "below(0)");
        self.next_u64() % n
    }

    /// Fisher–Yates.
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            v.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference stream for seed 0 (Vigna's splitmix64.c), so the
    /// generator, and every exam generated from it, never drifts.
    #[test]
    fn the_stream_matches_the_reference() {
        let mut r = Rng::new(0);
        assert_eq!(r.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(r.next_u64(), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(r.next_u64(), 0x06C4_5D18_8009_454F);
    }
}

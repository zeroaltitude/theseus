//! Grouping records of similar material (issue 21 of 1.0.7): k-means over the int8 cuts, on the
//! directions the cuts point in, so that a search by meaning can read the few groups nearest its
//! query and not the whole cold part.
//!
//! The centres are made from a sample (a few tens of thousands of records, whatever the store's
//! size) in a few rounds, and then every cold record goes to its nearest centre. It runs on the
//! embedding thread's own pool at its priority, when the machine is quiet, and nothing waits for
//! it: a search reads the layout that was written, and this makes the next.

use rayon::prelude::*;

use crate::embedder::{dot_i8, quantize};

/// A source of records' cuts: what the clustering reads, and a test stands in for.
pub(super) trait Cuts: Sync {
    fn cut(&self, e: u32) -> &[i8];
    fn scale(&self, e: u32) -> f32;
}

/// Group centres, as a search reads them: int8 and a scale each.
#[derive(Debug, Clone, Default)]
pub(super) struct Centres {
    pub cut: usize,
    pub q: Vec<i8>,
    pub scale: Vec<f32>,
}

impl Centres {
    pub fn len(&self) -> usize {
        self.scale.len()
    }

    pub fn centre(&self, g: usize) -> (&[i8], f32) {
        (&self.q[g * self.cut..(g + 1) * self.cut], self.scale[g])
    }

    /// The centre nearest `q`: the best of `dot * scale`, the earlier on a tie.
    pub fn nearest(&self, q: &[i8]) -> usize {
        let mut best = (f32::NEG_INFINITY, 0);
        for g in 0..self.len() {
            let (c, s) = self.centre(g);
            let v = dot_i8(q, c) as f32 * s;
            if v > best.0 {
                best = (v, g);
            }
        }
        best.1
    }
}

/// A small deterministic generator (splitmix64): the same store gives the same groups.
struct Rng(u64);

impl Rng {
    fn step(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.step() % n.max(1) as u64) as usize
    }
}

/// How many groups `cold` records make: about their square root, at least one.
pub(super) fn group_count(cold: usize) -> usize {
    ((cold as f64).sqrt().round() as usize).max(1)
}

/// A record's direction as floats: its cut times its scale.
fn direction(src: &dyn Cuts, e: u32, into: &mut [f32]) {
    let s = src.scale(e);
    for (o, &x) in into.iter_mut().zip(src.cut(e)) {
        *o = f32::from(x) * s;
    }
}

fn quantized(v: &[f32], cut: usize) -> Centres {
    let mut c = Centres {
        cut,
        ..Centres::default()
    };
    for row in v.chunks_exact(cut) {
        let (q, s) = quantize(row);
        c.q.extend_from_slice(&q);
        c.scale.push(s);
    }
    c
}

/// `k` centres for the records `entries`, from at most `sample` of them, in `rounds` rounds of
/// Lloyd's algorithm. The records' directions are unit length, so the centres are normalised
/// after each round (spherical k-means).
pub(super) fn make(
    src: &dyn Cuts,
    entries: &[u32],
    cut: usize,
    k: usize,
    sample: usize,
    rounds: usize,
) -> Centres {
    let k = k.min(entries.len()).max(1);
    let mut rng = Rng(entries.len() as u64 ^ 0xC0FF_EE00);
    // A sample drawn evenly through the list, jittered inside its stride.
    let take = sample.min(entries.len()).max(k);
    let stride = entries.len() as f64 / take as f64;
    let pick: Vec<u32> = (0..take)
        .map(|i| {
            let lo = (i as f64 * stride) as usize;
            let hi = (((i + 1) as f64 * stride) as usize).clamp(lo + 1, entries.len());
            entries[lo + rng.below(hi - lo)]
        })
        .collect();
    let mut centres = vec![0f32; k * cut];
    for (g, row) in centres.chunks_exact_mut(cut).enumerate() {
        direction(src, pick[g * pick.len() / k], row);
    }
    let mut cur = quantized(&centres, cut);
    for _ in 0..rounds {
        let assigned: Vec<u32> = pick
            .par_iter()
            .map(|&e| cur.nearest(src.cut(e)) as u32)
            .collect();
        let mut sums = vec![0f32; k * cut];
        let mut counts = vec![0u32; k];
        let mut dir = vec![0f32; cut];
        for (&e, &g) in pick.iter().zip(&assigned) {
            direction(src, e, &mut dir);
            let g = g as usize;
            counts[g] += 1;
            for (s, d) in sums[g * cut..(g + 1) * cut].iter_mut().zip(&dir) {
                *s += d;
            }
        }
        for g in 0..k {
            let row = &mut sums[g * cut..(g + 1) * cut];
            if counts[g] == 0 {
                // An empty group is seeded again from a record of the sample.
                direction(src, pick[rng.below(pick.len())], row);
            }
            let norm = row.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
            row.iter_mut().for_each(|x| *x /= norm);
        }
        centres = sums;
        cur = quantized(&centres, cut);
    }
    cur
}

/// Each of `entries`' nearest centre, in the same order.
pub(super) fn assign(src: &dyn Cuts, centres: &Centres, entries: &[u32]) -> Vec<u32> {
    entries
        .par_iter()
        .map(|&e| centres.nearest(src.cut(e)) as u32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Flat {
        q: Vec<Vec<i8>>,
    }

    impl Cuts for Flat {
        fn cut(&self, e: u32) -> &[i8] {
            &self.q[e as usize]
        }
        fn scale(&self, _: u32) -> f32 {
            1.0 / 127.0
        }
    }

    /// Records drawn round `centres` points: four well apart, 64 numbers each.
    fn planted(per: usize) -> (Flat, Vec<usize>) {
        let mut rng = Rng(7);
        let anchors: Vec<Vec<f32>> = (0..4)
            .map(|_| {
                (0..64)
                    .map(|_| (rng.step() % 2001) as f32 / 1000.0 - 1.0)
                    .collect()
            })
            .collect();
        let (mut q, mut of) = (Vec::new(), Vec::new());
        for i in 0..4 * per {
            let a = i % 4;
            let v: Vec<f32> = anchors[a]
                .iter()
                .map(|x| x + (rng.step() % 201) as f32 / 1000.0 - 0.1)
                .collect();
            q.push(quantize(&v).0);
            of.push(a);
        }
        (Flat { q }, of)
    }

    #[test]
    fn four_planted_groups_are_found_and_every_record_has_one() {
        let (src, of) = planted(50);
        let entries: Vec<u32> = (0..200).collect();
        let c = make(&src, &entries, 64, 4, 200, 8);
        assert_eq!(c.len(), 4);
        let got = assign(&src, &c, &entries);
        // Records of one planted group land together, and the four groups are four.
        let mut seen = std::collections::HashMap::new();
        for (e, &g) in got.iter().enumerate() {
            let want = seen.entry(of[e]).or_insert(g);
            assert_eq!(*want, g, "a planted group was split");
        }
        let distinct: std::collections::HashSet<u32> = seen.values().copied().collect();
        assert_eq!(distinct.len(), 4);
    }

    #[test]
    fn the_same_records_make_the_same_groups() {
        let (src, _) = planted(30);
        let entries: Vec<u32> = (0..120).collect();
        let a = make(&src, &entries, 64, 6, 120, 5);
        let b = make(&src, &entries, 64, 6, 120, 5);
        assert_eq!(a.q, b.q);
    }

    #[test]
    fn a_square_root_of_the_cold_count() {
        assert_eq!(group_count(291_000), 539);
        assert_eq!(group_count(0), 1);
        assert_eq!(group_count(1), 1);
    }
}

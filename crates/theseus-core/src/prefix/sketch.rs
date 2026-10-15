//! A header's sketch (theseus-rr15): a small hashed bag of words, the key the
//! library finds its nearest headers by. It is computed in the daemon from
//! the text alone, in microseconds, with nothing to wait for: the pick runs
//! at a compile point, on the turn's path, so it never asks the index tender
//! for a model embedding (a follow-up may store the tender's beside it).
//! Words are runs of letters and digits, lower-cased, each hashed (FNV-1a)
//! to one of `DIM` signed buckets; the vector is normalized and stored as
//! bytes, so a header's sketch is `DIM` bytes on disk and in memory.

/// The sketch's dimensions.
pub const DIM: usize = 64;

/// The sketch of `texts`, as one bag.
pub fn of<'a>(texts: impl IntoIterator<Item = &'a str>) -> Vec<i8> {
    let mut v = [0f32; DIM];
    for text in texts {
        for w in text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 1)
        {
            let h = fnv(&w.to_lowercase());
            let sign = if h >> 63 == 0 { 1.0 } else { -1.0 };
            v[(h % DIM as u64) as usize] += sign;
        }
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm == 0.0 {
        return vec![0; DIM];
    }
    v.iter()
        .map(|x| (x / norm * 127.0).round().clamp(-127.0, 127.0) as i8)
        .collect()
}

/// The cosine of two sketches; 0 for an empty one.
pub fn cosine(a: &[i8], b: &[i8]) -> f64 {
    let dot: i64 = a.iter().zip(b).map(|(x, y)| *x as i64 * *y as i64).sum();
    let n = |v: &[i8]| (v.iter().map(|x| (*x as i64).pow(2)).sum::<i64>() as f64).sqrt();
    let (na, nb) = (n(a), n(b));
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot as f64 / (na * nb)
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

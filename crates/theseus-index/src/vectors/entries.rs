//! The vector side's two maps by text hash, cut to what they must hold
//! (theseus-agqn): each was most of a chunk's heap beside its int8 cut.
//!
//! - [`Entries`]: a vector file's records by their text's hash. An
//!   open-addressed table of entry numbers (4 bytes a slot, at most three
//!   quarters full), keyed by the hashes the file's memory already holds, in
//!   place of a `HashMap<u128, u32>` (a 32-byte bucket, and a second copy of
//!   every hash).
//! - [`Holders`]: the rows holding one text. Nearly every text is one row's,
//!   held inline; a shared text's rows go in a `Vec`. In place of a `Vec<u32>`
//!   per text, a heap block of its own.

/// A slot with no entry.
const EMPTY: u32 = u32::MAX;

/// A vector file's entries by their hash (see the module's doc).
#[derive(Debug, Default)]
pub struct Entries {
    slots: Vec<u32>,
    len: usize,
}

impl Entries {
    /// Room for `n` entries without growing.
    pub fn with_capacity(n: usize) -> Entries {
        let mut cap = 16usize;
        while cap * 3 < n * 4 {
            cap *= 2;
        }
        Entries {
            slots: vec![EMPTY; cap],
            len: 0,
        }
    }

    /// Where `hash`'s probe starts: its bits mixed (Fibonacci hashing), the
    /// table's power of two from the top.
    fn start(&self, hash: u128) -> usize {
        let x = (hash as u64) ^ ((hash >> 64) as u64);
        let bits = self.slots.len().trailing_zeros();
        (x.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> (64 - bits)) as usize
    }

    /// The entry whose hash in `hashes` is `hash`.
    pub fn get(&self, hash: u128, hashes: &[u128]) -> Option<u32> {
        if self.slots.is_empty() {
            return None;
        }
        let mask = self.slots.len() - 1;
        let mut i = self.start(hash);
        loop {
            let e = self.slots[i];
            if e == EMPTY {
                return None;
            }
            if hashes[e as usize] == hash {
                return Some(e);
            }
            i = (i + 1) & mask;
        }
    }

    /// Entry `e`, whose hash is `hashes[e]`: it replaces an earlier entry of
    /// the same hash (the file's later record answers, as a map's insert).
    pub fn insert(&mut self, e: u32, hashes: &[u128]) {
        if (self.len + 1) * 4 > self.slots.len() * 3 {
            self.grow(hashes);
        }
        let hash = hashes[e as usize];
        let mask = self.slots.len() - 1;
        let mut i = self.start(hash);
        loop {
            let s = self.slots[i];
            if s == EMPTY {
                self.slots[i] = e;
                self.len += 1;
                return;
            }
            if hashes[s as usize] == hash {
                self.slots[i] = e;
                return;
            }
            i = (i + 1) & mask;
        }
    }

    fn grow(&mut self, hashes: &[u128]) {
        let old = std::mem::replace(self, Entries::with_capacity((self.len + 1) * 2));
        for e in old.slots.into_iter().filter(|&e| e != EMPTY) {
            self.insert(e, hashes);
        }
    }
}

/// The rows that hold one text (see the module's doc).
#[derive(Debug, Clone)]
pub enum Holders {
    One(u32),
    Many(Vec<u32>),
}

impl Default for Holders {
    fn default() -> Self {
        Holders::Many(Vec::new())
    }
}

impl Holders {
    pub fn push(&mut self, i: u32) {
        match self {
            Holders::Many(v) if v.is_empty() => *self = Holders::One(i),
            Holders::One(a) => *self = Holders::Many(vec![*a, i]),
            Holders::Many(v) => v.push(i),
        }
    }

    pub fn retain(&mut self, f: impl Fn(&u32) -> bool) {
        match self {
            Holders::One(a) => {
                if !f(a) {
                    *self = Holders::default();
                }
            }
            Holders::Many(v) => v.retain(f),
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Holders::Many(v) if v.is_empty())
    }

    pub fn first(&self) -> Option<&u32> {
        self.as_slice().first()
    }

    pub fn as_slice(&self) -> &[u32] {
        match self {
            Holders::One(a) => std::slice::from_ref(a),
            Holders::Many(v) => v,
        }
    }
}

impl<'a> IntoIterator for &'a Holders {
    type Item = &'a u32;
    type IntoIter = std::slice::Iter<'a, u32>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weights::SplitMix;
    use std::collections::HashMap;

    /// Against a map, over hashes that share their low bits and repeat: every
    /// lookup the same, and a repeated hash answers its latest entry.
    #[test]
    fn entries_answer_as_a_map_does() {
        let mut rng = SplitMix(41);
        let mut hashes: Vec<u128> = Vec::new();
        let mut e = Entries::default();
        let mut m: HashMap<u128, u32> = HashMap::new();
        for i in 0..20_000u32 {
            let h = if i % 9 == 4 {
                hashes[(rng.next_u64() % u64::from(i)) as usize]
            } else {
                u128::from(rng.next_u64() % 5000) << 8 | u128::from(i % 3)
            };
            hashes.push(h);
            e.insert(i, &hashes);
            m.insert(h, i);
        }
        for (&h, &i) in &m {
            assert_eq!(e.get(h, &hashes), Some(i));
        }
        assert_eq!(e.get(u128::MAX, &hashes), None);
        assert_eq!(e.len, m.len());
        assert!(Entries::default().get(7, &[]).is_none());
    }

    #[test]
    fn holders_hold_one_inline_and_many_in_a_vec() {
        assert!(std::mem::size_of::<Holders>() <= 24);
        let mut h = Holders::default();
        assert!(h.is_empty());
        h.push(3);
        assert!(matches!(h, Holders::One(3)));
        h.push(5);
        h.push(8);
        assert_eq!(h.as_slice(), &[3, 5, 8]);
        h.retain(|&x| x != 5);
        assert_eq!(h.as_slice(), &[3, 8]);
        h.retain(|_| false);
        assert!(h.is_empty() && h.first().is_none());
        let mut one = Holders::default();
        one.push(9);
        one.retain(|&x| x != 9);
        assert!(one.is_empty());
    }
}

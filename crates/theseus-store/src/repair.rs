//! Repairing corrupt frames from a copy of the store (theseus-15g). After the
//! history check finds a frame corrupt, reads of its records are refused,
//! and list reads skip them (R4). The repair takes each frame of the WAL that
//! does not check (its magic, its length, its crc) from a copy of the store
//! that holds it whole: a backup taken any time after that frame was
//! written, since the WAL only ever grows, and a frame once written never
//! moves. Every other byte is the store's own, so nothing written after the
//! copy is lost. A copy that does not hold the frame whole is refused, and
//! the store is left as it was.
//!
//! This module repairs a segment's bytes; `theseus_core::restore::repair`
//! stages the repaired WAL beside the store, opens it (a full replay checks
//! every frame and its positions), and swaps it in, keeping the store it
//! replaces.

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::wal::{decode_record, FRAME_HEADER, MAGIC};

/// One frame a repair took from the copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Patched {
    pub segment: u32,
    pub offset: u64,
    /// The whole frame's length, its header included.
    pub bytes: u64,
    /// The positions of its first and last records.
    pub first: u64,
    pub last: u64,
}

fn u32_le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// The length of the frame at `off` in `b`, its header included, when it
/// checks: its magic, a length inside `b`, and its crc.
fn frame_at(b: &[u8], off: usize) -> Option<usize> {
    if off.checked_add(FRAME_HEADER)? > b.len() || u32_le(b, off) != MAGIC {
        return None;
    }
    let len = u32_le(b, off + 4) as usize;
    let end = off.checked_add(FRAME_HEADER)?.checked_add(len)?;
    if end > b.len() {
        return None;
    }
    let crc = u32_le(b, off + 8);
    (crc32fast::hash(&b[off + FRAME_HEADER..end]) == crc).then_some(end - off)
}

/// The positions of the first and last records of the frame at `off`, `n`
/// bytes long, which checks.
fn positions(b: &[u8], off: usize, n: usize) -> Option<(u64, u64)> {
    let body = &b[off + FRAME_HEADER..off + n];
    let count = u32_le(body, 0) as usize;
    let mut i = 4;
    let mut first = None;
    let mut last = None;
    for _ in 0..count {
        let (r, used) = decode_record(body, i)?;
        first.get_or_insert(r.position);
        last = Some(r.position);
        i += used;
    }
    Some((first?, last?))
}

/// Segment `segment` of a store, `live`, with each frame that does not check
/// replaced by the frame at the same offset of `copy`, the same segment of a
/// copy of the store, which must check. What was patched comes with it.
pub fn repair_segment(segment: u32, live: &[u8], copy: &[u8]) -> Result<(Vec<u8>, Vec<Patched>)> {
    let mut out = live.to_vec();
    let mut patched = Vec::new();
    let mut off = 0usize;
    while off < live.len() {
        if let Some(n) = frame_at(live, off) {
            off += n;
            continue;
        }
        let Some(n) = frame_at(copy, off) else {
            bail!(
                "segment {segment}: the frame at offset {off} does not check, and the copy holds \
                 no whole frame there: it was taken before that frame was written, or it is a copy \
                 of another store"
            );
        };
        let end = off + n;
        if end > live.len() {
            bail!(
                "segment {segment}: the copy's frame at offset {off} runs past the end of this \
                 store's segment, a torn last frame, which a start cuts by itself"
            );
        }
        let (first, last) = positions(copy, off, n)
            .with_context(|| format!("segment {segment}: the copy's frame at offset {off}"))?;
        out[off..end].copy_from_slice(&copy[off..end]);
        patched.push(Patched {
            segment,
            offset: off as u64,
            bytes: n as u64,
            first,
            last,
        });
        off = end;
    }
    Ok((out, patched))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{kinds, NewRecord};
    use crate::store::{Store, WalStore};
    use crate::wal::WalConfig;

    /// A segment of four frames, one record each.
    fn segment() -> Vec<u8> {
        let dir = tempfile::tempdir().unwrap();
        let s = WalStore::open(dir.path(), WalConfig::default()).unwrap();
        for i in 0..4u32 {
            s.append(&[NewRecord::json(kinds::LEDGER, None, &[i; 8]).unwrap()])
                .unwrap();
        }
        drop(s);
        std::fs::read(dir.path().join("wal").join(format!("{:09}.seg", 1))).unwrap()
    }

    /// The offsets at which the segment's frames begin.
    fn frames(b: &[u8]) -> Vec<usize> {
        let mut out = vec![];
        let mut off = 0;
        while let Some(n) = frame_at(b, off) {
            out.push(off);
            off += n;
        }
        out
    }

    /// The second frame's body flipped, then its length too: each is taken
    /// whole from the copy, and the rest of the segment is the store's own.
    #[test]
    fn a_corrupt_frame_is_taken_whole_from_the_copy() {
        let copy = segment();
        let at = frames(&copy);
        assert_eq!(at.len(), 4);
        let mut live = copy.clone();
        let n = at[2] - at[1];
        live[at[1] + n - 1] ^= 0x01;
        let (fixed, patched) = repair_segment(1, &live, &copy).unwrap();
        assert_eq!(fixed, copy);
        assert_eq!(
            patched,
            [Patched {
                segment: 1,
                offset: at[1] as u64,
                bytes: n as u64,
                first: 2,
                last: 2
            }]
        );
        // A length field that lies: the copy's says where the frame ends.
        let mut live = copy.clone();
        live[at[1] + 4] ^= 0x10;
        assert_eq!(repair_segment(1, &live, &copy).unwrap().0, copy);
        // A whole segment needs nothing.
        assert!(repair_segment(1, &copy, &copy).unwrap().1.is_empty());
    }

    /// A copy taken before the bad frame was written holds no frame there,
    /// and is refused.
    #[test]
    fn a_copy_without_the_frame_is_refused() {
        let whole = segment();
        let at = frames(&whole);
        let mut live = whole.clone();
        live[at[3] + FRAME_HEADER] ^= 0x01;
        let before = &whole[..at[3]];
        let e = repair_segment(1, &live, before).unwrap_err();
        assert!(e.to_string().contains("holds no whole frame there"), "{e}");
    }
}

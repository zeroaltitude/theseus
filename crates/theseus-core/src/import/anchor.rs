//! Finding the stored end of a session in an episode the source renumbered
//! (theseus-ectzg): a compaction, a resume, a /reset moves every idx, so no
//! stored `(idx, sha256)` pair is where it was, and the position can only be
//! read from content.
//!
//! The rule is built to refuse. The anchor is the stored session's **last
//! two messages, adjacent and in order, occurring exactly once in the
//! episode**: a run of two, never a single digest (a repeated "ok" hashes
//! alike), at the stored end (the new turns are those after it), and unique
//! (a conversation that repeats itself has no one end). The episode must also
//! have **lost** a stored message: one that kept all of them and gained some
//! before the anchor is an insertion, whose inserted message the anchor
//! would drop, and it stays refused. No message after the anchor may repeat a
//! stored digest, because a copy of a stored message is indistinguishable from
//! the message itself and could be written twice.
//!
//! The failure modes accepted: a message the source added *before* the
//! anchor along with a compaction (the compaction's own summary turn, say) is
//! not imported; and a growth that repeats a stored text is refused until the
//! source's next rewrite. Both lose less than a wrong append would write.
//!
//! The new messages keep the source's text, time, author and digest but not
//! its idx, which would collide with a stored one: they take the idxs after
//! the highest stored one, in order.

use std::collections::{BTreeMap, BTreeSet};

use super::episode::Episode;

/// Where the new messages start, and the idx each takes.
pub(super) struct Anchor {
    /// The episode's messages before this index are stored already.
    pub from: usize,
    /// The stored idx of each message, by the episode's idx: the ones the
    /// store holds already, and the new ones.
    pub idx_of: BTreeMap<u32, u32>,
}

/// The anchor of `e` over the stored messages `stored` (idx and digest, in
/// the order written), or why there is none.
pub(super) fn find(stored: &[(u32, String)], e: &Episode) -> Result<Anchor, String> {
    let refuse = |why: &str| Err(format!("{why}: not appended over"));
    let [.., (_, pd), (_, ld)] = stored else {
        return refuse("fewer than two stored messages cannot anchor a renumbered episode");
    };
    let digests: Vec<&str> = e.messages.iter().map(|m| m.sha256.as_str()).collect();
    let ends: Vec<usize> = (1..digests.len())
        .filter(|&p| digests[p - 1] == pd && digests[p] == ld)
        .collect();
    let [end] = ends[..] else {
        return refuse(if ends.is_empty() {
            "the stored session's last two messages are not together in this renumbered episode"
        } else {
            "the stored session's last two messages occur more than once in this renumbered episode"
        });
    };
    let held: BTreeSet<&str> = stored.iter().map(|(_, d)| d.as_str()).collect();
    if stored.iter().all(|(_, d)| digests.contains(&d.as_str())) {
        return refuse(
            "every stored message is still here and others came before the end (an insertion)",
        );
    }
    if digests[end + 1..].iter().any(|d| held.contains(d)) {
        return refuse(
            "a message after the stored end repeats a stored one (a copy cannot be told from it)",
        );
    }
    let top = stored.iter().map(|(i, _)| *i).max().unwrap_or(0);
    let mut idx_of: BTreeMap<u32, u32> = BTreeMap::new();
    for m in &e.messages[..=end] {
        if let Some((i, _)) = stored.iter().find(|(_, d)| *d == m.sha256) {
            idx_of.insert(m.idx, *i);
        }
    }
    for (m, i) in e.messages[end + 1..].iter().zip(top + 1..) {
        idx_of.insert(m.idx, i);
    }
    Ok(Anchor {
        from: end + 1,
        idx_of,
    })
}

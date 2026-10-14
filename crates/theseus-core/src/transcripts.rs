//! Each session's transcript as last read, kept across turns (theseus-0l3g):
//! a turn's first reader takes the stubs kept for its session and reads only
//! the session's records after the last one it holds, where it read every
//! record of the session (its payloads included) at every turn. A long
//! session's turn read its whole history from the WAL each time.
//!
//! What is kept are cold stubs: a stub's fields (id, kind, origin, turn, a
//! summary's end) and its position, never its bytes nor its node. A reader
//! gets fresh cold stubs, so what a turn decodes is the turn's and the heat
//! cache's (`node_cache`), never pinned here. Bounded: at most
//! `MAX_SESSIONS` sessions and `MAX_STUBS` stubs, the least recently read
//! session going first. Nodes never change and a session's records are only
//! appended, so the kept stubs and those after them are the store's whole
//! read (debug builds compare the two at every turn's read, `Store::transcript`).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::store::Transcript;

/// Sessions kept at most.
pub const MAX_SESSIONS: usize = 256;
/// Stubs kept at most, across the sessions (about 150 bytes each).
pub const MAX_STUBS: usize = 400_000;

struct Entry {
    nodes: Transcript,
    /// The last position of the session's own records read.
    through: u64,
    used: u64,
}

#[derive(Default)]
pub struct Kept {
    inner: Mutex<Inner>,
    /// The records reads have scanned (tests).
    #[cfg(test)]
    pub scanned: std::sync::atomic::AtomicU64,
}

#[derive(Default)]
struct Inner {
    by: HashMap<String, Entry>,
    stubs: usize,
    tick: u64,
}

impl Kept {
    /// The kept stubs of `session` and the position they run through, taken
    /// out (a reader puts back what it read with `put`).
    pub fn take(&self, session: &str) -> Option<(Transcript, u64)> {
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let e = i.by.remove(session)?;
        i.stubs -= e.nodes.len();
        Some((e.nodes, e.through))
    }

    /// Keep `nodes` (cold stubs) of `session`, read through `through`; the
    /// least recently read sessions go past the bounds.
    pub fn put(&self, session: &str, nodes: Transcript, through: u64) {
        if nodes.len() > MAX_STUBS {
            return;
        }
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        i.tick += 1;
        let used = i.tick;
        i.stubs += nodes.len();
        if let Some(old) = i.by.insert(
            session.to_string(),
            Entry {
                nodes,
                through,
                used,
            },
        ) {
            i.stubs -= old.nodes.len();
        }
        while i.stubs > MAX_STUBS || i.by.len() > MAX_SESSIONS {
            let Some(oldest) =
                i.by.iter()
                    .min_by_key(|(_, e)| e.used)
                    .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some(e) = i.by.remove(&oldest) {
                i.stubs -= e.nodes.len();
            }
        }
    }

    /// The stubs and sessions kept now.
    pub fn size(&self) -> (usize, usize) {
        let i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        (i.stubs, i.by.len())
    }
}

#[cfg(test)]
mod tests {
    use crate::node::Node;
    use crate::store::Store;

    /// A turn's read keeps the session's stubs, cold, and the next turn's
    /// read takes them and reads only what was written after: the same
    /// transcript as a whole read, its nodes decoded only when touched.
    #[test]
    fn a_later_turn_reads_only_what_was_written_since() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path()).unwrap();
        let node = |sid: &str, text: &str| Node::user(sid, None, "op", text);
        let mut want = Vec::new();
        for i in 0..5 {
            let n = node("ses_k", &format!("m{i}"));
            let p = store.append(&[n.record().unwrap()]).unwrap()[0];
            want.push((p, n.id));
            store
                .append(&[node("ses_other", "x").record().unwrap()])
                .unwrap();
        }
        let ids = |t: &crate::store::Transcript| -> Vec<(u64, String)> {
            t.iter().map(|(p, n)| (*p, n.id.clone())).collect()
        };
        let scanned = || store.kept_scanned();
        let first = store.for_turn().transcript("ses_k").unwrap();
        assert_eq!(scanned(), 5);
        assert_eq!(ids(&first), want);
        let _: &Node = &first[0].1;
        assert_eq!(
            store.kept_transcripts(),
            (5, 1),
            "five stubs kept, of one session"
        );
        for i in 5..8 {
            let n = node("ses_k", &format!("m{i}"));
            let p = store.append(&[n.record().unwrap()]).unwrap()[0];
            want.push((p, n.id));
        }
        let second = store.for_turn().transcript("ses_k").unwrap();
        assert_eq!(ids(&second), want);
        assert_eq!(
            scanned(),
            5 + 3,
            "the second read scanned the three written since"
        );
        // Cold: the first read's decode is not the second's.
        assert!(!second[0].1.is_hydrated());
        let Node { body, .. } = &*second[6].1.node();
        assert_eq!(crate::stub::Kind::of(body), crate::stub::Kind::UserMessage);
        assert_eq!(store.kept_transcripts(), (8, 1));
    }
}

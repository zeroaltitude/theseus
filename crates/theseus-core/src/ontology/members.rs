//! The ontology snapshot's membership lists, read from the store when asked
//! (theseus-mem-daemon-rest).
//!
//! The snapshot holds the categories and what each is held by in counts, and
//! none of the 71,000 memberships a store with many imported sessions has: a
//! turn reads its own session's, recall reads a hit's session's, and a sweep
//! or a merge reads them all, which it does rarely. A session's lists are kept
//! after a read, `KEPT` sessions at most, oldest read out first, and forgotten
//! when a write puts one (`written`), so a cached answer is the store's.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use theseus_ontology::{keys, MemberList, MemberSource, Record};
use theseus_store::{kinds, Store as _};

use crate::store::Store;

/// Sessions whose lists are kept after a read: a few hundred bytes each.
pub const KEPT: usize = 2048;

/// The store's `onto:member:` records, as a snapshot's source.
pub struct StoreMembers {
    store: Store,
    kept: Mutex<Kept>,
    /// Counts the writes: a read keeps what it read only if no write came
    /// between its start and its keeping, since it may have read the lists
    /// the write replaced.
    writes: AtomicU64,
}

#[derive(Default)]
struct Kept {
    lists: HashMap<String, Vec<MemberList>>,
    /// The order the sessions were read in.
    order: VecDeque<String>,
}

impl fmt::Debug for StoreMembers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoreMembers").finish_non_exhaustive()
    }
}

impl StoreMembers {
    pub fn new(store: &Store) -> Self {
        Self {
            store: store.clone(),
            kept: Mutex::default(),
            writes: AtomicU64::new(0),
        }
    }

    /// What the store holds under `prefix`, as lists: a record that does not
    /// read is left out, as the load leaves it out.
    fn read(&self, prefix: &str) -> Vec<MemberList> {
        let records = match self.store.inner().latest_with_prefix(kinds::META, prefix) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(prefix, error = %e, "the ontology's memberships cannot be read");
                return Vec::new();
            }
        };
        records
            .into_iter()
            .filter_map(|r| {
                let key = r.key.clone().unwrap_or_default();
                let value = r.decode::<serde_json::Value>().ok()?;
                match Record::decode(&key, value) {
                    Ok(Record::Members(l)) => Some(l),
                    _ => None,
                }
            })
            .collect()
    }

    /// Keep `lists` for `session`, read after `seen` writes, unless a write
    /// has come since: they may be the lists it replaced.
    fn keep(&self, seen: u64, session: &str, lists: &[MemberList]) -> bool {
        let mut kept = self.kept.lock().unwrap();
        if self.writes.load(Ordering::Acquire) != seen {
            return false;
        }
        if kept
            .lists
            .insert(session.to_string(), lists.to_vec())
            .is_none()
        {
            kept.order.push_back(session.to_string());
            while kept.order.len() > KEPT {
                if let Some(old) = kept.order.pop_front() {
                    kept.lists.remove(&old);
                }
            }
        }
        true
    }

    /// The sessions kept now.
    #[cfg(test)]
    pub(crate) fn kept(&self) -> usize {
        self.kept.lock().unwrap().lists.len()
    }
}

impl MemberSource for StoreMembers {
    fn list(&self, session: &str, kind: &str) -> Option<MemberList> {
        self.of_session(session)
            .into_iter()
            .find(|l| l.kind == kind)
    }

    fn of_session(&self, session: &str) -> Vec<MemberList> {
        if let Some(lists) = self.kept.lock().unwrap().lists.get(session) {
            return lists.clone();
        }
        let seen = self.writes.load(Ordering::Acquire);
        // The id holds no `:`, so this prefix is this session's alone.
        let mut lists = self.read(&format!("{}{session}:", keys::MEMBERS));
        lists.sort_by(|a, b| a.kind.cmp(&b.kind));
        self.keep(seen, session, &lists);
        lists
    }

    fn all(&self) -> Vec<MemberList> {
        self.read(keys::MEMBERS)
    }

    fn written(&self, keys: &[(String, String)]) {
        let mut kept = self.kept.lock().unwrap();
        self.writes.fetch_add(1, Ordering::AcqRel);
        for (session, _) in keys {
            if kept.lists.remove(session).is_some() {
                kept.order.retain(|s| s != session);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::Board;
    use theseus_ontology::{Category, CategoryId, Membership, Origin};

    fn id(s: &str) -> CategoryId {
        CategoryId::parse(s).unwrap()
    }

    fn list(session: &str, categories: &[&str]) -> Record {
        Record::Members(MemberList {
            session: session.into(),
            kind: "topic".into(),
            members: categories
                .iter()
                .map(|c| Membership::operator(id(c), 10))
                .collect(),
        })
    }

    /// The board holds no list after a write; a session's lists are read from
    /// the store, kept for at most `KEPT` sessions, and a write to a session
    /// forgets what was kept of it, so the next read is the store's.
    #[test]
    fn a_session_is_read_from_the_store_kept_in_few_and_forgotten_on_a_write() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let board = Board::default();
        let mut records: Vec<Record> = ["a", "b"]
            .iter()
            .map(|n| Record::Category(Category::new(id(&format!("topic:{n}")), *n, "operator")))
            .collect();
        let sessions = KEPT + 50;
        records.extend((0..sessions).map(|i| list(&format!("s{i}"), &["topic:a"])));
        let o = board
            .write(&store, records, Origin::Operator, |_| Ok(Vec::new()))
            .unwrap();
        assert_eq!(
            o.held_lists(),
            0,
            "the store has them: the snapshot holds none"
        );
        assert_eq!(
            o.member_counts().get(&id("topic:a")),
            Some(&(sessions as u64))
        );

        let source = StoreMembers::new(&store);
        for i in 0..sessions {
            assert_eq!(source.of_session(&format!("s{i}")).len(), 1);
        }
        assert_eq!(source.kept(), KEPT, "the oldest read were let go");

        // A write to a session the board has read forgets what it kept.
        let before = board.snapshot(&store).unwrap();
        assert_eq!(before.memberships("s7").len(), 1);
        board
            .write(
                &store,
                vec![list("s7", &["topic:a", "topic:b"])],
                Origin::Operator,
                |_| Ok(Vec::new()),
            )
            .unwrap();
        let after = board.snapshot(&store).unwrap();
        assert_eq!(after.held_lists(), 0);
        assert_eq!(
            after.memberships("s7").len(),
            2,
            "the store's, not the one kept"
        );
        assert_eq!(after.member_counts().get(&id("topic:b")), Some(&1));
    }

    /// A read that began before a write must not keep what it read: it may
    /// be the lists the write replaced, and a later read would serve them.
    #[test]
    fn a_read_that_overlapped_a_write_keeps_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let source = StoreMembers::new(&store);
        let old = vec![MemberList {
            session: "s1".into(),
            kind: "topic".into(),
            members: Vec::new(),
        }];
        // The read starts, a write lands, then the read would keep its lists.
        let seen = source.writes.load(Ordering::Acquire);
        source.written(&[("s1".to_string(), "topic".to_string())]);
        assert!(!source.keep(seen, "s1", &old));
        assert_eq!(source.kept(), 0);
        // With no write in between it keeps them.
        let seen = source.writes.load(Ordering::Acquire);
        assert!(source.keep(seen, "s1", &old));
        assert_eq!(source.kept(), 1);
    }
}

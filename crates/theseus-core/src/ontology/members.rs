//! Lazy membership sources, stable for the lifetime of an ontology snapshot.
//! Before a write replaces a session's lists, each live generation keeps its
//! old lists. The next generation reads the new store; old snapshots never do.
//! Ordinary reads cache at most `KEPT` sessions. Historical overrides live only
//! as long as the snapshots that need them, and hold only changed sessions.

use crate::store::Store;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex, Weak};
use theseus_ontology::{keys, MemberList, MemberSource, Record};
use theseus_store::{kinds, Store as _};

pub const KEPT: usize = 2048;

pub struct StoreMembers {
    shared: Arc<Shared>,
    version: Arc<Mutex<Version>>,
}
struct Shared {
    store: Store,
    // Lock order: generations, then a version. Reads hold it through the
    // store read, so freezing cannot miss a read that overlaps the write.
    generations: Mutex<Vec<Weak<Mutex<Version>>>>,
}
#[derive(Default)]
struct Version {
    frozen: HashMap<String, Vec<MemberList>>,
    kept: HashMap<String, Vec<MemberList>>,
    order: VecDeque<String>,
}
impl fmt::Debug for StoreMembers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoreMembers").finish_non_exhaustive()
    }
}
impl StoreMembers {
    pub fn new(store: &Store) -> Self {
        Self::generation(Arc::new(Shared {
            store: store.clone(),
            generations: Mutex::default(),
        }))
    }
    fn generation(shared: Arc<Shared>) -> Self {
        let version = Arc::new(Mutex::new(Version::default()));
        theseus_store::blocking(|| shared.generations.lock().unwrap())
            .push(Arc::downgrade(&version));
        Self { shared, version }
    }
    fn read(&self, prefix: &str) -> Result<Vec<MemberList>, String> {
        let records = self
            .shared
            .store
            .inner()
            .latest_with_prefix(kinds::META, prefix)
            .map_err(|e| e.to_string())?;
        Ok(records
            .into_iter()
            .filter_map(|r| {
                let key = r.key.as_deref().unwrap_or_default();
                let value = r.decode::<serde_json::Value>().ok()?;
                match Record::decode(key, value) {
                    Ok(Record::Members(l)) => Some(l),
                    _ => None,
                }
            })
            .collect())
    }
    fn read_or_warn(&self, prefix: &str) -> Vec<MemberList> {
        self.read(prefix).unwrap_or_else(|e| {
            tracing::warn!(prefix, error = %e, "the ontology's memberships cannot be read");
            Vec::new()
        })
    }
    #[cfg(test)]
    fn kept(&self) -> usize {
        self.version.lock().unwrap().kept.len()
    }
}
impl MemberSource for StoreMembers {
    fn list(&self, session: &str, kind: &str) -> Option<MemberList> {
        self.of_session(session)
            .into_iter()
            .find(|l| l.kind == kind)
    }
    fn of_session(&self, session: &str) -> Vec<MemberList> {
        let _generations = theseus_store::blocking(|| self.shared.generations.lock().unwrap());
        let mut version = self.version.lock().unwrap();
        if let Some(lists) = version
            .frozen
            .get(session)
            .or_else(|| version.kept.get(session))
        {
            return lists.clone();
        }
        let mut lists = self.read_or_warn(&format!("{}{session}:", keys::MEMBERS));
        lists.sort_by(|a, b| a.kind.cmp(&b.kind));
        version.kept.insert(session.into(), lists.clone());
        version.order.push_back(session.into());
        while version.order.len() > KEPT {
            if let Some(old) = version.order.pop_front() {
                version.kept.remove(&old);
            }
        }
        lists
    }
    fn all(&self) -> Vec<MemberList> {
        let _generations = theseus_store::blocking(|| self.shared.generations.lock().unwrap());
        let version = self.version.lock().unwrap();
        let mut lists: BTreeMap<_, _> = self
            .read_or_warn(keys::MEMBERS)
            .into_iter()
            .filter(|l| !version.frozen.contains_key(&l.session))
            .map(|l| ((l.session.clone(), l.kind.clone()), l))
            .collect();
        for l in version.frozen.values().flatten() {
            lists.insert((l.session.clone(), l.kind.clone()), l.clone());
        }
        lists.into_values().collect()
    }
    fn before_write(&self, keys: &[(String, String)]) -> Result<(), String> {
        let mut generations = theseus_store::blocking(|| self.shared.generations.lock().unwrap());
        let mut old = BTreeMap::new();
        for (session, _) in keys {
            if let std::collections::btree_map::Entry::Vacant(entry) = old.entry(session.clone()) {
                entry.insert(self.read(&format!("{}{session}:", keys::MEMBERS))?);
            }
        }
        generations.retain(|weak| {
            let Some(version) = weak.upgrade() else {
                return false;
            };
            let mut version = version.lock().unwrap();
            for (session, lists) in &old {
                version
                    .frozen
                    .entry(session.clone())
                    .or_insert_with(|| lists.clone());
                version.kept.remove(session);
            }
            version.order.retain(|s| !old.contains_key(s));
            true
        });
        Ok(())
    }
    fn after_write(&self) -> Option<Arc<dyn MemberSource>> {
        Some(Arc::new(Self::generation(self.shared.clone())))
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

    /// New snapshots read the store and cache at most `KEPT` sessions; older
    /// snapshots retain their own generation.
    #[test]
    fn a_session_is_read_from_the_store_and_kept_in_few_per_generation() {
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

        // A write gives the next snapshot a new source generation.
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

    /// Freeze before the store changes, including a session no reader has
    /// touched. The next source sees the write while this source stays old.
    #[test]
    fn a_read_that_overlapped_a_write_keeps_its_generation() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let source = StoreMembers::new(&store);
        source
            .before_write(&[("s1".into(), "topic".into())])
            .unwrap();
        let record = list("s1", &["topic:a"]);
        store
            .append(&[theseus_store::NewRecord::json(
                kinds::META,
                Some(&record.key()),
                &record.to_value(),
            )
            .unwrap()])
            .unwrap();
        assert!(source.of_session("s1").is_empty());
        assert_eq!(source.after_write().unwrap().of_session("s1").len(), 1);
    }

    #[test]
    fn old_snapshots_keep_cached_and_unread_memberships() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let board = Board::default();
        let before = board
            .write(
                &store,
                vec![
                    Record::Category(Category::new(id("topic:a"), "A", "operator")),
                    Record::Guidance(theseus_ontology::Guidance::new(
                        id("topic:a"),
                        "Keep the harbour chart current.",
                        1,
                        "operator",
                    )),
                    list("cached", &["topic:a"]),
                    list("unread", &["topic:a"]),
                ],
                Origin::Operator,
                |_| Ok(vec![]),
            )
            .unwrap();
        assert_eq!(before.memberships("cached")[0].category, id("topic:a"));
        let after = board
            .write(
                &store,
                vec![
                    Record::Category(Category::new(id("topic:b"), "B", "operator")),
                    list("cached", &["topic:b"]),
                    list("unread", &["topic:b"]),
                    list("new", &["topic:b"]),
                ],
                Origin::Operator,
                |_| Ok(vec![]),
            )
            .unwrap();
        for session in ["cached", "unread"] {
            assert_eq!(before.memberships(session)[0].category, id("topic:a"));
            assert_eq!(after.memberships(session)[0].category, id("topic:b"));
        }
        assert!(before.memberships("new").is_empty());
        assert!(before.category(&id("topic:b")).is_none());
        assert_eq!(before.member_counts().get(&id("topic:a")), Some(&2));
        assert_eq!(before.lists().count(), 2);
        let walk = crate::ontology::Walk::of(before.clone(), "unread", None, 10).unwrap();
        assert!(walk
            .compose(&walk.current)
            .render()
            .contains("Keep the harbour chart current."));
        assert_eq!(after.member_counts().get(&id("topic:b")), Some(&3));
        let staged = board
            .stage(&store, Origin::Operator, |_| {
                Ok((vec![list("unread", &["topic:a"])], vec![]))
            })
            .unwrap()
            .unwrap();
        store.append(staged.frame()).unwrap();
        staged.commit();
        assert_eq!(after.memberships("unread")[0].category, id("topic:b"));
        assert_eq!(before.memberships("unread")[0].category, id("topic:a"));
        assert_eq!(
            board.snapshot(&store).unwrap().memberships("unread")[0].category,
            id("topic:a")
        );
    }
}

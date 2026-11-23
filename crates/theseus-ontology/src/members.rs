//! Where a snapshot holds its membership lists (theseus-mem-daemon-rest).
//!
//! A list per session and kind is most of what an ontology holds on a store
//! with many imported sessions: 414 categories and 71,000 memberships over
//! 51,000 sessions were 15 to 25 MiB the daemon kept for hours, and a turn
//! reads one session's. So a snapshot may hold none of them, and read a list
//! from where it is stored when something asks ([`MemberSource`]):
//!
//! - **Held** (no source): every list is in memory. What a test, a trial
//!   copy and a caller with no store have.
//! - **Lean** (a source): the categories' counts and the times of their
//!   memberships are in memory, and a list is read when asked. Only what the store
//!   does not say is held: the lists [`load`](crate::Ontology::load) trimmed
//!   (`held`) or dropped (`hidden`), and the lists a write has put but the
//!   store has not yet been given (`staged`, until `settle`).
//!
//! Every answer is the held snapshot's: the same lists, in the same order.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use crate::category::{CategoryId, MemberList, Membership};

/// A store of membership lists, as written.
pub trait MemberSource: Send + Sync + fmt::Debug {
    /// The list stored for a session and kind.
    fn list(&self, session: &str, kind: &str) -> Option<MemberList>;
    /// Every list stored for a session, of every kind, in kind order.
    fn of_session(&self, session: &str) -> Vec<MemberList>;
    /// Every list stored.
    fn all(&self) -> Vec<MemberList>;
    /// Freeze old versions before the caller appends these keys to its store.
    fn before_write(&self, _keys: &[(String, String)]) -> Result<(), String> {
        Ok(())
    }
    /// A source for the new snapshot after the append. Existing sources stay
    /// valid for older snapshots. In-memory test sources may use `written`.
    fn after_write(&self) -> Option<Arc<dyn MemberSource>> {
        None
    }
    /// A write put these lists: forget what was kept of them.
    fn written(&self, _keys: &[(String, String)]) {}
}

type Key = (String, String);

/// What a category's memberships come to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Tally {
    /// The sessions that hold it: a list holds a category once.
    sessions: u64,
    /// How many memberships there are at each `as_of_ms`: the newest is the
    /// last, and stays right when the newest is dropped.
    times: BTreeMap<u64, u32>,
}

#[derive(Debug, Clone)]
struct Lean {
    source: Arc<dyn MemberSource>,
    tally: BTreeMap<CategoryId, Tally>,
    /// Lists the source holds that this snapshot does not.
    hidden: BTreeSet<Key>,
    /// Keys put since the last `settle`.
    staged: BTreeSet<Key>,
}

/// The membership lists of a snapshot, by session and then kind.
#[derive(Debug, Clone, Default)]
pub(crate) struct Members {
    /// Every list when held; when lean, the lists that differ from the
    /// source's.
    held: BTreeMap<Key, MemberList>,
    lean: Option<Lean>,
}

impl PartialEq for Members {
    fn eq(&self, other: &Self) -> bool {
        let owned = |m: &Members| m.all().map(Cow::into_owned).collect::<Vec<_>>();
        owned(self) == owned(other)
    }
}

impl Members {
    /// Lists read from `source`, none held yet.
    pub(crate) fn lean(source: Arc<dyn MemberSource>) -> Self {
        Members {
            held: BTreeMap::new(),
            lean: Some(Lean {
                source,
                tally: BTreeMap::new(),
                hidden: BTreeSet::new(),
                staged: BTreeSet::new(),
            }),
        }
    }

    pub(crate) fn get(&self, session: &str, kind: &str) -> Option<Cow<'_, MemberList>> {
        let key = (session.to_string(), kind.to_string());
        if let Some(l) = self.held.get(&key) {
            return Some(Cow::Borrowed(l));
        }
        let lean = self.lean.as_ref()?;
        if lean.hidden.contains(&key) {
            return None;
        }
        lean.source.list(session, kind).map(Cow::Owned)
    }

    /// A session's lists, every kind's, in kind order.
    pub(crate) fn of_session(&self, session: &str) -> Vec<Cow<'_, MemberList>> {
        let Some(lean) = &self.lean else {
            return self
                .held
                .range((session.to_string(), String::new())..)
                .take_while(|((s, _), _)| s == session)
                .map(|(_, l)| Cow::Borrowed(l))
                .collect();
        };
        let mut lists: BTreeMap<String, Cow<'_, MemberList>> = BTreeMap::new();
        for l in lean.source.of_session(session) {
            let key = (l.session.clone(), l.kind.clone());
            if !lean.hidden.contains(&key) && !self.held.contains_key(&key) {
                lists.insert(l.kind.clone(), Cow::Owned(l));
            }
        }
        for (_, l) in self
            .held
            .range((session.to_string(), String::new())..)
            .take_while(|((s, _), _)| s == session)
        {
            lists.insert(l.kind.clone(), Cow::Borrowed(l));
        }
        lists.into_values().collect()
    }

    /// Every list, in key order.
    pub(crate) fn all(&self) -> Box<dyn Iterator<Item = Cow<'_, MemberList>> + '_> {
        let Some(lean) = &self.lean else {
            return Box::new(self.held.values().map(Cow::Borrowed));
        };
        let mut lists: BTreeMap<Key, Cow<'_, MemberList>> = BTreeMap::new();
        for l in lean.source.all() {
            let key = (l.session.clone(), l.kind.clone());
            if !lean.hidden.contains(&key) && !self.held.contains_key(&key) {
                lists.insert(key, Cow::Owned(l));
            }
        }
        for (key, l) in &self.held {
            lists.insert(key.clone(), Cow::Borrowed(l));
        }
        Box::new(lists.into_values())
    }

    /// A list the load found in the store: `kept` is what checked, and
    /// `trimmed` whether it is less than what is stored.
    pub(crate) fn loaded(&mut self, kept: MemberList, trimmed: bool) {
        let Some(lean) = &mut self.lean else {
            self.held
                .insert((kept.session.clone(), kept.kind.clone()), kept);
            return;
        };
        for m in &kept.members {
            lean.tally_add(m);
        }
        if trimmed {
            self.held
                .insert((kept.session.clone(), kept.kind.clone()), kept);
        }
    }

    /// A list the load found in the store and left out.
    pub(crate) fn dropped(&mut self, session: &str, kind: &str) {
        if let Some(lean) = &mut self.lean {
            lean.hidden.insert((session.to_string(), kind.to_string()));
        }
    }

    /// Hold `list` in place of the one it supersedes.
    pub(crate) fn put(&mut self, list: MemberList) {
        let key = (list.session.clone(), list.kind.clone());
        let old = match self.lean {
            Some(_) => self.get(&list.session, &list.kind).map(Cow::into_owned),
            None => None,
        };
        if let Some(lean) = &mut self.lean {
            for m in old.iter().flat_map(|l| &l.members) {
                lean.tally_remove(m);
            }
            for m in &list.members {
                lean.tally_add(m);
            }
            lean.hidden.remove(&key);
            lean.staged.insert(key.clone());
        }
        self.held.insert(key, list);
    }

    pub(crate) fn before_write(&self) -> Result<(), String> {
        if let Some(lean) = &self.lean {
            if !lean.staged.is_empty() {
                lean.source
                    .before_write(&lean.staged.iter().cloned().collect::<Vec<_>>())?;
            }
        }
        Ok(())
    }

    /// The store has what was put: it speaks for those lists again.
    pub(crate) fn settle(&mut self) {
        let Some(lean) = &mut self.lean else { return };
        let keys: Vec<Key> = std::mem::take(&mut lean.staged).into_iter().collect();
        for k in &keys {
            self.held.remove(k);
        }
        if !keys.is_empty() {
            lean.source.written(&keys);
            if let Some(source) = lean.source.after_write() {
                lean.source = source;
            }
        }
    }

    pub(crate) fn held_lists(&self) -> usize {
        self.held.len()
    }

    /// How many sessions hold `id`.
    pub(crate) fn holders(&self, id: &CategoryId) -> u64 {
        match &self.lean {
            Some(lean) => lean.tally.get(id).map_or(0, |t| t.sessions),
            None => self
                .held
                .values()
                .filter(|l| l.members.iter().any(|m| &m.category == id))
                .count() as u64,
        }
    }

    /// How many sessions hold each category.
    pub(crate) fn counts(&self) -> BTreeMap<CategoryId, u64> {
        match &self.lean {
            Some(lean) => lean
                .tally
                .iter()
                .map(|(c, t)| (c.clone(), t.sessions))
                .collect(),
            None => {
                let mut out: BTreeMap<CategoryId, u64> = BTreeMap::new();
                for m in self.held.values().flat_map(|l| &l.members) {
                    *out.entry(m.category.clone()).or_default() += 1;
                }
                out
            }
        }
    }

    /// When each category was last held: its newest membership's `as_of_ms`.
    pub(crate) fn newest(&self) -> BTreeMap<CategoryId, u64> {
        if let Some(lean) = &self.lean {
            return lean
                .tally
                .iter()
                .filter_map(|(c, t)| Some((c.clone(), *t.times.keys().next_back()?)))
                .collect();
        }
        let mut out: BTreeMap<CategoryId, u64> = BTreeMap::new();
        for m in self.held.values().flat_map(|l| &l.members) {
            let at = out.entry(m.category.clone()).or_default();
            *at = (*at).max(m.as_of_ms);
        }
        out
    }

    /// The lists of `kind` that hold at least one membership.
    pub(crate) fn nonempty_of_kind(&self, kind: &str) -> usize {
        self.all()
            .filter(|l| l.kind == kind && !l.members.is_empty())
            .count()
    }
}

impl Lean {
    fn tally_add(&mut self, m: &Membership) {
        let t = self.tally.entry(m.category.clone()).or_default();
        t.sessions += 1;
        *t.times.entry(m.as_of_ms).or_default() += 1;
    }

    fn tally_remove(&mut self, m: &Membership) {
        let Some(t) = self.tally.get_mut(&m.category) else {
            return;
        };
        t.sessions = t.sessions.saturating_sub(1);
        if let Some(n) = t.times.get_mut(&m.as_of_ms) {
            *n -= 1;
            if *n == 0 {
                t.times.remove(&m.as_of_ms);
            }
        }
        if t.sessions == 0 {
            self.tally.remove(&m.category);
        }
    }
}

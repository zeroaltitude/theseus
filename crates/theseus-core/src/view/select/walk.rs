//! Several timelines walked newest first as one, within one allowance of references read: one person's
//! handles (the people block's recent lines, `super::people`), or every person's timeline at once
//! ([`Walk::every_person`]), which is every exchange a person opened: the ledger touches a person's root
//! beside the place's for each of them, and a wake opens none. Bounded: the walk stops when its allowance is
//! spent and says so ([`Walk::stopped`]); it never reads further, and never falls back to another walk.

use std::collections::{BTreeSet, VecDeque};

use super::super::history::PAGE_EXCHANGES;
use super::super::timeline::{Cursor, ExchangeRef, TimelineRoot};
use super::Snapshot;

/// Timelines walked newest first as one: by the touch index's own order (`Cursor`), each exchange once, in
/// bounded pages, every reference read charged to one allowance.
pub struct Walk<'s> {
    snap: &'s dyn Snapshot,
    lanes: Vec<Lane>,
    seen: BTreeSet<String>,
    /// The references still to be read.
    left: usize,
    /// The allowance was spent with a timeline not yet read to its end.
    stopped: bool,
}

struct Lane {
    root: TimelineRoot,
    before: Option<Cursor>,
    page: VecDeque<ExchangeRef>,
    done: bool,
}

impl<'s> Walk<'s> {
    /// A walk over `roots`, reading at most `allowance` references in all.
    pub fn new(
        snap: &'s dyn Snapshot,
        roots: impl IntoIterator<Item = TimelineRoot>,
        allowance: usize,
    ) -> Self {
        let lanes = (roots.into_iter())
            .map(|root| Lane {
                root,
                before: None,
                page: VecDeque::new(),
                done: false,
            })
            .collect();
        Self {
            snap,
            lanes,
            seen: BTreeSet::new(),
            left: allowance,
            stopped: false,
        }
    }

    /// Every person's timeline as one (`Timeline::persons`), reading at most `allowance` references of
    /// them; listing the persons is one read a person, outside the allowance.
    pub fn every_person(snap: &'s dyn Snapshot, allowance: usize) -> anyhow::Result<Self> {
        let persons = snap.timeline().persons()?;
        Ok(Self::new(
            snap,
            persons.into_iter().map(TimelineRoot::Person),
            allowance,
        ))
    }

    /// Whether the walk ended because its allowance was spent with a timeline not yet read to its end: what
    /// it gave is the newest, and older exchanges were not read.
    pub fn stopped(&self) -> bool {
        self.stopped
    }

    /// The next touch, newest first across every timeline; `None` when all are read, or when a timeline's next
    /// page is wanted and the allowance is spent ([`Walk::stopped`]). Each page asked is sized by what is left,
    /// shared among the timelines that want one. A touch already met on another timeline is still a touch
    /// read: `Some(None)`.
    pub fn next(&mut self) -> anyhow::Result<Option<Option<ExchangeRef>>> {
        let mut wanting = (self.lanes.iter())
            .filter(|l| l.page.is_empty() && !l.done)
            .count();
        for l in self
            .lanes
            .iter_mut()
            .filter(|l| l.page.is_empty() && !l.done)
        {
            if self.left == 0 {
                self.stopped = true;
                return Ok(None);
            }
            let most = (self.left / wanting).clamp(1, PAGE_EXCHANGES);
            wanting -= 1;
            let page = (self.snap.timeline()).newest(&l.root, l.before.as_ref(), most)?;
            self.left = self.left.saturating_sub(page.len());
            l.done = page.is_empty();
            l.before = page.last().map(ExchangeRef::cursor);
            l.page = page.into();
        }
        let newest = (self.lanes.iter_mut())
            .filter(|l| !l.page.is_empty())
            .max_by(|a, b| a.page[0].cursor().cmp(&b.page[0].cursor()));
        let Some(e) = newest.and_then(|l| l.page.pop_front()) else {
            return Ok(None);
        };
        Ok(Some(self.seen.insert(e.turn_id.clone()).then_some(e)))
    }
}

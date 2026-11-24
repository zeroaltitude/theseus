//! A stored piece's source, read again today ([`super::show::Source`]): a view record is a map of what WAS
//! admitted, and provenance that cannot be read any more keeps a piece out of every reader. The check is the
//! gate's own: the source session's marks (`view_gate::session_marks`: its place and today's reading of it)
//! against a point of view through `view_gate::Gate::check`. Anything goes (2026-11-22): who is present, a
//! seal, a move or a veil holds nothing.
//!
//! - An exchange range: each exchange's session, through its exchange record (`exchange.<turn>`) for a
//!   range of one, and through the place's touch index for a longer one, read back at most
//!   [`RANGE_MAX`] exchanges; a range that does not resolve whole is not shown.
//! - Code's own text (the frame, the clock): every reader's; it has no source.
//! - A named thing's card and a waiting record: every session it was read from (its `from`, ids only)
//!   readable today; the facts about the person: shown.
//! - A recalled item: its session's marks through the gate.
//! - A fold: today's node, found by the piece's handle, of the range's place, level and ends, and every
//!   exchange it stands for resolved (`ladder::fold::constituents`, the model's assembly's rule), each of
//!   that place and its session clear.
//! - A task's card: the task's record read today, and its home session (`task_graph::home`) clear.
//!
//! This is not the assembly's whole scope check: a self-loop's permit (`view::permit`) and a fork's cut
//! (`fork::window`) are kept by what the assembly lays, and are not applied again here.
//!
//! What a listing promises. This reader is a read-only listing, and a source lives for ONE call (one
//! `view.show`, one preview, one look): it is made for that call and dropped when the call answers. Every
//! answer in one call's listing was true at some moment during that call. A write that lands while the call
//! runs may or may not be reflected, as with any read that races a write (a proof is several reads of the
//! store, not one snapshot). Nothing proved in one call is reused by another, so a write that finished before
//! a call began is in that call.
//!
//! A fold's proof reads its node and every exchange it stands for, so a source keeps what it proved (its
//! memo): one source serves every view of a call, and a node is proved once however many views hold it.
//! Each answer is kept with the store's last log position when it was proved (`Store::last_position`), and a
//! later view of the same call proves it again when that position has moved. The stamp is a best-effort
//! freshener, not a guarantee: it notices a write the reader could already see between two views, but it does
//! not mark when a write is published to this reader. Two gaps lie between them. The store's index publishes
//! a write after the log position has moved, so a proof made in that gap reads the old index and is kept under
//! the new position. A place binding is published (in the outbox's memory) after its record is written, and
//! publishing it moves no position. In both gaps the kept answer was still true at a moment of the call.
//! Reading the stamp may wait while the log writer appends: the store offers no read of it that never waits.
//!
//! The listing equals the stored tally's slots only while every source still reads: the tally is history,
//! and a source gone since is not listed (a dropped one is taken out of its band's residue).
//!
//! What a proof costs is counted in units (a fold's node and each exchange it stands for, the task list once),
//! not in the store's own reads: a node is found through its handle's record first, each session's place is
//! read, and the task list reads every task.
//! A source given a budget of units ([`GateSource::within`]) leaves a fold or a card it has no units left for
//! unchecked: not shown, and counted ([`GateSource::unchecked`]).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};

use theseus_memory::gate::{PointOfView, Veil};

use super::super::{AtHand, Handle, Marks, Provenance, SourceRange, Space};
use super::show::Source;
use crate::exchange::{self, Root};
use crate::turn::TurnRunner;
use crate::view_gate::{self, Gate};

/// The most exchanges of one range read back to prove its sources.
pub const RANGE_MAX: usize = 200;

/// Today's sources, for one reader.
pub struct GateSource<'a> {
    pub runner: &'a TurnRunner,
    /// The reader's point of view: the turn's verified one for the model, the owner alone for his own
    /// inspection.
    pub pov: PointOfView,
    /// The reader's own session, which a seal of its own thread does not hold out; none for the owner's
    /// inspection, so no seal is lifted by inspecting.
    pub asker: Option<&'a str>,
    /// The most units of proof this source spends; none for no bound.
    budget: Option<u64>,
    memo: Memo,
}

/// An answer, with the store's last position when it was proved; `None` when it was left unchecked.
type Kept = (u64, Option<bool>);

/// What a source has proved, the units it spent proving folds and task cards, and what it left unchecked.
#[derive(Default)]
struct Memo {
    /// By the fold's handle and its range.
    folds: RefCell<HashMap<(Handle, SourceRange), Kept>>,
    /// By task id.
    tasks: RefCell<HashMap<String, Kept>>,
    /// Every task, read again when the store's last position has moved; none when they cannot be read.
    all: RefCell<Option<(u64, Option<Vec<crate::task_graph::TaskRecord>>)>>,
    reads: Cell<u64>,
    unchecked: Cell<u64>,
}

impl<'a> GateSource<'a> {
    /// A source for ONE call (one `view.show`, one preview, one look): made for the call and dropped when it
    /// answers, never kept, nor its memo, across calls. What it lists was true at some moment of the call; its
    /// stamp is a best-effort freshener within the call, not a guarantee (the module's head says why).
    pub fn new(runner: &'a TurnRunner, pov: PointOfView, asker: Option<&'a str>) -> Self {
        Self {
            runner,
            pov,
            asker,
            budget: None,
            memo: Memo::default(),
        }
    }

    /// [`Self::new`], for one call as it says, spending at most `budget` units of proof.
    pub fn within(
        runner: &'a TurnRunner,
        pov: PointOfView,
        asker: Option<&'a str>,
        budget: u64,
    ) -> Self {
        Self {
            budget: Some(budget),
            ..Self::new(runner, pov, asker)
        }
    }

    /// The units spent so far proving folds and task cards: a fold's node and each exchange it stands for,
    /// and the task list once each time it is read. Units, not the store's own reads.
    pub fn reads(&self) -> u64 {
        self.memo.reads.get()
    }

    /// The pieces, folds and task cards, left unchecked because the budget was spent.
    pub fn unchecked(&self) -> u64 {
        self.memo.unchecked.get()
    }

    /// Spend `n` units, if the budget holds them.
    fn spend(&self, n: usize) -> bool {
        let after = self.memo.reads.get() + n as u64;
        if self.budget.is_some_and(|b| after > b) {
            return false;
        }
        self.memo.reads.set(after);
        true
    }

    /// The store's last log position: the stamp an answer is kept with, a best-effort freshener.
    fn at(&self) -> u64 {
        self.runner.store.last_position()
    }

    /// An answer as shown: an unchecked one is no, and counted.
    fn shown(&self, ok: Option<bool>) -> bool {
        if ok.is_none() {
            self.memo.unchecked.set(self.memo.unchecked.get() + 1);
        }
        ok == Some(true)
    }
}

impl GateSource<'_> {
    /// Whether `session`'s marks pass the gate for this reader: its place reads, and today's reading of it.
    fn clear(&self, session: &str) -> bool {
        let m = view_gate::session_marks(self.runner, self.asker, session, Veil::Clear);
        crate::view::sources_names::place_reads(m.place.as_deref())
            && Gate::check(&self.pov, &m).is_ok()
    }

    /// Whether a card's or a waiting line's provenance holds today: every session it read, and every
    /// exchange it read, readable.
    fn proven(&self, p: &Provenance) -> bool {
        p.sessions.iter().all(|s| self.clear(s))
            && p.exchanges
                .iter()
                .all(|id| match exchange::exchange(&self.runner.store, id) {
                    Ok(Some(e)) => self.clear(&e.session_id),
                    _ => false,
                })
    }

    /// The sessions behind an exchange range of `place`; none when they cannot all be proven.
    fn sessions(&self, place: &str, range: &SourceRange) -> Option<BTreeSet<String>> {
        let store = &self.runner.store;
        let one = |turn: &str| -> Option<String> {
            let e = exchange::exchange(store, turn).ok()??;
            (e.place == place).then_some(e.session_id)
        };
        let mut out = BTreeSet::from([one(&range.first.id)?, one(&range.last.id)?]);
        if range.first.id == range.last.id {
            return Some(out);
        }
        let before = range.last.at_ms.checked_add(1)?;
        let page = exchange::newest_before(store, Root::Place(place), before, RANGE_MAX).ok()?;
        let reached = page.iter().any(|t| t.at_ms < range.first.at_ms) || page.len() < RANGE_MAX;
        if !reached {
            return None;
        }
        for t in page.into_iter().filter(|t| t.at_ms >= range.first.at_ms) {
            if !matches!(exchange::exchange(store, &t.exchange), Ok(Some(_))) {
                return None;
            }
            out.insert(t.session_id);
        }
        Some(out)
    }

    /// Whether fold `handle` over `range` of `place` at `level` is proven today: once per source, and again
    /// when the store's last position has moved.
    fn fold(&self, (place, level): (&str, u8), range: &SourceRange, handle: &Handle) -> bool {
        let key = (handle.clone(), range.clone());
        let at = self.at();
        let ok = match self.memo.folds.borrow().get(&key) {
            Some(&(then, ok)) if then == at => Some(ok),
            _ => None,
        };
        let ok = ok.unwrap_or_else(|| {
            let ok = self.fold_read((place, level), range, handle);
            self.memo.folds.borrow_mut().insert(key, (at, ok));
            ok
        });
        self.shown(ok)
    }

    /// Today's node by its handle: of the range's place, level and ends, every exchange it stands for
    /// resolved and of that place, and each session clear; `None` when the budget does not hold its proof.
    fn fold_read(
        &self,
        (place, level): (&str, u8),
        range: &SourceRange,
        handle: &Handle,
    ) -> Option<bool> {
        let store = &self.runner.store;
        if !self.spend(1) {
            return None;
        }
        #[cfg(test)]
        self.witness(false);
        let Ok(Some(node)) = crate::ladder::fold::node(store, &handle.0) else {
            return Some(false);
        };
        let ends = (node.exchanges.first(), node.exchanges.last());
        if node.place != place
            || node.level != level
            || ends != (Some(&range.first.id), Some(&range.last.id))
        {
            return Some(false);
        }
        if !self.spend(node.exchanges.len()) {
            return None;
        }
        let Some(all) = crate::ladder::fold::whole(store, &node) else {
            return Some(false);
        };
        let sessions: BTreeSet<&str> = all.iter().map(|e| e.session_id.as_str()).collect();
        Some(all.iter().all(|e| e.place == place) && sessions.into_iter().all(|s| self.clear(s)))
    }

    /// Whether task `id`'s card is proven today: its record reads and its home session is clear. Once per
    /// source, and again when the store's last position has moved.
    fn task(&self, id: &str) -> bool {
        let at = self.at();
        let ok = match self.memo.tasks.borrow().get(id) {
            Some(&(then, ok)) if then == at => Some(ok),
            _ => None,
        };
        let ok = ok.unwrap_or_else(|| {
            let ok = self.task_read(id, at);
            self.memo.tasks.borrow_mut().insert(id.into(), (at, ok));
            ok
        });
        self.shown(ok)
    }

    /// [`Self::task`]'s proof, with the task list read again when the store has moved since it was read.
    fn task_read(&self, id: &str, at: u64) -> Option<bool> {
        let mut all = self.memo.all.borrow_mut();
        if all.as_ref().is_none_or(|(then, _)| *then != at) {
            if !self.spend(1) {
                return None;
            }
            #[cfg(test)]
            self.witness(true);
            *all = Some((at, crate::task_graph::all(&self.runner.store).ok()));
        }
        let tasks = all.as_ref().and_then(|(_, tasks)| tasks.as_deref());
        let ok = tasks
            .and_then(|all| {
                Some(crate::task_graph::home(
                    all,
                    all.iter().find(|t| t.id == id)?,
                ))
            })
            .is_some_and(|home| self.clear(&home));
        Some(ok)
    }
}

impl Source for GateSource<'_> {
    fn may_show(&self, range: &SourceRange, _marks: Option<&Marks>) -> bool {
        let place = match &range.space {
            Space::Code { .. } | Space::Facts { .. } => return true,
            Space::Exchanges { place } => place,
            // A named thing's card and what waits: its provenance proven today.
            Space::Card { from, .. } | Space::Waiting { from, .. } => {
                return from.as_ref().is_some_and(|p| self.proven(p))
            }
            Space::Recalled { session, .. } => return self.clear(session),
            // A fold and a task's card are proven by their handle and id (`may_show_piece`).
            Space::Folds { .. } | Space::Task { .. } => return false,
        };
        let Some(sessions) = self.sessions(place, range) else {
            return false;
        };
        sessions.iter().all(|s| self.clear(s))
    }

    fn may_show_piece(&self, range: &SourceRange, handle: &Handle, marks: Option<&Marks>) -> bool {
        match &range.space {
            Space::Folds { place, level } => self.fold((place, *level), range, handle),
            Space::Task { task } => self.task(task),
            _ => self.may_show(range, marks),
        }
    }

    /// A line of work, a task: shown.
    fn at_hand(&self, _at_hand: &AtHand) -> bool {
        true
    }
}

/// The proofs each runner's sources read from the store, by the runner's address: the fold proofs (a node
/// read, `fold_read`) and the task-list loads (`task_read`). A test's witness that a call proves afresh.
#[cfg(test)]
static PROVED: std::sync::Mutex<std::collections::BTreeMap<usize, (u64, u64)>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

#[cfg(test)]
impl GateSource<'_> {
    /// One proof read from the store for this source's runner: a task-list load, else a fold's.
    fn witness(&self, task: bool) {
        let mut all = PROVED.lock().unwrap();
        let n = all
            .entry(std::ptr::from_ref(self.runner) as usize)
            .or_default();
        match task {
            true => n.1 += 1,
            false => n.0 += 1,
        }
    }
}

/// The fold proofs and the task-list loads every source of `runner` read from the store since the last ask,
/// and from now none: what was proved, not what a source says it spent ([`GateSource::reads`], which a kept
/// source would return again).
#[cfg(test)]
pub(crate) fn proved(runner: &TurnRunner) -> (u64, u64) {
    let key = std::ptr::from_ref(runner) as usize;
    PROVED.lock().unwrap().remove(&key).unwrap_or_default()
}

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
//! A fold's proof reads its node and every exchange it stands for, so each source keeps what it proved: one
//! source serves every view of a call, and a node is read once however many views hold it.

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
    memo: Memo,
}

/// What a source has proved, and the records it read to prove folds and task cards.
#[derive(Default)]
struct Memo {
    /// By the fold's handle and its range.
    folds: RefCell<HashMap<(Handle, SourceRange), bool>>,
    /// By task id.
    tasks: RefCell<HashMap<String, bool>>,
    /// Every task, read once; none when they cannot be read.
    all: RefCell<Option<Option<Vec<crate::task_graph::TaskRecord>>>>,
    reads: Cell<u64>,
}

impl<'a> GateSource<'a> {
    pub fn new(runner: &'a TurnRunner, pov: PointOfView, asker: Option<&'a str>) -> Self {
        Self {
            runner,
            pov,
            asker,
            memo: Memo::default(),
        }
    }

    /// The records read so far to prove folds and task cards: a fold's node and each exchange it stands
    /// for, and the task list once.
    pub fn reads(&self) -> u64 {
        self.memo.reads.get()
    }

    fn read(&self, n: usize) {
        self.memo.reads.set(self.memo.reads.get() + n as u64);
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

    /// Whether fold `handle` over `range` of `place` at `level` is proven today, once per source.
    fn fold(&self, (place, level): (&str, u8), range: &SourceRange, handle: &Handle) -> bool {
        let key = (handle.clone(), range.clone());
        if let Some(ok) = self.memo.folds.borrow().get(&key) {
            return *ok;
        }
        let ok = self.fold_read((place, level), range, handle);
        self.memo.folds.borrow_mut().insert(key, ok);
        ok
    }

    /// Today's node by its handle: of the range's place, level and ends, every exchange it stands for
    /// resolved and of that place, and each session clear.
    fn fold_read(&self, (place, level): (&str, u8), range: &SourceRange, handle: &Handle) -> bool {
        let store = &self.runner.store;
        self.read(1);
        let Ok(Some(node)) = crate::ladder::fold::node(store, &handle.0) else {
            return false;
        };
        let ends = (node.exchanges.first(), node.exchanges.last());
        if node.place != place
            || node.level != level
            || ends != (Some(&range.first.id), Some(&range.last.id))
        {
            return false;
        }
        self.read(node.exchanges.len());
        let Some(all) = crate::ladder::fold::whole(store, &node) else {
            return false;
        };
        let sessions: BTreeSet<&str> = all.iter().map(|e| e.session_id.as_str()).collect();
        all.iter().all(|e| e.place == place) && sessions.into_iter().all(|s| self.clear(s))
    }

    /// Whether task `id`'s card is proven today: its record reads and its home session is clear.
    fn task(&self, id: &str) -> bool {
        if let Some(ok) = self.memo.tasks.borrow().get(id) {
            return *ok;
        }
        let mut all = self.memo.all.borrow_mut();
        let all = all.get_or_insert_with(|| {
            self.read(1);
            crate::task_graph::all(&self.runner.store).ok()
        });
        let ok = (all.as_deref())
            .and_then(|all| {
                Some(crate::task_graph::home(
                    all,
                    all.iter().find(|t| t.id == id)?,
                ))
            })
            .is_some_and(|home| self.clear(&home));
        self.memo.tasks.borrow_mut().insert(id.into(), ok);
        ok
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

//! The view wired to the store, for the compiler's strategy `view` (`compiler::view`): the ledger's touch
//! index as a [`Timeline`], one frozen [`Snapshot`] a turn, and [`assemble`], which runs the selectors, the
//! gate, the budgeter, the renderer and the ladder's read once, in that order (`turn::view_step` gives it
//! the gate's `super::permit::Permit`, the renderer `super::wire_text::TextRenderer` and the ladder
//! `crate::ladder::view::StoreLadder`).

use super::budget::{Budget, Overflow, View};
use super::fold::{Ladder, LadderRead, FOLD_STEP};
use super::gate::Gate;
use super::render::{Renderer, RequestParts};
use super::select::Selector;
use super::timeline::{Cursor, ExchangeRef, Timeline, TimelineRoot};
use super::{Anchor, AtHand, PointOfView, Pos, Snapshot};
use crate::exchange::{self, Root};
use crate::store::Store;

/// The ledger's touch index, read as a timeline.
pub struct StoreTimeline<'a>(pub &'a Store);

impl Timeline for StoreTimeline<'_> {
    fn newest(
        &self,
        root: &TimelineRoot,
        before: Option<&Cursor>,
        n: usize,
    ) -> anyhow::Result<Vec<ExchangeRef>> {
        let root = match root {
            TimelineRoot::Place(p) => Root::Place(p),
            TimelineRoot::Person(p) => Root::Person(p),
            TimelineRoot::Thing(t) => Root::Thing(t),
        };
        let touches = match before {
            None => exchange::newest(self.0, root, n)?,
            Some(c) => exchange::newest_before_exchange(self.0, root, c.at_ms, &c.turn_id, n)?,
        };
        Ok(touches
            .into_iter()
            .map(|t| ExchangeRef {
                turn_id: t.exchange,
                session_id: t.session_id,
                at_ms: t.at_ms,
            })
            .collect())
    }
}

/// A turn's one read of the store: its time, the timeline, and what is at hand.
pub struct StoreSnapshot<'a> {
    pub now_ms: u64,
    pub timeline: StoreTimeline<'a>,
    pub at_hand: Option<AtHand>,
}

impl Snapshot for StoreSnapshot<'_> {
    fn now_ms(&self) -> u64 {
        self.now_ms
    }
    fn timeline(&self) -> &dyn Timeline {
        &self.timeline
    }
    fn at_hand(&self, _: &Anchor) -> Option<AtHand> {
        self.at_hand.clone()
    }
}

/// A ladder read once per place and mark for an assembly: every selector, the text and the window's
/// materialization see the same nodes and window, whatever folds while the assembly runs.
pub struct OnceLadder<'a> {
    inner: &'a dyn Ladder,
    read: std::cell::RefCell<std::collections::HashMap<ReadKey, LadderRead>>,
}

/// A ladder read's place, grain and `/new` mark.
type ReadKey = (String, usize, Option<Pos>);

impl<'a> OnceLadder<'a> {
    pub fn new(inner: &'a dyn Ladder) -> Self {
        Self {
            inner,
            read: Default::default(),
        }
    }
}

impl Ladder for OnceLadder<'_> {
    fn read(&self, place: &str, n: usize, since: Option<&Pos>) -> anyhow::Result<LadderRead> {
        let key = (place.to_string(), n, since.cloned());
        if let Some(r) = self.read.borrow().get(&key) {
            return Ok(r.clone());
        }
        let r = self.inner.read(place, n, since)?;
        self.read.borrow_mut().insert(key, r.clone());
        Ok(r)
    }
}

/// What a view is assembled with.
pub struct Parts<'a> {
    /// One per row of the admission table (`super::select::table`).
    pub selectors: &'a [Box<dyn Selector + 'a>],
    pub gate: &'a dyn Gate,
    pub renderer: &'a dyn Renderer,
    pub ladder: &'a dyn Ladder,
    /// The place's newest `/new` mark: nothing before it is in its window or ladder.
    pub since: Option<Pos>,
}

/// A turn's view, assembled once: what the budgeter kept, its parts in wire order, and the ladder's read.
#[derive(Debug, Clone)]
pub struct Assembled {
    pub view: View,
    pub parts: RequestParts,
    pub memory_page: (u32, &'static str),
    pub ladder: LadderRead,
}

/// Why a view could not be assembled: the floor did not fit (rule 9), or a read failed.
#[derive(Debug)]
pub enum NotAssembled {
    Overflow(Overflow),
    Read(anyhow::Error),
}

impl From<anyhow::Error> for NotAssembled {
    fn from(e: anyhow::Error) -> Self {
        Self::Read(e)
    }
}

/// Run every selector once from `snap`, the gate on each piece, the budgeter, the renderer, and the ladder's
/// read for the point of view's place: one turn's view (design §5.1, "Frozen within a turn").
pub fn assemble(
    pov: &PointOfView,
    snap: &dyn Snapshot,
    parts: &Parts<'_>,
    budget: &Budget,
) -> Result<Assembled, NotAssembled> {
    let counted = super::select::assemble_counted(pov, snap, parts.selectors, parts.gate, budget)?;
    if counted.faults > 0 {
        tracing::warn!(
            faults = counted.faults,
            "the view left out pieces that claimed code's own text"
        );
    }
    let mut view = counted.view.map_err(NotAssembled::Overflow)?;
    // Slot H against the window as chosen: a recalled passage the request carries already is laid once.
    let carried = parts.renderer.carried(&view)?;
    super::budget::set_aside(&mut view, &carried);
    let rendered = parts.renderer.render(&view, snap)?;
    let ladder = match &pov.anchor {
        Anchor::Place(place) => parts.ladder.read(place, FOLD_STEP, parts.since.as_ref())?,
        Anchor::Task(_) => LadderRead {
            nodes: Vec::new(),
            window: Vec::new(),
        },
    };
    Ok(Assembled {
        view,
        parts: rendered,
        memory_page: parts.renderer.memory_page(),
        ladder,
    })
}

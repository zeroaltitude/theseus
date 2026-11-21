//! The view wired to the store, for the compiler's strategy `view` (`compiler::view`): the ledger's touch
//! index as a [`Timeline`], one frozen [`Snapshot`] a turn, and [`assemble`], which runs the selectors, the
//! gate, the budgeter, the renderer and the ladder's read once, in that order (`turn::view_step` gives it
//! the gate's `super::permit::Permit`, the renderer `super::wire_text::TextRenderer` and the ladder
//! `crate::ladder::view::StoreLadder`).

use super::budget::{Budget, Overflow, View};
use super::fold::{Ladder, LadderRead, WINDOW_N};
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
            Some(c) => exchange::newest_before(self.0, root, c.at_ms, n)?,
        };
        Ok(touches
            .into_iter()
            .map(|t| ExchangeRef {
                turn_id: t.exchange,
                session_id: t.session_id,
                at_ms: t.at_ms,
            })
            .filter(|e| before.is_none_or(|c| e.cursor() < *c))
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
    let view = super::select::assemble(pov, snap, parts.selectors, parts.gate, budget)?
        .map_err(NotAssembled::Overflow)?;
    let rendered = parts.renderer.render(&view, snap)?;
    let ladder = match &pov.anchor {
        Anchor::Place(place) => parts.ladder.read(place, WINDOW_N, parts.since.as_ref())?,
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

//! The view wired to the store, for the compiler's strategy `view` (`compiler::view`): the ledger's touch
//! index as a [`Timeline`], one frozen [`Snapshot`] a turn, and [`assemble`], which runs the selectors, the
//! gate, the budgeter, the renderer and the ladder's read once, in that order. Until the selector, gate,
//! renderer and fold work is wired in, [`Parts::unwired`] holds what stands in for each: no selector, a
//! renderer that lays no block, and a ladder of no node whose window is the place's newest exchanges. The
//! gate is the gate's own ([`super::permit::Permit`]).

use super::budget::{fit, Budget, Overflow, View};
use super::fold::{Ladder, LadderRead, WINDOW_N};
use super::gate::{Admitted, Gate};
use super::render::{Renderer, RequestParts};
use super::select::Selector;
use super::timeline::{Cursor, ExchangeRef, Timeline, TimelineRoot};
use super::{Anchor, AtHand, PointOfView, Pos, Snapshot, Space};
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

/// The ladder before the fold work: no node, and as its window the place's newest exchanges, as many as a
/// window holds before it folds (`2N - 1`), oldest first. `since` (the `/new` mark) cuts the window.
pub struct Unfolded<'a>(pub &'a dyn Timeline);

impl Ladder for Unfolded<'_> {
    fn read(&self, place: &str, n: usize, since: Option<&Pos>) -> anyhow::Result<LadderRead> {
        let root = TimelineRoot::Place(place.to_string());
        let mut window = self.0.newest(&root, None, (2 * n).saturating_sub(1))?;
        window.retain(|e| since.is_none_or(|s| e.at_ms > s.at_ms));
        window.reverse();
        Ok(LadderRead {
            nodes: Vec::new(),
            window,
        })
    }
}

/// The renderer before the renderer work: no memory page and no block.
pub struct Shell;

impl Renderer for Shell {
    fn memory_page(&self) -> (u32, &'static str) {
        (0, "")
    }
    fn render(&self, _: &View, _: &dyn Snapshot) -> anyhow::Result<RequestParts> {
        Ok(RequestParts::default())
    }
}

/// What a view is assembled with.
pub struct Parts<'a> {
    pub selectors: &'a [&'a dyn Selector],
    pub gate: &'a dyn Gate,
    pub renderer: &'a dyn Renderer,
    pub ladder: &'a dyn Ladder,
}

impl<'a> Parts<'a> {
    /// The stand-ins (module doc), with `ladder` as the ladder.
    pub fn unwired(ladder: &'a dyn Ladder) -> Self {
        Self {
            selectors: &[],
            gate: &super::permit::Permit,
            renderer: &Shell,
            ladder,
        }
    }
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
    let mut admitted = Vec::new();
    for s in parts.selectors {
        for piece in s.select(pov, snap)? {
            // What code laid has nothing to permit; everything else passes the gate.
            admitted.extend(match piece.range.space {
                Space::Code { .. } => Admitted::code(piece),
                _ => parts.gate.admit(pov, piece),
            });
        }
    }
    let view = fit(pov, admitted, budget).map_err(NotAssembled::Overflow)?;
    let rendered = parts.renderer.render(&view, snap)?;
    let ladder = match &pov.anchor {
        Anchor::Place(place) => parts.ladder.read(place, WINDOW_N, None)?,
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

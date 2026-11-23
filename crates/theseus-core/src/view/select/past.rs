//! Slot E, his past (design §5.3): his own words, older than two days, by the bands of their age. To seven
//! days, all of them with the `did` line of what he asked; to thirty days, all of them, alone; beyond, only
//! the lines that are a correction, made a standing record, or still wait. A line the judge marked a
//! fragment or a correction is never shown bare: at every age it carries its exchange's `did` line, or it is
//! cut and counted. Where the owner is alone it reads his timeline across places; otherwise, this place's
//! words only. A place's own conversation since its `/new` mark is its ladder's, not his past's; what he
//! said here before the mark is his past at any age: younger than 48 hours it is in the first band, with the
//! same line and `did` line (slot F leaves this place out, so it would otherwise be nowhere).

use super::super::history::PAGE_EXCHANGES;
use super::super::timeline::{Cursor, TimelineRoot};
use super::super::{Anchor, Form, Piece, PointOfView, Pos, Reason};
use super::elsewhere::LATELY_MS;
use super::make::{self, Cut};
use super::sources::{ExchangeFacts, Sources};
use super::{Band, Selector, Snapshot};

const DAY_MS: u64 = 24 * 3_600_000;

/// The ages a band holds: older than `from`, at most `to` (ms).
fn ages(band: Band) -> (u64, Option<u64>) {
    match band {
        Band::Past7 => (LATELY_MS, Some(7 * DAY_MS)),
        Band::Past30 => (7 * DAY_MS, Some(30 * DAY_MS)),
        _ => (30 * DAY_MS, None),
    }
}

/// One selector of slot E.
pub struct Past<'a> {
    band: Band,
    src: Sources<'a>,
}

impl<'a> Past<'a> {
    /// `band` is `Past7`, `Past30` or `PastOlder`.
    pub fn new(band: Band, src: Sources<'a>) -> Self {
        debug_assert!(matches!(band, Band::Past7 | Band::Past30 | Band::PastOlder));
        Self { band, src }
    }

    /// Whether this band lays the exchange.
    fn keeps(&self, f: &ExchangeFacts) -> bool {
        self.band != Band::PastOlder
            || f.kind == super::sources::Kind::Correction
            || f.standing
            || f.waiting
    }
}

/// What a point of view's past reads: the root, the person whose words they must be (`None`: anyone's, in
/// company), this place, and its `/new` mark.
struct Scope {
    root: TimelineRoot,
    person: Option<String>,
    here: Option<String>,
    mark: Option<Pos>,
}

impl Scope {
    fn of(pov: &PointOfView, src: &Sources<'_>) -> anyhow::Result<Option<Self>> {
        let here = match &pov.anchor {
            Anchor::Place(p) => Some(p.clone()),
            Anchor::Task(_) => None,
        };
        let mark = match &here {
            Some(p) => src.places.new_mark(p)?,
            None => None,
        };
        let (root, person) = match (&pov.person, &here) {
            (Some(p), _) => (TimelineRoot::Person(p.clone()), Some(p.clone())),
            (None, Some(h)) => (TimelineRoot::Place(h.clone()), None),
            (None, None) => return Ok(None),
        };
        Ok(Some(Self {
            root,
            person,
            here,
            mark,
        }))
    }

    /// An exchange of this place is past only before its `/new` mark; with no mark, it is all the ladder's.
    fn admits(&self, pos: &Pos, f: &ExchangeFacts) -> bool {
        let his = match &self.person {
            Some(p) => f.person.as_deref() == Some(p),
            None => f.person.is_some(),
        };
        let past_here =
            Some(&f.place) != self.here.as_ref() || self.mark.as_ref().is_some_and(|m| pos < m);
        his && past_here
    }
}

impl Past<'_> {
    /// Page `root` newest first from strictly before `start` down to `oldest`, laying each exchange this
    /// scope and band keep.
    fn walk(
        &self,
        scope: &Scope,
        snap: &dyn Snapshot,
        root: &TimelineRoot,
        start: Cursor,
        oldest: Option<u64>,
    ) -> anyhow::Result<Vec<Piece>> {
        let cut = Cut::Past {
            did: self.band == Band::Past7,
        };
        let mut out = Vec::new();
        let mut before = Some(start);
        loop {
            let page = snap
                .timeline()
                .newest(root, before.as_ref(), PAGE_EXCHANGES)?;
            let Some(last) = page.last() else { break };
            before = Some(last.cursor());
            for e in page {
                if oldest.is_some_and(|o| e.at_ms < o) {
                    return Ok(out);
                }
                let pos = Pos {
                    at_ms: e.at_ms,
                    id: e.turn_id.clone(),
                };
                if let Some(f) = self.src.exchanges.facts(&e.turn_id)? {
                    if scope.admits(&pos, &f) && self.keeps(&f) {
                        out.push(make::exchange(
                            &e,
                            &f,
                            self.band,
                            Reason::Past,
                            Form::Line,
                            cut,
                        ));
                    }
                }
            }
        }
        Ok(out)
    }
}

impl Selector for Past<'_> {
    fn band(&self) -> Band {
        self.band
    }

    fn select(&self, pov: &PointOfView, snap: &dyn Snapshot) -> anyhow::Result<Vec<Piece>> {
        if self.band == Band::PastOlder && !self.src.exchanges.has_old_candidates() {
            return Ok(Vec::new());
        }
        let Some(scope) = Scope::of(pov, &self.src)? else {
            return Ok(Vec::new());
        };
        let (from, to) = ages(self.band);
        let oldest = to.map(|t| pov.now_ms.saturating_sub(t));
        let start = Cursor {
            at_ms: pov.now_ms.saturating_sub(from),
            turn_id: String::new(),
        };
        let mut out = self.walk(&scope, snap, &scope.root, start, oldest)?;
        if let (Band::Past7, Some(here), Some(mark)) = (self.band, &scope.here, &scope.mark) {
            let before = Cursor {
                at_ms: mark.at_ms,
                turn_id: mark.id.clone(),
            };
            let root = TimelineRoot::Place(here.clone());
            let lately = Some(pov.now_ms.saturating_sub(LATELY_MS));
            out.extend(self.walk(&scope, snap, &root, before, lately)?);
        }
        Ok(out)
    }
}

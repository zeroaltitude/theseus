//! Slot E, his past (design §5.3): his own words, older than two days, by the bands of their age. To seven
//! days, all of them with the `did` line of what he asked; to thirty days, all of them, alone; beyond, only
//! the lines that are a correction, made a standing record, or still wait. A line the judge marked a
//! fragment or a correction is never shown bare: at every age it carries its exchange's `did` line, or it is
//! cut and counted. Where the owner is alone it reads his timeline across places; otherwise, this place's
//! words only. A place's own conversation since its `/new` mark is its ladder's, not his past's.

use super::super::history::PAGE_EXCHANGES;
use super::super::timeline::{Cursor, TimelineRoot};
use super::super::{Anchor, Form, Piece, PointOfView, Pos, Present, Reason};
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
        let (root, person) = match (pov.present, &pov.person, &here) {
            (Present::Unknown, _, _) => return Ok(None),
            (Present::OwnerAlone, Some(p), _) => (TimelineRoot::Person(p.clone()), Some(p.clone())),
            (_, _, Some(h)) => (TimelineRoot::Place(h.clone()), None),
            _ => return Ok(None),
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

impl Selector for Past<'_> {
    fn band(&self) -> Band {
        self.band
    }

    fn select(&self, pov: &PointOfView, snap: &dyn Snapshot) -> anyhow::Result<Vec<Piece>> {
        let Some(scope) = Scope::of(pov, &self.src)? else {
            return Ok(Vec::new());
        };
        let (from, to) = ages(self.band);
        let oldest = to.map(|t| pov.now_ms.saturating_sub(t));
        let cut = Cut::Past {
            did: self.band == Band::Past7,
        };
        let mut before = Some(Cursor {
            at_ms: pov.now_ms.saturating_sub(from),
            turn_id: String::new(),
        });
        let mut out = Vec::new();
        'pages: loop {
            let page = snap
                .timeline()
                .newest(&scope.root, before.as_ref(), PAGE_EXCHANGES)?;
            let Some(last) = page.last() else { break };
            before = Some(last.cursor());
            for e in page {
                if oldest.is_some_and(|o| e.at_ms < o) {
                    break 'pages;
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

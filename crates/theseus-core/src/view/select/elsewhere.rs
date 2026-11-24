//! Slot F, elsewhere lately (design §5.1, §5.3): his words in his other
//! places in the last 48 hours, verbatim with their `did` lines; per place the newest reply in full;
//! and always, whatever its age, his newest exchange in each other place, five places at most. Two
//! bands: his words (a line each) and the replies (the newest exchange per place, spoken); the two ask for
//! the same range, so rule 3 shows each exchange once and the cut takes a reply away before his words.

use std::collections::BTreeSet;

use super::super::history::PAGE_EXCHANGES;
use super::super::timeline::{Cursor, ExchangeRef, TimelineRoot};
use super::super::{Anchor, Form, Piece, PointOfView, Reason};
use super::make::{self, Cut};
use super::sources::{ExchangeFacts, Sources};
use super::{Band, Selector, Snapshot};

/// How far back "lately" reaches.
pub const LATELY_MS: u64 = 48 * 3_600_000;
/// His newest exchange is laid for at most this many other places.
pub const PLACES_ALWAYS: usize = 5;

/// One exchange of his elsewhere.
struct Seen {
    e: ExchangeRef,
    f: ExchangeFacts,
}

/// Whose and from where: the person, and this place (to leave out).
struct Who<'p> {
    person: &'p str,
    here: Option<&'p str>,
}

impl Who<'_> {
    fn of<'p>(pov: &'p PointOfView) -> Option<Who<'p>> {
        Some(Who {
            person: pov.person.as_deref()?,
            here: match &pov.anchor {
                Anchor::Place(p) => Some(p),
                Anchor::Task(_) => None,
            },
        })
    }

    fn keeps(&self, f: &ExchangeFacts) -> bool {
        Some(f.place.as_str()) != self.here && f.person.as_deref() == Some(self.person)
    }
}

/// His exchanges elsewhere, in one pass over his own timeline, newest first, in bounded pages: every
/// admitted one of the last 48 hours, and the first admitted one in each of his five newest other
/// places, whatever its age. The pass ends when both are found, or the timeline ends. Every choice is made
/// over exchanges the gate admits.
fn scan(pov: &PointOfView, snap: &dyn Snapshot, src: &Sources<'_>) -> anyhow::Result<Vec<Seen>> {
    let Some(who) = Who::of(pov) else {
        return Ok(Vec::new());
    };
    let cutoff = pov.now_ms.saturating_sub(LATELY_MS);
    let root = TimelineRoot::Person(who.person.to_owned());
    let mut seen = Vec::new();
    let mut places = BTreeSet::new();
    let mut before: Option<Cursor> = None;
    loop {
        let page = snap
            .timeline()
            .newest(&root, before.as_ref(), PAGE_EXCHANGES)?;
        let Some(last) = page.last() else { break };
        before = Some(last.cursor());
        for e in page {
            let lately = e.at_ms >= cutoff;
            if !lately && places.len() >= PLACES_ALWAYS {
                return Ok(seen);
            }
            let Some(f) = src.exchanges.facts(&e.turn_id)?.filter(|f| who.keeps(f)) else {
                continue;
            };
            let p = make::exchange(
                &e,
                &f,
                Band::ElsewhereWords,
                Reason::Elsewhere,
                Form::Line,
                Cut::Elsewhere,
            );
            if !make::admits(src, pov, &p) {
                continue;
            }
            let first_here = places.len() < PLACES_ALWAYS && !places.contains(&f.place);
            if first_here {
                places.insert(f.place.clone());
            }
            if lately || first_here {
                seen.push(Seen { e, f });
            }
        }
    }
    Ok(seen)
}

/// One selector of slot F.
pub struct Elsewhere<'a> {
    band: Band,
    src: Sources<'a>,
}

impl<'a> Elsewhere<'a> {
    /// `band` is `ElsewhereWords` or `ElsewhereReplies`.
    pub fn new(band: Band, src: Sources<'a>) -> Self {
        debug_assert!(matches!(
            band,
            Band::ElsewhereWords | Band::ElsewhereReplies
        ));
        Self { band, src }
    }
}

impl Selector for Elsewhere<'_> {
    fn band(&self) -> Band {
        self.band
    }

    fn select(&self, pov: &PointOfView, snap: &dyn Snapshot) -> anyhow::Result<Vec<Piece>> {
        let seen = scan(pov, snap, &self.src)?;
        if self.band == Band::ElsewhereWords {
            return Ok(seen
                .iter()
                .map(|s| {
                    make::exchange(
                        &s.e,
                        &s.f,
                        self.band,
                        Reason::Elsewhere,
                        Form::Line,
                        Cut::Elsewhere,
                    )
                })
                .collect());
        }
        let mut places = BTreeSet::new();
        let mut newest: Vec<&Seen> = seen.iter().collect();
        newest.sort_by_key(|s| std::cmp::Reverse(s.e.cursor()));
        // `seen` holds admitted exchanges only, so a denied newer reply cannot take a place's turn.
        Ok(newest
            .into_iter()
            .filter(|s| places.insert(s.f.place.clone()))
            .map(|s| {
                make::exchange(
                    &s.e,
                    &s.f,
                    self.band,
                    Reason::Elsewhere,
                    Form::Spoken,
                    Cut::Elsewhere,
                )
            })
            .collect())
    }
}

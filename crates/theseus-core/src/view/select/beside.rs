//! The selectors of what moves every turn, beside the message (slots C, D, H): each reads one source and
//! asks for what it returns (slot B is `super::people`'s). Slot C is laid only where the owner is alone;
//! nothing is laid for a point of view that may see nothing.

use std::collections::BTreeSet;

use super::super::history::PAGE_EXCHANGES;
use super::super::timeline::{Cursor, TimelineRoot};
use super::super::{Form, Piece, PointOfView, Pos, Reason};
use super::here::anchor_place;
use super::make;
use super::sources::Sources;
use super::{Band, Selector, Snapshot};

/// What is in play: the things named in the newest this-many exchanges here, since the `/new` mark.
pub const IN_PLAY_EXCHANGES: usize = 3;

/// One selector of slots C, D or H.
pub struct Beside<'a> {
    band: Band,
    src: Sources<'a>,
}

impl<'a> Beside<'a> {
    /// `band` is one of `Waiting`, `InPlay`, `Reached`, `TaskCard`, `Named`, `Recall`.
    pub fn new(band: Band, src: Sources<'a>) -> Self {
        Self { band, src }
    }

    /// The things touched in the newest exchanges here since `/new`, newest first, each once.
    fn in_play(&self, pov: &PointOfView, snap: &dyn Snapshot) -> anyhow::Result<Vec<Piece>> {
        let Some(place) = anchor_place(pov) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        if let Some(at) = &pov.at_hand {
            out.extend(
                self.src
                    .things
                    .at_hand(at)?
                    .into_iter()
                    .map(|r| make::of_ref(r, self.band, Reason::AtHand)),
            );
        }
        let mut things = BTreeSet::new();
        for turn in self.played(pov, snap, place)? {
            for thing in self.src.things.named_in(&turn)? {
                if things.insert(thing.clone()) {
                    out.extend(
                        self.src
                            .things
                            .card(&thing)?
                            .into_iter()
                            .map(|r| make::of_ref(r, self.band, Reason::InPlay)),
                    );
                }
            }
        }
        Ok(out)
    }

    /// The newest [`IN_PLAY_EXCHANGES`] exchanges here since the `/new` mark that the gate admits, newest
    /// first, read in bounded pages: a denied exchange never pushes a permitted one out of play.
    fn played(
        &self,
        pov: &PointOfView,
        snap: &dyn Snapshot,
        place: &str,
    ) -> anyhow::Result<Vec<String>> {
        let mark = self.src.places.new_mark(place)?;
        let root = TimelineRoot::Place(place.to_owned());
        let mut out = Vec::new();
        let mut before: Option<Cursor> = None;
        loop {
            let page = snap
                .timeline()
                .newest(&root, before.as_ref(), PAGE_EXCHANGES)?;
            let Some(last) = page.last() else { break };
            before = Some(last.cursor());
            for e in page {
                let pos = Pos {
                    at_ms: e.at_ms,
                    id: e.turn_id.clone(),
                };
                if mark.as_ref().is_some_and(|m| pos < *m) || out.len() >= IN_PLAY_EXCHANGES {
                    return Ok(out);
                }
                let Some(f) = self.src.exchanges.facts(&e.turn_id)? else {
                    continue;
                };
                let p = make::exchange(
                    &e,
                    &f,
                    self.band,
                    Reason::InPlay,
                    Form::Line,
                    make::Cut::Window,
                );
                if make::admits(&self.src, pov, &p) {
                    out.push(e.turn_id);
                }
            }
        }
        Ok(out)
    }

    /// The cards of the things this message names.
    fn named(&self) -> anyhow::Result<Vec<Piece>> {
        let mut out = Vec::new();
        let names: BTreeSet<String> = self
            .src
            .names
            .named(self.src.message)?
            .into_iter()
            .collect();
        for thing in names {
            out.extend(
                self.src
                    .things
                    .card(&thing)?
                    .into_iter()
                    .map(|r| make::of_ref(r, self.band, Reason::Named)),
            );
        }
        Ok(out)
    }
}

impl Selector for Beside<'_> {
    fn band(&self) -> Band {
        self.band
    }

    fn select(&self, pov: &PointOfView, snap: &dyn Snapshot) -> anyhow::Result<Vec<Piece>> {
        let refs = match self.band {
            Band::InPlay => return self.in_play(pov, snap),
            Band::Named => return self.named(),
            // The reach is the model's own history tool in this build: no search before the first word
            // (trim item 3).
            Band::Reached => return Ok(Vec::new()),
            Band::Waiting => match &pov.person {
                Some(p) => (self.src.waiting.waiting(p)?, Reason::Waiting),
                None => return Ok(Vec::new()),
            },
            Band::TaskCard => (self.src.tasks.joined(pov, self.src.message)?, Reason::Task),
            Band::Recall => (
                self.src.recall.recall(pov, self.src.message)?,
                Reason::Recalled,
            ),
            _ => return Ok(Vec::new()),
        };
        let (refs, reason) = refs;
        Ok(refs
            .into_iter()
            .map(|r| make::of_ref(r, self.band, reason))
            .collect())
    }
}

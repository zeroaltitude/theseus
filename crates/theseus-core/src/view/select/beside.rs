//! The selectors of what moves every turn, beside the message (slots C, D, H) and slot B: each reads one
//! source and asks for what it returns. Slot C is laid only where the owner is alone; nothing is laid for a
//! point of view that may see nothing.

use std::collections::BTreeSet;

use super::super::timeline::TimelineRoot;
use super::super::{Piece, PointOfView, Present, Reason};
use super::here::anchor_place;
use super::make;
use super::sources::Sources;
use super::{Band, Selector, Snapshot};

/// What is in play: the things named in the newest this-many exchanges here, since the `/new` mark.
pub const IN_PLAY_EXCHANGES: usize = 3;

/// One selector of slots B, C, D or H.
pub struct Beside<'a> {
    band: Band,
    src: Sources<'a>,
}

impl<'a> Beside<'a> {
    /// `band` is one of `WhoAndWhere`, `Waiting`, `InPlay`, `Reached`, `TaskCard`, `Named`, `Recall`.
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
        let mark = self.src.places.new_mark(place)?;
        let root = TimelineRoot::Place(place.to_owned());
        let mut things = BTreeSet::new();
        for e in snap.timeline().newest(&root, None, IN_PLAY_EXCHANGES)? {
            if mark
                .as_ref()
                .is_some_and(|m| (e.at_ms, e.turn_id.as_str()) < (m.at_ms, m.id.as_str()))
            {
                continue;
            }
            for thing in self.src.things.named_in(&e.turn_id)? {
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
        if pov.present == Present::Unknown {
            return Ok(Vec::new());
        }
        let refs = match self.band {
            Band::InPlay => return self.in_play(pov, snap),
            Band::Named => return self.named(),
            // The reach is the model's own history tool in this build: no search before the first word
            // (trim item 3).
            Band::Reached => return Ok(Vec::new()),
            Band::Waiting => match (&pov.person, pov.present) {
                (Some(p), Present::OwnerAlone) => (self.src.waiting.waiting(p)?, Reason::Waiting),
                _ => return Ok(Vec::new()),
            },
            Band::WhoAndWhere => match &pov.person {
                Some(p) => (self.src.people.about(p)?, Reason::Fixed),
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

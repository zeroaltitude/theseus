//! WHO AM I TALKING TO (the owner's shape of a turn's context, 2026-11-24). The people present are the speaker
//! and the distinct persons who opened the exchanges this place's window lays, at most [`MOST_PEOPLE`], each
//! named once whatever handle they spoke by. Two bands of slot B:
//! - `WhoAndWhere`, in the front: each person's name, description and the owner's guidance (`People::about`),
//!   about [`PERSON_TOKENS`] a person in company, [`PEOPLE_TOKENS`] at most in all. The same for every person,
//!   whoever speaks, so the front holds from one speaker to the next.
//! - `PeopleRecent`, beside the message: the one line that names the speaker (`People::speaking`), then for each
//!   person present but the owner, their newest [`RECENT_LINES`] exchanges in other places, by any of their
//!   handles, leaving out every exchange the window lays; one line each, with its place and the handle that
//!   opens it, all of them as laid within [`RECENT_TOKENS`]. Real exchanges from the record, never stored notes
//!   about a person. The owner's own are in the window and the look-backs, and his facts in the frame. Which
//!   of a person's lines may be laid is one decision: [`may_lay`], which lays every place's.
//!
//! A turn with no speaker (a wake, an agent's process) is told none of it, as before.

use std::collections::BTreeSet;

use super::super::timeline::TimelineRoot;
use super::super::{Form, Piece, PointOfView, Reason};
use super::here::{anchor_place, shown};
use super::make::{self, Cut};
use super::sources::{ExchangeFacts, Sources};
use super::walk::Walk;
use super::{Band, Selector, Snapshot};

/// The most people the block names: the speaker, then the people of the window who spoke most recently.
pub const MOST_PEOPLE: usize = 8;
/// One person's entry in company, in tokens by today's estimate.
pub const PERSON_TOKENS: u64 = 100;
/// The whole stable part's bound: a person alone has all of it.
pub const PEOPLE_TOKENS: u64 = 1_500;
/// The newest exchanges elsewhere laid for one person.
pub const RECENT_LINES: usize = 3;
/// What every person's recent lines may cost together, as they are laid: headings, wrappers, names and all
/// (`People::laid`).
pub const RECENT_TOKENS: u64 = 600;
/// One recent line's words and `did` line: beyond it, the line is cut where it is laid, with its handle
/// (`forms::fit`).
pub const RECENT_LINE_TOKENS: u64 = 150;
/// The most references of one person's timelines read for their recent lines, all their handles and pages
/// together, a touch met twice counted twice.
pub const RECENT_READ_MOST: usize = 300;

/// A person present: the ledger's name they were found by, and what their record holds.
pub struct Present {
    pub person: String,
    pub id: String,
    /// The owner by identity (`Held::owner`).
    pub owner: bool,
    /// Every ledger handle of theirs but a name.
    pub handles: Vec<String>,
}

/// Whether `p`'s exchange `source`, of another place, may be laid beside the message: the one decision on a
/// person's lines from elsewhere, asked of every one. It lays every place's line, in any company.
/// The owner ruled on 2026-11-24 that no conversation is private; a stricter rule is a change to this function.
pub fn may_lay(_p: &Present, _source: &ExchangeFacts) -> bool {
    true
}

/// The people present: the speaker first, then the window's people newest first, only people a record holds,
/// each once. Nobody for a turn with no speaker.
fn company(src: &Sources<'_>, pov: &PointOfView) -> anyhow::Result<Vec<Present>> {
    let mut c = Vec::new();
    let Some(speaker) = pov.person.as_deref() else {
        return Ok(c);
    };
    let window = shown(src, pov)?;
    let mut asked = BTreeSet::new();
    let others = window.iter().rev().filter_map(|w| w.f.person.as_deref());
    for person in std::iter::once(speaker).chain(others) {
        if !asked.insert(person) {
            continue;
        }
        let Some(held) = src.people.held(person)? else {
            continue;
        };
        if c.len() < MOST_PEOPLE && !c.iter().any(|p| p.id == held.id) {
            c.push(Present {
                person: person.to_string(),
                id: held.id,
                owner: held.owner,
                handles: held.handles,
            });
        }
    }
    Ok(c)
}

/// One selector of slot B: `WhoAndWhere` or `PeopleRecent`.
pub struct People<'a> {
    band: Band,
    src: Sources<'a>,
}

impl<'a> People<'a> {
    pub fn new(band: Band, src: Sources<'a>) -> Self {
        debug_assert!(matches!(band, Band::WhoAndWhere | Band::PeopleRecent));
        Self { band, src }
    }

    /// Each person's entry.
    fn about(&self, present: &[Present]) -> anyhow::Result<Vec<Piece>> {
        let most = match present.len() {
            0 | 1 => PEOPLE_TOKENS,
            _ => PERSON_TOKENS,
        };
        let mut out = Vec::new();
        for p in present {
            for r in self.src.people.about(&p.id, most)? {
                out.push(make::of_ref(r, self.band, Reason::Fixed));
            }
        }
        Ok(out)
    }

    /// The speaker's line, then each person's (but the owner's) newest exchanges elsewhere the gate admits
    /// and [`may_lay`] lays, in bounded pages, within [`RECENT_TOKENS`] in all as laid.
    fn beside(
        &self,
        pov: &PointOfView,
        snap: &dyn Snapshot,
        present: &[Present],
    ) -> anyhow::Result<Vec<Piece>> {
        let mut out = Vec::new();
        let speaker = (present.first()).filter(|p| pov.person.as_deref() == Some(&p.person));
        if let Some(p) = speaker {
            for r in self.src.people.speaking(&p.id)? {
                out.push(make::of_ref(r, self.band, Reason::Fixed));
            }
        }
        let here = anchor_place(pov);
        let window: BTreeSet<String> = (shown(&self.src, pov)?.into_iter())
            .map(|w| w.e.turn_id)
            .collect();
        let mut spent = 0u64;
        for p in present.iter().filter(|p| !p.owner) {
            // One person's timelines, one a handle, walked as one within one allowance.
            let roots = p.handles.iter().cloned().map(TimelineRoot::Person);
            let mut walk = Walk::new(snap, roots, RECENT_READ_MOST);
            let mut lines = 0usize;
            while lines < RECENT_LINES {
                let Some(touch) = walk.next_touch()? else { break };
                let Some(e) = touch else { continue };
                if window.contains(&e.turn_id) {
                    continue;
                }
                let Some(f) = self.src.exchanges.facts(&e.turn_id)? else {
                    continue;
                };
                let theirs = f.person.as_ref().is_some_and(|h| p.handles.contains(h));
                if Some(f.place.as_str()) == here || !theirs || !may_lay(p, &f) {
                    continue;
                }
                let mut piece =
                    make::exchange(&e, &f, self.band, Reason::Present, Form::Line, Cut::Window);
                piece.sizes.line = piece.sizes.line.min(RECENT_LINE_TOKENS);
                if !make::admits(&self.src, pov, &piece) {
                    continue;
                }
                lines += 1;
                let cost = self.src.people.laid(&piece, pov)?;
                if spent + cost > RECENT_TOKENS {
                    continue;
                }
                spent += cost;
                out.push(piece);
            }
        }
        Ok(out)
    }
}

impl Selector for People<'_> {
    fn band(&self) -> Band {
        self.band
    }

    fn select(&self, pov: &PointOfView, snap: &dyn Snapshot) -> anyhow::Result<Vec<Piece>> {
        let present = company(&self.src, pov)?;
        match self.band {
            Band::WhoAndWhere => self.about(&present),
            Band::PeopleRecent => self.beside(pov, snap, &present),
            _ => Ok(Vec::new()),
        }
    }
}

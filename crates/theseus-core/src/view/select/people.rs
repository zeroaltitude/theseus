//! WHO AM I TALKING TO (the owner's shape of a turn's context, 2026-11-24). The people present are the speaker
//! and the distinct persons who opened the exchanges this place's window lays, at most [`MOST_PEOPLE`], each
//! named once whatever handle they spoke by. Two bands of slot B:
//! - `WhoAndWhere`, in the front: each person's name, description and the owner's guidance (`People::about`),
//!   about [`PERSON_TOKENS`] a person in company, [`PEOPLE_TOKENS`] at most in all.
//! - `PeopleRecent`, beside the message: for each person present but the owner, their newest
//!   [`RECENT_LINES`] exchanges in other places, on their own timeline, leaving out every exchange the window
//!   lays; one line each, with its place and the handle that opens it. Real exchanges from the record, never
//!   stored notes about a person. The owner's own are in the window and the look-backs, and his facts in the
//!   frame.
//!
//! A turn with no speaker (a wake, an agent's process) is told none of it, as before.

use std::collections::BTreeSet;

use super::super::budget::HEADING_TOKENS;
use super::super::history::PAGE_EXCHANGES;
use super::super::timeline::{Cursor, TimelineRoot};
use super::super::{Form, Piece, PointOfView, Reason};
use super::here::{anchor_place, shown};
use super::make::{self, Cut};
use super::sources::Sources;
use super::{Band, Selector, Snapshot};
use crate::exchange::OWNER;

/// The most people the block names: the speaker, then the people of the window who spoke most recently.
pub const MOST_PEOPLE: usize = 8;
/// One person's entry in company, in tokens by today's estimate.
pub const PERSON_TOKENS: u64 = 100;
/// The whole stable part's bound: a person alone has all of it.
pub const PEOPLE_TOKENS: u64 = 1_500;
/// The newest exchanges elsewhere laid for one person.
pub const RECENT_LINES: usize = 3;
/// What every person's recent lines may cost together, each with its headings (`budget::HEADING_TOKENS`).
pub const RECENT_TOKENS: u64 = 600;
/// One recent line's words and `did` line: beyond it, the line is cut where it is laid, with its handle
/// (`forms::fit`).
pub const RECENT_LINE_TOKENS: u64 = 150;
/// The most of one person's timeline read for their recent lines.
pub const RECENT_READ_MOST: usize = 300;

/// A person present: the ledger's name for them, and the id of the record that holds them.
struct Present {
    person: String,
    id: String,
}

/// The people present, the speaker first, then the window's people newest first; only people a record holds,
/// each once. Nobody for a turn with no speaker.
fn present(src: &Sources<'_>, pov: &PointOfView) -> anyhow::Result<Vec<Present>> {
    let Some(speaker) = pov.person.as_deref() else {
        return Ok(Vec::new());
    };
    let window = shown(src, pov)?;
    let mut asked = BTreeSet::new();
    let mut out: Vec<Present> = Vec::new();
    let others = window.iter().rev().filter_map(|w| w.f.person.as_deref());
    for person in std::iter::once(speaker).chain(others) {
        if out.len() >= MOST_PEOPLE {
            break;
        }
        if !asked.insert(person) {
            continue;
        }
        if let Some(id) = src.people.held(person)? {
            if !out.iter().any(|p| p.id == id) {
                out.push(Present {
                    person: person.to_string(),
                    id,
                });
            }
        }
    }
    Ok(out)
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

    /// Each person's entry, the speaker's said so.
    fn about(&self, pov: &PointOfView, present: &[Present]) -> anyhow::Result<Vec<Piece>> {
        let most = match present.len() {
            0 | 1 => PEOPLE_TOKENS,
            _ => PERSON_TOKENS,
        };
        let mut out = Vec::new();
        for p in present {
            let speaking = pov.person.as_deref() == Some(p.person.as_str());
            for r in self.src.people.about(&p.id, speaking, most)? {
                out.push(make::of_ref(r, self.band, Reason::Fixed));
            }
        }
        Ok(out)
    }

    /// Each person's (but the owner's) newest exchanges elsewhere the gate admits, in bounded pages, within
    /// [`RECENT_TOKENS`] in all.
    fn recent(
        &self,
        pov: &PointOfView,
        snap: &dyn Snapshot,
        present: &[Present],
    ) -> anyhow::Result<Vec<Piece>> {
        let here = anchor_place(pov);
        let window: BTreeSet<String> = (shown(&self.src, pov)?.into_iter())
            .map(|w| w.e.turn_id)
            .collect();
        let mut out = Vec::new();
        let mut spent = 0u64;
        for p in present.iter().filter(|p| p.person != OWNER) {
            let root = TimelineRoot::Person(p.person.clone());
            let (mut before, mut read, mut lines) = (None::<Cursor>, 0usize, 0usize);
            'walk: while read < RECENT_READ_MOST && lines < RECENT_LINES {
                let page = snap
                    .timeline()
                    .newest(&root, before.as_ref(), PAGE_EXCHANGES)?;
                let Some(last) = page.last() else { break };
                before = Some(last.cursor());
                for e in page {
                    read += 1;
                    if read > RECENT_READ_MOST || lines >= RECENT_LINES {
                        break 'walk;
                    }
                    if window.contains(&e.turn_id) {
                        continue;
                    }
                    let Some(f) = self.src.exchanges.facts(&e.turn_id)? else {
                        continue;
                    };
                    if Some(f.place.as_str()) == here || f.person.as_deref() != Some(&p.person) {
                        continue;
                    }
                    let mut piece =
                        make::exchange(&e, &f, self.band, Reason::Present, Form::Line, Cut::Window);
                    piece.sizes.line = piece.sizes.line.min(RECENT_LINE_TOKENS);
                    if !make::admits(&self.src, pov, &piece) {
                        continue;
                    }
                    lines += 1;
                    let cost = piece.sizes.line + HEADING_TOKENS;
                    if spent + cost > RECENT_TOKENS {
                        continue;
                    }
                    spent += cost;
                    out.push(piece);
                }
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
        let present = present(&self.src, pov)?;
        match self.band {
            Band::WhoAndWhere => self.about(pov, &present),
            Band::PeopleRecent => self.recent(pov, snap, &present),
            _ => Ok(Vec::new()),
        }
    }
}

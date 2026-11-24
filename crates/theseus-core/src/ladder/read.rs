//! The ladder's read for a place, and the fold step. Only what came after the
//! place's latest `/new` ([`super::new_mark`], a position) is counted.
//!
//! - **The window**: every exchange of the place after the last one a
//!   first-level node holds, oldest first, read page by page back through
//!   the place's touches until that position: none is ever passed over, so
//!   after a long failure of the fold's model it holds them all (the
//!   budgeter bounds what is shown).
//! - **A level's standing nodes**: those after the last one the level above
//!   holds, oldest first.
//! - **The fold step**: while the window holds
//!   [`WINDOW_FLOOR`](super::WINDOW_FLOOR) + [`FOLD_STEP`] ended exchanges
//!   (`crate::view::fold::fold_due`), its oldest [`FOLD_STEP`] become a
//!   first-level node; while a level holds 2[`FOLD_STEP`] nodes, its oldest
//!   [`FOLD_STEP`] become one node of the level above, by code. A node is
//!   written once for its range (`fold::write_once`).
//!
//! Every exchange and node read is checked to be of the place (and level)
//! asked: a key's prefix is never the boundary. A place whose name holds the
//! key separator has no ladder.

use std::collections::HashSet;

use theseus_store::blocking;

use super::did::{self, Read};
use super::fold::{self, Flags, FoldNode};
use super::writer::Writer;
use super::{Purse, FOLD_STEP, TASK_WINDOW};
use crate::exchange::{self, Exchange, Root};
use crate::store::Store;
use crate::view::Pos;

/// The most levels a ladder reads: with a fold step of 20, the sixth stands
/// for 64 million exchanges.
pub const MAX_LEVEL: u8 = 6;
/// The most of a level's nodes one read takes (a level stands at fewer than
/// 2[`FOLD_STEP`]).
const READ: usize = 4 * FOLD_STEP;
/// The touches one page of the window's read takes.
const PAGE: usize = 200;

/// One level's standing nodes, oldest first.
#[derive(Debug, Clone, PartialEq)]
pub struct Rung {
    pub level: u8,
    pub nodes: Vec<FoldNode>,
}

/// A place's ladder: its levels, the highest first, and its window.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ladder {
    pub place: String,
    pub since: Option<Pos>,
    pub rungs: Vec<Rung>,
    /// The exchanges no node stands for yet, oldest first.
    pub window: Vec<Exchange>,
}

impl Ladder {
    fn rung(&self, level: u8) -> Option<&Rung> {
        self.rungs.iter().find(|r| r.level == level)
    }
}

fn pos(at_ms: u64, id: &str) -> Pos {
    Pos {
        at_ms,
        id: id.to_string(),
    }
}

/// A node's first and last exchange, as positions.
pub fn first_pos(n: &FoldNode) -> Pos {
    pos(n.first_ms, n.exchanges.first().map_or("", String::as_str))
}
pub fn last_pos(n: &FoldNode) -> Pos {
    pos(n.last_ms, n.exchanges.last().map_or("", String::as_str))
}

/// Whether `p` came after the `/new` mark `since`.
fn after(p: &Pos, since: Option<&Pos>) -> bool {
    since.is_none_or(|s| p > s)
}

/// The ladder of `place`, counting only what came after `since`. A place
/// that may have none reads as empty.
pub fn ladder(store: &Store, place: &str, since: Option<&Pos>) -> anyhow::Result<Ladder> {
    let mut l = Ladder {
        place: place.to_string(),
        since: since.cloned(),
        ..Ladder::default()
    };
    if !super::laddered(place) {
        return Ok(l);
    }
    let mut top = 0;
    while top < MAX_LEVEL && !fold::newest(store, place, top + 1, 1)?.is_empty() {
        top += 1;
    }
    let mut cover: Option<Pos> = None;
    let mut first_cover = None;
    for level in (1..=top).rev() {
        let mut nodes = fold::newest(store, place, level, READ)?;
        let newest = nodes.first().map(last_pos);
        nodes.retain(|n| {
            after(&first_pos(n), since) && cover.as_ref().is_none_or(|c| last_pos(n) > *c)
        });
        nodes.reverse();
        l.rungs.push(Rung { level, nodes });
        cover = newest;
        first_cover = cover.clone();
    }
    l.window = window(store, place, since, first_cover.as_ref())?;
    // A task's place is never summarised (`build::ladder_after`): its window holds its newest
    // [`TASK_WINDOW`]; older exchanges are reached by the history tool.
    if l.rungs.is_empty() && crate::task_graph::place::task_of(place).is_some() {
        let over = l.window.len().saturating_sub(TASK_WINDOW);
        l.window.drain(..over);
    }
    Ok(l)
}

/// The place's exchanges after `covered` (the last one a first-level node
/// holds) and after `since`, oldest first, read back page by page.
fn window(
    store: &Store,
    place: &str,
    since: Option<&Pos>,
    covered: Option<&Pos>,
) -> anyhow::Result<Vec<Exchange>> {
    let root = Root::Place(place);
    let floor_ms = [since, covered]
        .into_iter()
        .flatten()
        .map(|p| p.at_ms)
        .max();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut page = exchange::newest(store, root, PAGE)?;
    loop {
        let mut past = false;
        for t in &page {
            if !seen.insert(t.exchange.clone()) {
                continue;
            }
            // Ties at the floor's millisecond are each weighed; older is done.
            if floor_ms.is_some_and(|f| t.at_ms < f) {
                past = true;
                continue;
            }
            let p = pos(t.at_ms, &t.exchange);
            if !after(&p, since) || covered.is_some_and(|c| p <= *c) {
                continue;
            }
            if let Some(e) = exchange::exchange(store, &t.exchange)? {
                if e.place == place {
                    out.push(e);
                }
            }
        }
        let Some(oldest) = page.last() else {
            break;
        };
        if past || page.len() < PAGE {
            break;
        }
        page = exchange::newest_after_touch(store, root, oldest, PAGE)?;
    }
    out.sort_by_key(|a| pos(a.start_ms, &a.turn_id));
    Ok(out)
}

/// What a fold step made.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stepped {
    pub made: Vec<FoldNode>,
    pub spent_usd: f64,
    /// Why it stopped before the ladder was whole.
    pub failed: Option<String>,
}

/// What the step is given beside the store.
pub struct Step<'a> {
    pub writer: &'a dyn Writer,
    pub flags: &'a dyn Flags,
    pub purse: &'a dyn Purse,
}

/// Fold `place`'s ladder until no window is due a fold and no level holds 2[`FOLD_STEP`] nodes.
pub async fn fold_step(
    store: &Store,
    place: &str,
    since: Option<&Pos>,
    step: &Step<'_>,
) -> anyhow::Result<Stepped> {
    let _one = super::WRITING.lock().await;
    let mut out = Stepped::default();
    if !super::laddered(place) {
        return Ok(out);
    }
    loop {
        let l = blocking(|| ladder(store, place, since))?;
        if crate::view::fold::fold_due(l.window.len()) {
            let oldest = &l.window[..FOLD_STEP];
            if oldest.iter().any(|e| e.end_ms.is_none()) {
                return Ok(out);
            }
            match first(store, oldest, step).await? {
                Ok((node, spent)) => {
                    out.spent_usd += spent;
                    out.made.push(node);
                }
                Err((why, spent)) => {
                    out.spent_usd += spent;
                    out.failed = Some(why);
                    return Ok(out);
                }
            }
            continue;
        }
        let full = (1..=MAX_LEVEL)
            .find_map(|lv| l.rung(lv).filter(|r| r.nodes.len() >= 2 * FOLD_STEP));
        let Some(rung) = full else { return Ok(out) };
        let Some(node) = fold::above(&rung.nodes[..FOLD_STEP]) else {
            return Ok(out);
        };
        out.made.push(fold::write_once(store, node)?);
    }
}

/// A first-level node and what it cost, or why there is none and what it cost.
pub type Made = Result<(FoldNode, f64), (String, f64)>;

/// One first-level node over `exchanges`, written once for its range.
/// Called under [`super::WRITING`].
pub async fn first(store: &Store, exchanges: &[Exchange], step: &Step<'_>) -> anyhow::Result<Made> {
    let turns: Vec<String> = exchanges.iter().map(|e| e.turn_id.clone()).collect();
    if let (Some(e), false) = (exchanges.first(), turns.is_empty()) {
        let key = fold::range_key(&e.place, 1, e.start_ms, &turns);
        if let Some(had) = blocking(|| store.get_meta::<FoldNode>(&key))? {
            return Ok(Ok((had, 0.0)));
        }
    }
    let mut reads: Vec<(Read, did::Said)> = Vec::new();
    for e in exchanges {
        let Some(r) = blocking(|| did::read(store, &e.turn_id))? else {
            return Ok(Err((
                format!("the exchange {} cannot be read", e.turn_id),
                0.0,
            )));
        };
        let said = blocking(|| did::said(store, &e.turn_id))?;
        reads.push((r, said));
    }
    let (node, w) = fold::first_level(&reads, step.writer, step.flags, step.purse).await;
    Ok(match node {
        Ok(n) => Ok((fold::write_once(store, n)?, w.cost_usd)),
        Err(why) => Err((why, w.cost_usd)),
    })
}

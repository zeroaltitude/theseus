//! The ladder's read for a place, and the fold step. Only what came after the
//! place's latest `/new` (`since_ms`) is counted: a node made before it, or
//! an exchange begun before it, is not this ladder's.
//!
//! - **The window**: the place's exchanges after its newest first-level
//!   node, oldest first; [`N`] to `2N` of them once the ladder has a node.
//! - **A level's standing nodes**: those its level above does not yet stand
//!   for, oldest first.
//! - **The fold step**: while the window holds `2N` (ended) exchanges, its
//!   oldest [`N`] become a first-level node; while a level holds `2N`
//!   nodes, its oldest [`N`] become one node of the level above, by code.

use theseus_store::{blocking, NewRecord};

use super::did::{self, Read};
use super::fold::{self, Flags, FoldNode};
use super::writer::Writer;
use super::{Day, DAY_KEY, N};
use crate::exchange::{self, Exchange, Root};
use crate::store::Store;

/// The most levels a ladder reads: with N = 20, the sixth stands for 64
/// million exchanges.
pub const MAX_LEVEL: u8 = 6;
/// The most of a level's nodes, and of the window's exchanges, one read takes.
const READ: usize = 4 * N;

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
    pub since_ms: u64,
    pub rungs: Vec<Rung>,
    /// The exchanges no node stands for yet, oldest first.
    pub window: Vec<Exchange>,
}

impl Ladder {
    fn rung(&self, level: u8) -> Option<&Rung> {
        self.rungs.iter().find(|r| r.level == level)
    }
}

/// The ladder of `place`, counting only what began at or after `since_ms`.
pub fn ladder(store: &Store, place: &str, since_ms: u64) -> anyhow::Result<Ladder> {
    let mut top = 0;
    while top < MAX_LEVEL && !fold::newest(store, place, top + 1, 1)?.is_empty() {
        top += 1;
    }
    let mut rungs = Vec::new();
    let mut cover: Option<FoldNode> = None;
    for level in (1..=top).rev() {
        let mut nodes: Vec<FoldNode> = fold::newest(store, place, level, READ)?;
        let newest = nodes.first().cloned();
        nodes.retain(|n| n.first_ms >= since_ms && !covers(cover.as_ref(), n.last_ms, &n.id));
        nodes.reverse();
        rungs.push(Rung { level, nodes });
        cover = newest;
    }
    // `cover` is now the newest first-level node, whatever stands above it.
    let mut window = Vec::new();
    for t in exchange::newest(store, Root::Place(place), READ)? {
        if t.at_ms < since_ms || covers_exchange(cover.as_ref(), t.at_ms, &t.exchange) {
            break;
        }
        window.extend(exchange::exchange(store, &t.exchange)?);
    }
    window.reverse();
    Ok(Ladder {
        place: place.to_string(),
        since_ms,
        rungs,
        window,
    })
}

/// Whether the node above, `by`, stands for a child that ended at `last_ms`.
fn covers(by: Option<&FoldNode>, last_ms: u64, id: &str) -> bool {
    by.is_some_and(|b| last_ms < b.last_ms || b.children.iter().any(|c| c.id == id))
}

/// Whether the first-level node `by` stands for an exchange begun at `at_ms`.
fn covers_exchange(by: Option<&FoldNode>, at_ms: u64, turn: &str) -> bool {
    by.is_some_and(|b| at_ms < b.last_ms || b.exchanges.iter().any(|t| t == turn))
}

/// The day's spend, today, and what is left: of the day's line, or of `cap`.
fn left(store: &Store, cap: Option<f64>) -> anyhow::Result<(Day, String, f64)> {
    let today = crate::judge::spend::local_day(theseus_protocol::now_unix_ms());
    let day: Day = blocking(|| store.get_meta(DAY_KEY))?.unwrap_or_default();
    let left = cap.unwrap_or_else(|| day.left(&today));
    Ok((day, today, left))
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
    /// Whether a session is sealed or veiled: its exchanges are folded by
    /// code, their words read by no model.
    pub hidden: &'a (dyn Fn(&str) -> bool + Sync),
    /// The run's own line, in dollars; none for the day's.
    pub cap_usd: Option<f64>,
}

/// Fold `place`'s ladder until no window or level holds `2N`.
pub async fn fold_step(
    store: &Store,
    place: &str,
    since_ms: u64,
    step: &Step<'_>,
) -> anyhow::Result<Stepped> {
    let _one = super::WRITING.lock().await;
    let mut out = Stepped::default();
    loop {
        let l = blocking(|| ladder(store, place, since_ms))?;
        if l.window.len() >= 2 * N {
            let oldest = &l.window[..N];
            if oldest.iter().any(|e| e.end_ms.is_none()) {
                return Ok(out);
            }
            let cap = step.cap_usd.map(|c| c - out.spent_usd);
            match first(store, oldest, step, cap).await? {
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
        let full = (1..=MAX_LEVEL).find_map(|lv| l.rung(lv).filter(|r| r.nodes.len() >= 2 * N));
        let Some(rung) = full else { return Ok(out) };
        let Some(node) = fold::above(&rung.nodes[..N]) else {
            return Ok(out);
        };
        let recs = fold::records(&node)?;
        blocking(|| store.append(&recs))?;
        out.made.push(node);
    }
}

/// A first-level node and what it cost, or why there is none and what it cost.
pub type Made = Result<(FoldNode, f64), (String, f64)>;

/// One first-level node over `exchanges`, and its frame (with the day's
/// spend): the node and its cost, or why there is none and what it cost.
pub async fn first(
    store: &Store,
    exchanges: &[Exchange],
    step: &Step<'_>,
    cap: Option<f64>,
) -> anyhow::Result<Made> {
    let mut reads: Vec<(Read, did::Said)> = Vec::new();
    for e in exchanges {
        let Some(r) = blocking(|| did::read(store, &e.turn_id))? else {
            continue;
        };
        let said = blocking(|| did::said(store, &e.turn_id))?;
        reads.push((r, said));
    }
    let hidden = reads.iter().any(|(r, _)| (step.hidden)(&r.ex.session_id));
    let (day, today, left) = left(store, cap)?;
    let (node, w) = match hidden {
        true => (Ok(fold::hidden(&reads, step.flags)), Default::default()),
        false => fold::first_level(&reads, step.writer, step.flags, left).await,
    };
    let mut frame: Vec<NewRecord> = Vec::new();
    if w.cost_usd > 0.0 {
        frame.push(NewRecord::json(
            theseus_store::kinds::META,
            Some(DAY_KEY),
            &day.spend(&today, w.cost_usd),
        )?);
    }
    let made = match node {
        Ok(n) => {
            frame.extend(fold::records(&n)?);
            Ok((n, w.cost_usd))
        }
        Err(why) => Err((why, w.cost_usd)),
    };
    if !frame.is_empty() {
        blocking(|| store.append(&frame))?;
    }
    Ok(made)
}

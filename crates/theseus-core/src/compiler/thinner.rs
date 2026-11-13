//! Never silently thinner (M6 §2.11, theseus-3nk): a new compilation that
//! holds fewer of its predecessor's nodes says so, in the one place a
//! compilation already says what it left out.
//!
//! A compilation's prefix is its `includes`. Every way a compile may hold
//! fewer of them than the compilation it is derived from is a cause the
//! compile already knows and already records:
//!
//! | Cause | Where it is recorded |
//! |---|---|
//! | the operator asked to start over (`fresh`) | the trigger (`manual_fresh`) |
//! | the ring cut leading turns | a `BudgetDrop` with its range |
//! | a recall's item did not fit | a `BudgetDrop` with its node |
//! | a compaction's summary stands for a range | the summary node, `compaction::visible` |
//! | a split or a claim moved a range to a thread | a `Body::Moved` record, `moved::Moves` |
//! | the read this compile saw is narrower than the last one's | the node is absent from `nodes` |
//!
//! [`read`] classifies every dropped include against that table, and
//! [`say`] writes what it could not place into the compilation's own
//! `BudgetReport` as a drop of reason [`UNACCOUNTED`] and tier [`TIER`], so
//! a prompt that got thinner can never be attributed to anything else.
//! Pure: it reads what it is given, and no clock.
//!
//! It records; it does not refuse. §2.11's reason for the invariant is
//! attribution — "the harness cannot attribute an outcome to a prompt that
//! hid what it dropped" — and the designed carrier is the `BudgetReport`,
//! whose one failure class is a core overage (`context_overage`), not a
//! drop. An unaccounted drop is a bug in this table or in a compile pass;
//! a refusal would turn either into a daemon that cannot answer.

use std::collections::{HashMap, HashSet};

use theseus_protocol::memory::{BudgetDrop, BudgetReport};

use super::{compaction, Compilation};
use crate::moved::Moves;
use crate::stub::Stub;

/// The reason an unaccounted drop carries in the report.
pub const UNACCOUNTED: &str = "unaccounted";
/// The tier it carries: this check, not a compile pass.
pub const TIER: &str = "thinner";
/// The most unaccounted nodes the report names one by one; the whole count
/// is [`Thinner::unaccounted`], which the log and the row say.
pub const CAP: usize = 8;

/// Why one of the predecessor's nodes is not in this prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// The operator asked to start over: `fresh` keeps the last exchange.
    Fresh,
    /// A `BudgetDrop` names the node, or a range that holds it.
    Budget,
    /// A summary stands for it (30c).
    Compacted,
    /// A split or a claim moved it to a thread (theseus-q9b85).
    Moved,
    /// This compile's read does not hold the node at all, so the read was
    /// narrower than the one the predecessor compiled over: not a thinning
    /// this compile did.
    Absent,
    /// None of the above: the prompt got thinner and nothing says why.
    Unaccounted,
}

/// What a new compilation dropped of its predecessor's prefix, and how much
/// of it the record accounts for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Thinner {
    /// The predecessor's includes this prefix does not hold.
    pub dropped: u64,
    /// How many of those a cause above places.
    pub accounted: u64,
    /// How many it cannot place.
    pub unaccounted: u64,
    /// The first [`CAP`] of those, in the predecessor's own order.
    pub names: Vec<String>,
}

impl Thinner {
    /// Whether every drop is placed.
    pub fn whole(&self) -> bool {
        self.unaccounted == 0
    }
}

/// Classify every include `previous` held that `now` does not. `report` is
/// the report `now` is about to carry (its drops are not on `now` yet when
/// the compile calls this), `nodes` the session as this compile read it,
/// and `moves` the `Body::Moved` records read from it.
///
/// `None` when nothing was dropped, so an append and a recompile that holds
/// everything cost nothing and say nothing.
pub fn read(
    previous: &Compilation,
    now: &Compilation,
    report: &BudgetReport,
    nodes: &[(u64, Stub)],
    moves: &Moves,
) -> Option<Thinner> {
    let kept: HashSet<&str> = now.includes.iter().map(String::as_str).collect();
    let gone: Vec<&String> = previous
        .includes
        .iter()
        .filter(|id| !kept.contains(id.as_str()))
        .collect();
    if gone.is_empty() {
        return None;
    }
    let at: HashMap<&str, (u64, &Stub)> = nodes
        .iter()
        .map(|(p, n)| (n.id.as_str(), (*p, n)))
        .collect();
    let shown: HashSet<&str> = compaction::visible(nodes)
        .into_iter()
        .map(|(_, n)| n.id.as_str())
        .collect();
    let ranges = spans(report, &at);
    let mut t = Thinner::default();
    for id in gone {
        t.dropped += 1;
        match cause(id, now, report, &at, &shown, &ranges, moves) {
            Cause::Unaccounted => {
                t.unaccounted += 1;
                if t.names.len() < CAP {
                    t.names.push(id.clone());
                }
            }
            _ => t.accounted += 1,
        }
    }
    Some(t)
}

/// Each range the report cut, as the positions it holds. A range whose ends
/// this read does not hold is left out: it can place nothing.
fn spans(report: &BudgetReport, at: &HashMap<&str, (u64, &Stub)>) -> Vec<(u64, u64)> {
    report
        .dropped
        .iter()
        .filter_map(|d| d.range.as_ref())
        .filter_map(|r| {
            let first = at.get(r.first.as_str())?.0;
            let last = at.get(r.last.as_str())?.0;
            Some((first.min(last), first.max(last)))
        })
        .collect()
}

/// One dropped include's cause, first match in the table's order.
fn cause(
    id: &str,
    now: &Compilation,
    report: &BudgetReport,
    at: &HashMap<&str, (u64, &Stub)>,
    shown: &HashSet<&str>,
    ranges: &[(u64, u64)],
    moves: &Moves,
) -> Cause {
    if now.strategy == "fresh" {
        return Cause::Fresh;
    }
    if names(report, id) {
        return Cause::Budget;
    }
    let Some((p, n)) = at.get(id) else {
        return Cause::Absent;
    };
    if !moves.shows(n) {
        return Cause::Moved;
    }
    if !shown.contains(id) {
        return Cause::Compacted;
    }
    if ranges.iter().any(|(a, b)| (a..=b).contains(&p)) {
        return Cause::Budget;
    }
    Cause::Unaccounted
}

/// Whether a drop names the node itself.
fn names(report: &BudgetReport, id: &str) -> bool {
    report
        .dropped
        .iter()
        .any(|d| d.node_id.as_deref() == Some(id))
}

/// Write what `t` could not place into `report`, one drop a node up to
/// [`CAP`], and log the whole count. A whole accounting writes nothing: the
/// report of a compile that dropped only what it recorded is byte for byte
/// the report it was.
pub fn say(report: &mut BudgetReport, t: &Thinner, compilation_id: &str) {
    if t.whole() {
        return;
    }
    tracing::warn!(
        compilation = %compilation_id,
        dropped = t.dropped,
        accounted = t.accounted,
        unaccounted = t.unaccounted,
        nodes = ?t.names,
        "a compilation holds fewer of its predecessor's nodes than its record accounts for"
    );
    for id in &t.names {
        report.dropped.push(BudgetDrop {
            node_id: Some(id.clone()),
            range: None,
            reason: UNACCOUNTED.into(),
            tokens: 0,
            tier: TIER.into(),
        });
    }
}

/// The unaccounted drops a stored report carries: what `say` wrote, read
/// back by a surface or a row (`fact::turn::ContextRecompiled`).
pub fn unaccounted(report: &BudgetReport) -> usize {
    report
        .dropped
        .iter()
        .filter(|d| d.reason == UNACCOUNTED && d.tier == TIER)
        .count()
}

#[cfg(test)]
mod tests;

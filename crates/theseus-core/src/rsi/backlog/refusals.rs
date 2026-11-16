//! The alarm on a tool's refusals (issue 20): the nightly read files nothing
//! for one refusal (`failure = "refused"` on an `action.failed` row: the text
//! to replace is not in the file, a patch does not apply), because the model
//! reads it and tries again. The price is that a fault of ours in how an edit
//! is matched (line endings, a patch's line counts, the check that a file
//! changed since it was read) would answer the same way and never be filed.
//!
//! So the read counts refusals per producer (`inproc:fs.edit`), beside that
//! producer's calls over the same night, and files ONE item for a producer
//! whose refusals are both many and unusual. These are first guesses, to be
//! moved by the first nights they meet:
//!
//! - [`MIN_REFUSALS`], 20: fewer than this in a night is a handful of
//!   mistakes by a model, however large their share of a quiet night.
//! - [`SPIKE_FACTOR`], 2: a share at least twice the producer's own over the
//!   [`HISTORY_NIGHTS`] nights before. Twice, because a night's share swings
//!   with what the sessions happened to edit, and a bug in matching does not
//!   raise a share by a little.
//! - [`NO_HISTORY_SHARE_PCT`], 60: with under [`HISTORY_MIN_CALLS`] calls in
//!   that history there is no usual share to compare with, so a producer
//!   refused on at least this share of its calls files. Most calls of a tool
//!   that works are not refused, so a majority refused is not the model's
//!   doing alone.
//!
//! A producer files once: not while an item of its cause is open, and not
//! within [`REFILE_NIGHTS`] nights of the newest such item's filing, so a
//! spike that outlives its item (dropped, or failed) does not file again the
//! next night. A cause the owner dropped is kept out by the night's filing,
//! as any is. The history is read only for a producer that passes the first
//! bar, so an ordinary night counts one night's calls and nothing more.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use serde_json::Value;
use theseus_protocol::tasks::TaskRecord;
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use super::read::{action_why, obs, producer, Candidate, Obs};
use super::state::State;
use crate::ledger::LedgerRow;
use crate::store::Store;

/// Refusals a producer has in a night before they can be a cause.
pub const MIN_REFUSALS: u64 = 20;
/// How many times the producer's usual share a night's share must reach.
pub const SPIKE_FACTOR: u64 = 2;
/// The share (percent) that files where there is no usual share to compare.
pub const NO_HISTORY_SHARE_PCT: u64 = 60;
/// The nights before the one read that make a producer's usual share.
pub const HISTORY_NIGHTS: u64 = 14;
/// Calls that history needs for its share to count as a usual one.
pub const HISTORY_MIN_CALLS: u64 = 50;
/// Nights after an item's filing in which its cause is not filed again.
pub const REFILE_NIGHTS: u64 = 7;
/// Refused rows carried as evidence beside the numbers.
const SAMPLES: usize = 5;
/// Rows decoded in one read, at most; past it the counts are a floor.
const MAX_ROWS: usize = 300_000;
const DAY_MS: u64 = 86_400_000;

/// The key every producer's alarm has, before the producer's word.
pub(super) const KEY: &str = "refusal.rate:";

/// The rows a read follows: a call that ended, and how.
const KINDS: &[&str] = &["action.succeeded", "action.failed"];

/// What one producer did over a window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub refused: u64,
    /// Calls that ended: `refused` is among them.
    pub calls: u64,
}

/// A producer's tally, and the newest of its refused rows.
#[derive(Debug, Clone, Default)]
struct Counted {
    tally: Tally,
    samples: Vec<Obs>,
}

/// Whether `night` is many refusals and an unusual share of calls, against
/// the producer's `history` (the module's rule).
pub fn is_spike(night: Tally, history: Tally) -> bool {
    if night.refused < MIN_REFUSALS {
        return false;
    }
    if history.calls >= HISTORY_MIN_CALLS {
        // night/calls >= factor * history_refused/history_calls, crossed.
        return night.refused.saturating_mul(history.calls)
            >= SPIKE_FACTOR
                .saturating_mul(history.refused)
                .saturating_mul(night.calls);
    }
    night.refused.saturating_mul(100) >= NO_HISTORY_SHARE_PCT.saturating_mul(night.calls)
}

/// A share as a person reads it: a whole percent, one decimal under ten.
fn pct(t: Tally) -> String {
    let p = t.refused as f64 * 100.0 / t.calls.max(1) as f64;
    match p < 10.0 {
        true => format!("{p:.1}%"),
        false => format!("{p:.0}%"),
    }
}

/// A producer's name as the model knows its tool: `inproc:fs.edit` is `fs.edit`.
fn tool_of(producer: &str) -> &str {
    producer.split_once(':').map_or(producer, |(_, t)| t)
}

/// The one sentence that says the numbers: the item's title, and its summary.
pub(super) fn said(producer: &str, night: Tally, history: Tally) -> String {
    let usual = match history.calls >= HISTORY_MIN_CALLS {
        true => format!("its usual share is {}", pct(history)),
        false => format!(
            "it has no usual share yet (under {HISTORY_MIN_CALLS} calls in the {HISTORY_NIGHTS} \
             nights before)"
        ),
    };
    format!(
        "{} refused {} of {} calls last night; {usual}",
        tool_of(producer),
        night.refused,
        night.calls
    )
}

/// The observations for one spike: the numbers, then a few refused rows.
fn obs_of(producer: &str, c: Counted, history: Tally, now_ms: u64, date: &str) -> Vec<Obs> {
    let said = said(producer, c.tally, history);
    let key = format!("{KEY}{producer}");
    let summary = Obs {
        key: key.clone(),
        title: said.clone(),
        source: "action.failed".into(),
        id: format!("refusals:{producer}:{date}"),
        note: format!(
            "{said}. Filed at {MIN_REFUSALS} or more refusals in a night and a share of at least \
             {SPIKE_FACTOR} times the usual (or {NO_HISTORY_SHARE_PCT}% with no usual share)."
        ),
        at_ms: now_ms,
        usd: 0.0,
        person: false,
        task_failed: true,
        work: 2,
        graduate: None,
        pattern: None,
    };
    let mut out = vec![summary];
    for mut s in c.samples {
        s.key = key.clone();
        s.title = out[0].title.clone();
        out.push(s);
    }
    out
}

/// The objective of an item filed for a spike: the numbers, and where to look.
pub(super) fn objective_of(c: &Candidate, item: &str) -> String {
    let numbers = c.obs.first().map_or("", |o| o.note.as_str());
    format!(
        "Fix the cause `{}`: {}.\n\n{numbers}\n\nA tool refusing now and then is the tool working: the \
         model asks to replace text that is not there, and tries again. A share this far above \
         the usual may be a fault of ours in how the tool decides to refuse (line endings, a \
         patch's line counts, the check that a file changed since it was read). Read the refused \
         rows on this task, find whether the tool was right to refuse, and fix it with a test \
         that fails without the fix. Call self.propose_change {{ item: \"{item}\", issue: <the \
         issue>, why: <one sentence> }}.",
        c.key, c.title
    )
}

/// Whether the cause `key` has an item that is open, or filed within
/// [`REFILE_NIGHTS`] nights of `now_ms`.
pub(super) fn held(
    key: &str,
    st: &State,
    tasks: &BTreeMap<String, TaskRecord>,
    now_ms: u64,
) -> bool {
    st.items
        .iter()
        .filter(|(_, m)| m.cause == key)
        .filter_map(|(id, _)| tasks.get(id))
        .any(|t| {
            !t.state.is_closed() || now_ms.saturating_sub(t.created_at_ms) < REFILE_NIGHTS * DAY_MS
        })
}

/// Each producer's refusals and calls among the ledger's calls that ended in
/// `(since_ms, until_ms]`, with its newest `keep` refused rows.
fn count(
    store: &Store,
    since_ms: u64,
    until_ms: u64,
    keep: usize,
) -> Result<BTreeMap<String, Counted>> {
    let tags: Vec<String> = KINDS.iter().map(|k| ledger_kind(k)).collect();
    let mut out: BTreeMap<String, Counted> = BTreeMap::new();
    let (mut before, mut seen) = (None, 0usize);
    loop {
        let page = Page {
            kind: kinds::LEDGER,
            tags: tags.clone(),
            after: None,
            before,
            since_ms: Some(since_ms),
            until_ms: Some(until_ms),
            limit: 512,
        };
        let Some(got) = store.ledger_page(&page)? else {
            bail!("the ledger's index is still being built");
        };
        for r in got.records.iter().rev() {
            seen += 1;
            let Ok(row) = r.decode::<LedgerRow>() else {
                continue;
            };
            if row.at_unix_ms <= since_ms || row.at_unix_ms > until_ms {
                continue;
            }
            let c = out.entry(producer(&row.data["producer"])).or_default();
            c.tally.calls += 1;
            if row.kind == "action.failed" && row.data["failure"] == "refused" {
                c.tally.refused += 1;
                if c.samples.len() < keep {
                    c.samples.push(refused_row(r.position, &row));
                }
            }
        }
        match (got.more, got.first) {
            (true, Some(first)) if seen < MAX_ROWS => before = Some(first),
            _ => break,
        }
    }
    Ok(out)
}

/// One refused call, as evidence.
fn refused_row(pos: u64, row: &LedgerRow) -> Obs {
    let d: &Value = &row.data;
    let mut o = obs("action.failed", pos, row);
    o.note = format!(
        "action {} refused after {} ms{}",
        d["correlation_id"],
        d["duration_ms"],
        action_why(d)
    );
    o
}

/// The observations the night's refusals give: one summary and a few rows
/// for each producer in a spike that is not already held. `since_ms` is where
/// the night begins; the history is the [`HISTORY_NIGHTS`] nights before it.
/// A ledger that cannot be read gives none: the rest of the night stands.
pub(super) fn spikes(
    store: &Store,
    st: &State,
    tasks: &BTreeMap<String, TaskRecord>,
    since_ms: u64,
    now_ms: u64,
    date: &str,
) -> Vec<Obs> {
    let Ok(night) = count(store, since_ms, now_ms, SAMPLES) else {
        return vec![];
    };
    let many: Vec<(String, Counted)> = night
        .into_iter()
        .filter(|(p, c)| {
            c.tally.refused >= MIN_REFUSALS && !held(&format!("{KEY}{p}"), st, tasks, now_ms)
        })
        .collect();
    if many.is_empty() {
        return vec![];
    }
    let from = since_ms.saturating_sub(HISTORY_NIGHTS * DAY_MS);
    let Ok(history) = count(store, from, since_ms, 0) else {
        return vec![];
    };
    many.into_iter()
        .filter_map(|(p, c)| {
            let usual = history.get(&p).map_or(Tally::default(), |h| h.tally);
            is_spike(c.tally, usual).then(|| obs_of(&p, c, usual, now_ms, date))
        })
        .flatten()
        .collect()
}

//! `self.log` (theseus-pw1q.4): what Theseus changed about itself, newest
//! first. Two sources, read together through the ledger's pages by kind
//! (`theseus_store::pages`), never a scan:
//!
//! - the `self.*` rows, every name in `theseus_protocol::rsi::SELF_ROWS`,
//!   whose data carries `what`, `why`, `numbers` and `undo` as written;
//! - today's self-change rows ([`TODAY`]), worded here: a pack's move on
//!   the ladder, the learning loop's proposal, an extension's proposal,
//!   trial, answer, load and revoke, the owner's routing correction,
//!   consolidation's synthesis and its check, and a refused resume of the
//!   kill switch.

use anyhow::Result;
use serde_json::{json, Value};
use theseus_protocol::rsi::{SelfLogParams, SelfLogResult, SelfLogRow, SELF_ROWS};
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use crate::ledger::LedgerRow;
use crate::rpc::Core;

/// The kinds Theseus changes itself through today, before any self step.
pub const TODAY: &[&str] = &[
    "pack.mode",
    "judge.proposal",
    "extend.proposed",
    "extend.tested",
    "extend.acked",
    "extend.declined",
    "extend.loaded",
    "extend.revoked",
    "route.corrected",
    "synthesis.proposed",
    "synthesis.checked",
    "approval.refused",
];

/// The default page, and the most one call returns.
const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 500;

impl Core {
    /// The log: rows of the self kinds and today's, at or after
    /// `since_ms`, newest first, at most `limit`.
    pub fn self_log(&self, p: &SelfLogParams) -> Result<SelfLogResult> {
        let limit = p.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let (rows, more, building) = read(&self.store, p.since_ms, None, limit)?;
        Ok(SelfLogResult {
            rows,
            state: self.self_state(),
            more,
            building,
        })
    }
}

/// Every kind the log reads.
fn tags() -> Vec<String> {
    SELF_ROWS
        .iter()
        .map(|k| k.name)
        .chain(TODAY.iter().copied())
        .map(ledger_kind)
        .collect()
}

/// The log's rows in `[since_ms, until_ms]`, newest first, at most `limit`;
/// whether more matched; and whether the index's shape is still built (and
/// nothing could be read).
pub(super) fn read(
    store: &crate::store::Store,
    since_ms: Option<u64>,
    until_ms: Option<u64>,
    limit: usize,
) -> Result<(Vec<SelfLogRow>, bool, bool)> {
    let tags = tags();
    let mut out = Vec::new();
    let mut before = None;
    loop {
        let page = Page {
            kind: kinds::LEDGER,
            tags: tags.clone(),
            after: None,
            before,
            since_ms,
            until_ms,
            limit: 256,
        };
        let Some(got) = store.ledger_page(&page)? else {
            return Ok((Vec::new(), false, true));
        };
        for r in got.records.iter().rev() {
            let Ok(row) = r.decode::<LedgerRow>() else {
                continue;
            };
            // The kind's clock may hold a row a minute early: its own time decides.
            if since_ms.is_some_and(|s| row.at_unix_ms < s)
                || until_ms.is_some_and(|u| row.at_unix_ms > u)
            {
                continue;
            }
            let Some(e) = entry_of(r.position, &row) else {
                continue;
            };
            if out.len() == limit {
                return Ok((out, true, false));
            }
            out.push(e);
        }
        match (got.more, got.first) {
            (true, Some(first)) => before = Some(first),
            _ => return Ok((out, false, false)),
        }
    }
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("?")
}

fn opt(v: &Value, k: &str) -> Option<String> {
    v[k].as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

/// The first characters of a digest.
fn short(d: &str) -> &str {
    &d[..d.len().min(6)]
}

/// Only the fields of `v` named, those present.
fn pick(v: &Value, keys: &[&str]) -> Value {
    let m: serde_json::Map<String, Value> = keys
        .iter()
        .filter(|k| !v[**k].is_null())
        .map(|k| ((*k).to_string(), v[*k].clone()))
        .collect();
    if m.is_empty() {
        Value::Null
    } else {
        Value::Object(m)
    }
}

/// One ledger row as the log shows it: `None` for a row that is no change
/// to Theseus itself (an `approval.refused` of another act).
pub fn entry_of(position: u64, row: &LedgerRow) -> Option<SelfLogRow> {
    let d = &row.data;
    let mut e = SelfLogRow {
        position,
        at_unix_ms: row.at_unix_ms,
        kind: row.kind.clone(),
        what: String::new(),
        why: opt(d, "why"),
        numbers: Value::Null,
        undo: None,
        session_id: row.session_id.clone(),
    };
    match row.kind.as_str() {
        k if k.starts_with("self.") => {
            e.what = opt(d, "what").unwrap_or_else(|| k.to_string());
            e.numbers = d["numbers"].clone();
            e.undo = opt(d, "undo");
        }
        "pack.mode" => pack_moved(&mut e, d),
        "judge.proposal" => pack_learned(&mut e, d),
        "extend.proposed" | "extend.tested" | "extend.acked" | "extend.declined"
        | "extend.loaded" | "extend.revoked" => {
            let (name, digest) = (s(d, "name"), short(s(d, "digest")));
            let verb = row.kind.trim_start_matches("extend.");
            e.what = format!("extension {name} {digest} {verb}");
            e.numbers = pick(d, &["files", "bytes", "passed", "tests", "tools"]);
            e.undo =
                matches!(verb, "acked" | "loaded").then(|| format!("theseus extend revoke {name}"));
        }
        "route.corrected" => {
            e.what = format!(
                "routing corrected by {}: {} to {}",
                s(d, "who"),
                d["turn"].as_str().unwrap_or("a turn"),
                s(d, "to")
            );
            e.numbers = pick(d, &["pack", "mode", "profile"]);
        }
        "synthesis.proposed" => {
            let id = s(d, "synthesis_id");
            e.what = format!(
                "consolidation wrote synthesis {id} from {} sources",
                d["sources"].as_array().map_or(0, Vec::len)
            );
            e.numbers = pick(d, &["cost_usd", "model", "trigger"]);
            e.undo = Some(format!("theseus memory label {id} wrong"));
        }
        "synthesis.checked" => {
            e.what = format!(
                "synthesis {} checked: {}",
                s(d, "synthesis_id"),
                s(d, "verdict")
            );
            e.numbers = pick(d, &["least", "unsupported", "mode"]);
        }
        "approval.refused" if d["act"] == json!(theseus_protocol::method::SELF_RESUME) => {
            e.what = format!(
                "a resume of self-improvement from {} did not count",
                s(d, "who")
            );
        }
        _ => return None,
    }
    Some(e)
}

/// A pack's move on the ladder (`pack.mode`).
fn pack_moved(e: &mut SelfLogRow, d: &Value) {
    let (pack, mode) = (s(d, "pack"), s(d, "mode"));
    e.what = match d["declined"].as_bool() {
        Some(true) => format!("{pack}: the owner declined its move to {mode}"),
        _ => format!(
            "{pack} moved from {} to {mode} by {}",
            s(d, "from"),
            s(d, "who")
        ),
    };
    e.numbers = pick(d, &["share", "numbers", "report", "forced"]);
    e.undo = match mode {
        "live" | "canary" | "shadow" => Some(format!("theseus packs rollback {pack}")),
        _ => None,
    };
}

/// The learning loop's proposal (`judge.proposal`).
fn pack_learned(e: &mut SelfLogRow, d: &Value) {
    let version = d["version"].as_str();
    e.what = format!(
        "the learning loop rewrote {}'s wording{}: {}",
        s(d, "parent"),
        version.map(|v| format!(" as {v}")).unwrap_or_default(),
        s(d, "decision")
    );
    e.numbers = pick(
        d,
        &[
            "fixed",
            "broken",
            "train",
            "holdout",
            "new_errors",
            "writer_usd",
        ],
    );
    e.undo = match (s(d, "decision"), version) {
        ("live" | "canary", Some(v)) => Some(format!("theseus packs rollback {v}")),
        _ => None,
    };
}

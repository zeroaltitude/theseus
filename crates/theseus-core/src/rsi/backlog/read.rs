//! The nightly read: the last day's ledger, health's warnings and the
//! learning loop's report, as observations of a cause. An observation is one
//! piece of evidence (a ledger row, a health line, a report line) with the
//! stable key of the problem it shows; [`group`] folds a night's into one
//! [`Candidate`] per key, so the same problem tomorrow is the same item.
//!
//! The ledger is read by kind through the index's pages, never a scan, and a
//! page the index cannot give yet (its shape is still being built after a
//! start) fails the night: it reads nothing rather than half.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use serde_json::Value;
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use crate::ledger::LedgerRow;
use crate::store::Store;

/// A turn over this long missed its budget (ms).
pub const TURN_BUDGET_MS: u64 = 120_000;
/// A first token later than this missed its budget (ms).
pub const FIRST_TOKEN_BUDGET_MS: u64 = 10_000;
/// Evidence kept on a task from one night, per cause.
pub const NIGHT_EVIDENCE: usize = 20;
/// Rows read from one night's ledger, at most.
const MAX_ROWS: usize = 20_000;

/// The kinds the read follows.
const KINDS: &[&str] = &[
    "action.failed",
    "turn.failed",
    "turn.refused",
    "provider.error",
    "tool.invalid_input",
    "route.corrected",
    "judge.label",
    "judge.report",
    "turn.ended",
    "loop.cap_reached",
    "budget.over_limit",
    "approval.refused",
    "secrets.failed",
    "mcp.failed",
    "self.sealed.drift",
];

/// One piece of evidence for a cause.
#[derive(Debug, Clone, PartialEq)]
pub struct Obs {
    /// The cause's stable key.
    pub key: String,
    pub title: String,
    /// A ledger kind, `health` or `learning`.
    pub source: String,
    /// `ledger:<position>`, `health:<what>:<date>`.
    pub id: String,
    /// What it shows, in a line.
    pub note: String,
    pub at_ms: u64,
    pub usd: f64,
    pub person: bool,
    pub task_failed: bool,
    pub work: u8,
    /// An extension's graduation, on its observations alone.
    pub graduate: Option<super::graduate::Graduation>,
    /// A recurring tool-call sequence, on a pattern's observations alone.
    pub pattern: Option<Pattern>,
}

/// What the pattern miner saw (theseus-pw1q.21.3): the terms of its score,
/// and the words the item's objective carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub score: super::state::PatternScore,
    pub brief: String,
}

/// Every observation of one cause in a night.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub key: String,
    pub title: String,
    pub source: String,
    pub obs: Vec<Obs>,
    /// An extension's graduation: its counts and the port's brief.
    pub graduate: Option<super::graduate::Graduation>,
    /// The newest night's pattern terms.
    pub pattern: Option<Pattern>,
}

impl Candidate {
    pub fn usd(&self) -> f64 {
        self.obs.iter().map(|o| o.usd).sum()
    }
    pub fn person(&self) -> bool {
        self.obs.iter().any(|o| o.person)
    }
    pub fn task_failed(&self) -> bool {
        self.obs.iter().any(|o| o.task_failed)
    }
    pub fn work(&self) -> u8 {
        self.obs.iter().map(|o| o.work).max().unwrap_or(2)
    }
    pub fn last_ms(&self) -> u64 {
        self.obs.iter().map(|o| o.at_ms).max().unwrap_or(0)
    }
}

/// Fold observations into candidates, one per key, by key.
pub fn group(obs: Vec<Obs>) -> Vec<Candidate> {
    let mut by: BTreeMap<String, Candidate> = BTreeMap::new();
    for o in obs {
        by.entry(o.key.clone())
            .or_insert_with(|| Candidate {
                key: o.key.clone(),
                title: o.title.clone(),
                source: o.source.clone(),
                obs: vec![],
                graduate: o.graduate.clone(),
                pattern: None,
            })
            .obs
            .push(o);
    }
    for c in by.values_mut() {
        c.pattern = c.obs.iter().rev().find_map(|o| o.pattern.clone());
    }
    by.into_values().collect()
}

/// A word of a row's text, for a key: lowercase letters, digits and `_`,
/// cut to 32; `unknown` when it has none.
fn word(v: &Value) -> String {
    let w: String = v
        .as_str()
        .unwrap_or("")
        .trim()
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != ':')
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
        .take(32)
        .collect::<String>()
        .to_lowercase();
    if w.is_empty() {
        "unknown".into()
    } else {
        w
    }
}

fn obs(kind: &str, pos: u64, row: &LedgerRow) -> Obs {
    Obs {
        key: String::new(),
        title: String::new(),
        source: kind.into(),
        id: format!("ledger:{pos}"),
        note: String::new(),
        at_ms: row.at_unix_ms,
        usd: 0.0,
        person: false,
        task_failed: false,
        work: 2,
        graduate: None,
        pattern: None,
    }
}

/// What one ledger row shows, if it shows a problem.
pub fn of_row(pos: u64, row: &LedgerRow) -> Option<Obs> {
    let d = &row.data;
    let kind = row.kind.as_str();
    let mut o = obs(kind, pos, row);
    match kind {
        "action.failed" => {
            let p = word(&d["producer"]);
            o.key = format!("action.failed:{p}");
            o.title = format!("Calls from {p} fail");
            o.note = format!(
                "action {} failed after {} ms",
                d["correlation_id"], d["duration_ms"]
            );
            o.usd = d["cost_usd"].as_f64().unwrap_or(0.0);
            o.task_failed = true;
        }
        "turn.failed" => {
            let r = word(&d["reason"]);
            o.key = format!("turn.failed:{r}");
            o.title = format!("Turns fail with {r}");
            o.note = format!("turn failed ({r}) after {} loops", d["loops"]);
            o.usd = d["cost_usd"].as_f64().unwrap_or(0.0);
            (o.person, o.task_failed, o.work) = (true, true, 3);
        }
        "turn.refused" => {
            let c = word(&d["class"]);
            o.key = format!("turn.refused:{c}");
            o.title = format!("Turns are refused: {c}");
            o.note = format!("turn refused ({c}) for {}", d["provider"]);
            (o.person, o.work) = (true, 2);
        }
        "provider.error" => {
            // A call the daemon's stop kept from being sent is no provider's
            // answer: nothing was sent, and the run retries at the next start.
            // Health and the metrics already leave it out of their counts.
            if d["class"].as_str() == Some(crate::provider::STOPPING_CLASS) {
                return None;
            }
            let (c, p) = (word(&d["class"]), word(&d["provider"]));
            o.key = format!("provider.error:{c}:{p}");
            o.title = format!("{p} answers {c}");
            o.note = format!(
                "{p} {c} after {} ms: {}",
                d["elapsed_ms"],
                word(&d["message"])
            );
            (o.person, o.work) = (true, 3);
        }
        "tool.invalid_input" => {
            let t = word(&d["tool"]);
            o.key = format!("tool.invalid_input:{t}");
            o.title = format!("The model sends {t} invalid input");
            o.note = format!("{t}: {}", word(&d["reason"]));
            o.work = 1;
        }
        "route.corrected" => {
            let to = word(&d["to"]);
            o.key = format!("route.corrected:{to}");
            o.title = format!("The owner corrects routing to {to}");
            o.note = format!("routing corrected to {to}");
            (o.person, o.work) = (true, 2);
        }
        "judge.label" if is_wrong(&d["label"]) => {
            let p = word(&d["pack"]);
            o.key = format!("judge.wrong:{p}");
            o.title = format!("Jev's {p} judgments are labeled wrong");
            o.note = format!("judgment {} labeled wrong ({})", d["judgment"], d["source"]);
            (o.person, o.work) = (true, 3);
        }
        "judge.report" => return of_report(o, d),
        "turn.ended" => return of_slow(o, d),
        "loop.cap_reached" => {
            o.key = "loop.cap_reached".into();
            o.title = "Turns reach their loop cap".into();
            o.note = format!("{} loops of {}", d["loops"], d["max_loops"]);
            o.work = 3;
        }
        "budget.over_limit" => {
            let m = word(&d["model"]);
            o.key = format!("budget.over_limit:{m}");
            o.title = format!("Calls to {m} pass the session's limit");
            o.note = format!("needed {} of {}", d["needed_usd"], d["limit_usd"]);
            (o.person, o.work) = (true, 2);
        }
        "approval.refused" => {
            let a = word(&d["act"]);
            o.key = format!("approval.refused:{a}");
            o.title = format!("Refused acts: {a}");
            o.note = format!("{a} refused for {}", word(&d["who"]));
        }
        "self.sealed.drift" => of_drift(&mut o, d),
        "secrets.failed" | "mcp.failed" => {
            o.key = kind.into();
            o.title = format!("{kind} rows keep coming");
            o.note = format!("{kind}: {}", word(&d["error"]));
            o.work = 3;
        }
        _ => return None,
    }
    Some(o)
}

/// The installed main fell below the sealed baseline in a family
/// (theseus-pw1q.21.7); the row holds counts and a family's name, never text.
fn of_drift(o: &mut Obs, d: &Value) {
    let fam = d["family"]
        .as_str()
        .map_or_else(|| "_total".into(), |f| word(&Value::from(f)));
    o.key = format!("sealed.drift:{fam}");
    o.title = format!("The sealed score drifted down in {fam}");
    o.note = format!(
        "{} of {} items, from {}",
        d["after"], d["items"], d["before"]
    );
    o.work = 3;
}

/// A label that says the judgment was wrong.
fn is_wrong(label: &Value) -> bool {
    match label {
        Value::String(s) => matches!(s.as_str(), "wrong" | "noise" | "rejected"),
        Value::Object(m) => m.contains_key("not"),
        _ => false,
    }
}

/// A turn that ran past its budget, or whose first token did.
fn of_slow(mut o: Obs, d: &Value) -> Option<Obs> {
    let elapsed = d["elapsed_ms"].as_u64()?;
    let first = d["first_token_ms"].as_u64().unwrap_or(0);
    let model = word(&d["model"]);
    if elapsed > TURN_BUDGET_MS {
        o.key = format!("turn.slow:{model}");
        o.title = format!("Turns on {model} run past {} s", TURN_BUDGET_MS / 1000);
        o.note = format!(
            "{elapsed} ms, {} ms over the turn budget",
            elapsed - TURN_BUDGET_MS
        );
    } else if first > FIRST_TOKEN_BUDGET_MS {
        o.key = format!("turn.first_token:{model}");
        o.title = format!("First tokens from {model} come late");
        o.note = format!(
            "{first} ms, {} ms over the first-token budget",
            first - FIRST_TOKEN_BUDGET_MS
        );
    } else {
        return None;
    }
    o.usd = d["cost_usd"].as_f64().unwrap_or(0.0);
    (o.person, o.work) = (true, 3);
    Some(o)
}

/// A learning report with failed judgments or many disagreements.
fn of_report(mut o: Obs, d: &Value) -> Option<Obs> {
    let r = &d["report"];
    let pack = word(&r["pack"]);
    let (calls, failed) = (r["calls"].as_u64()?, r["failed"].as_u64().unwrap_or(0));
    let (labeled, disagree) = (
        r["labeled"].as_u64().unwrap_or(0),
        r["disagreements"].as_u64().unwrap_or(0),
    );
    o.source = "learning".into();
    if failed > 0 {
        o.key = format!("judge.failed:{pack}");
        o.title = format!("Jev's {pack} judgments fail");
        o.note = format!("{failed} of {calls} {pack} calls failed in the learning report");
    } else if labeled >= 10 && disagree * 4 > labeled {
        o.key = format!("judge.disagree:{pack}");
        o.title = format!("Jev's {pack} disagrees with the owner's labels");
        o.note = format!("{disagree} of {labeled} labeled {pack} judgments disagree");
    } else {
        return None;
    }
    o.work = 3;
    Some(o)
}

/// Observations from the ledger in `(since_ms, until_ms]`, oldest first.
pub fn from_ledger(store: &Store, since_ms: u64, until_ms: u64) -> Result<Vec<Obs>> {
    let tags: Vec<String> = KINDS.iter().map(|k| ledger_kind(k)).collect();
    let mut out = Vec::new();
    let (mut before, mut seen) = (None, 0usize);
    loop {
        let page = Page {
            kind: kinds::LEDGER,
            tags: tags.clone(),
            after: None,
            before,
            since_ms: Some(since_ms),
            until_ms: Some(until_ms),
            limit: 256,
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
            if let Some(o) = of_row(r.position, &row) {
                out.push(o);
            }
        }
        match (got.more, got.first) {
            (true, Some(first)) if seen < MAX_ROWS => before = Some(first),
            _ => break,
        }
    }
    out.sort_by(|a, b| a.at_ms.cmp(&b.at_ms).then_with(|| a.id.cmp(&b.id)));
    Ok(out)
}

/// Health's warnings, as observations dated `date`: a disk under its
/// warning, a failed MCP server, a failed or missed import night.
pub fn from_health(h: &theseus_protocol::HealthResult, now_ms: u64, date: &str) -> Vec<Obs> {
    let mut out = Vec::new();
    let mut add = |what: &str, title: String, note: String, work: u8| {
        out.push(Obs {
            key: format!("health:{what}"),
            title,
            source: "health".into(),
            id: format!("health:{what}:{date}"),
            note,
            at_ms: now_ms,
            usd: 0.0,
            person: false,
            task_failed: false,
            work,
            graduate: None,
            pattern: None,
        });
    };
    if h.disk.state != "ok" {
        add(
            "disk",
            "The disk is low".into(),
            format!(
                "disk {}: {} MB free of {}",
                h.disk.state, h.disk.free_mb, h.disk.total_mb
            ),
            2,
        );
    }
    for m in h.mcp.iter().filter(|m| m.state == "failed") {
        add(
            &format!("mcp:{}", word(&Value::from(m.name.clone()))),
            format!("MCP server {} is failed", m.name),
            format!(
                "{} failed, {} crashes, {} errors",
                m.name, m.crashes, m.errors
            ),
            3,
        );
    }
    if let Some(w) = h.imported.sync.as_ref().and_then(|s| s.warn.clone()) {
        add("import_sync", "The import sync is failing".into(), w, 2);
    }
    out
}

/// A motif as an observation (theseus-pw1q.21.3): a cause keyed
/// `pattern:<scope>:<digest>`, one piece of evidence a night, carrying the
/// terms of its score. The motif's evidence is positions and numbers, never a
/// value.
pub fn of_motif(m: &crate::rsi::mine::Motif, now_ms: u64, date: &str) -> Obs {
    let cause = m.cause_key();
    let words: Vec<&str> = m.steps.iter().map(|s| s.id.as_str()).collect();
    let flow = words.join(" → ");
    let bench = if m.bench_born { " (bench-born)" } else { "" };
    let note = format!(
        "{} occurrences in {} sessions over {} days{bench}; ${:.2} and {:.0} s of model time saved per occurrence",
        m.support.occurrences, m.support.sessions, m.support.days, m.saving.usd, m.saving.model_secs
    );
    Obs {
        key: cause.clone(),
        title: format!("Recurring sequence: {flow}"),
        source: "pattern".into(),
        id: format!("{cause}:{date}"),
        note: note.clone(),
        at_ms: m.last_ms.max(1).min(now_ms),
        usd: 0.0,
        person: false,
        task_failed: false,
        work: 1,
        graduate: None,
        pattern: Some(Pattern {
            score: super::state::PatternScore {
                occurrences: m.support.occurrences,
                usd: m.saving.usd,
                secs: m.saving.model_secs,
                decisions: m.saving.decisions,
                work: 1,
            },
            brief: format!(
                "A recurring sequence of tool calls (`{cause}`, spec digest {}): {flow}. {note}. \
                 The miner wrote its skeleton; nothing starts from this item yet: minting a tool \
                 from it is a later step.",
                m.digest
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn provider_error(class: &str) -> LedgerRow {
        LedgerRow::named(
            "provider.error",
            None,
            None,
            json!({"class": class, "provider": "anthropic", "elapsed_ms": 3, "message": "not sent"}),
        )
    }

    /// A call the daemon's stop kept back is no provider's error: it files
    /// no cause, where a real answer of the same provider does.
    #[test]
    fn a_call_the_stop_kept_back_is_no_cause() {
        assert!(of_row(1, &provider_error("stopping")).is_none());
        let o = of_row(2, &provider_error("overloaded")).expect("a real provider error");
        assert_eq!(o.key, "provider.error:overloaded:anthropic");
    }
}

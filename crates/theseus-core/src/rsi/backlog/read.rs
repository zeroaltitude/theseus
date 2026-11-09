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

/// What a failed action's producer names, for a key: `backend:name` (as
/// `harness:fs.read` or `provider:anthropic`) when the backend is one whose
/// second part is a tool, a provider or a hand's backend, else the backend
/// alone. The first word alone put every in-process tool's failure in one
/// cause (`harness`), which no one fix can close; `wrapper:<pid>` and
/// `reconciler:<why>` stay a word, since their second part is not a stable
/// name.
fn producer(v: &Value) -> String {
    const NAMED: &[&str] = &["harness", "hand", "provider"];
    let s = v.as_str().unwrap_or("").trim();
    let head = word(v);
    let name = s
        .split_once(':')
        .filter(|_| NAMED.contains(&head.as_str()))
        .map(|(_, rest)| word(&Value::from(rest)))
        .filter(|n| n != "unknown");
    match name {
        Some(n) => format!("{head}:{n}"),
        None => head,
    }
}

/// A reason's key: its first token (up to whitespace) as up to two
/// `:`-joined words, so `provider:invalid_request` stays itself and
/// `provider:rate_limited` is another cause. `word` alone cuts at the first
/// `:`, which filed every provider failure of a turn under `provider`, a cause
/// that said nothing of which failure it was. A reason with prose after its
/// first colon (`timeout: the provider did not answer`) keys as before.
fn reason_word(v: &Value) -> String {
    let token = v.as_str().unwrap_or("").split_whitespace().next();
    let parts: Vec<String> = token
        .unwrap_or("")
        .split(':')
        .filter(|p| !p.is_empty())
        .take(2)
        .map(|p| word(&Value::from(p)))
        .collect();
    if parts.is_empty() {
        "unknown".into()
    } else {
        parts.join(":")
    }
}

/// The start of a message for a note, as one line cut to `max` characters:
/// `word` leaves a provider's `invalid request (400): ...` as `invalid`,
/// which hid what the provider refused. `unknown` when it said nothing.
fn gist(v: &Value, max: usize) -> String {
    let text = line(v.as_str().unwrap_or(""), max);
    if text.is_empty() {
        "unknown".into()
    } else {
        text
    }
}

/// What a failed turn's reason says beyond its key, for the note: the whole
/// reason as one line cut to 200 characters, so a `kernel: <why>` refusal
/// (one cause, by its key) still says which refusal it was. Nothing when the
/// reason is only its key or is absent.
fn reason_said(v: &Value, key: &str) -> String {
    let text = line(v.as_str().unwrap_or(""), 200);
    if text.is_empty() || text == key {
        String::new()
    } else {
        format!(": {text}")
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

/// A producer's words as one line: whitespace collapsed, cut to `max`
/// characters. Empty when it said nothing.
fn line(text: &str, max: usize) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    one.chars().take(max).collect()
}

/// Where a failed action went and why, for its observation's note: the tool,
/// the outbox target a post was bound for, and the producer's own reason
/// (`detail.error`: a refusal's words, Discord's answer). A settle that wrote
/// none says nothing. Without this a failing post reads as "failed after 0
/// ms" and the item that fixes it cannot tell which refusal it is. The row's
/// `outcome` is left out: the note already says failed.
fn action_why(d: &Value) -> String {
    let mut parts = Vec::new();
    for (label, v) in [("tool", &d["tool"]), ("to", &d["outbox"])] {
        if let Some(s) = v.as_str().filter(|s| !s.is_empty()) {
            parts.push(format!("{label} {s}"));
        }
    }
    let why = line(d["detail"]["error"].as_str().unwrap_or(""), 240);
    if !why.is_empty() {
        parts.push(format!("error: {why}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join("; "))
    }
}

/// A failed action's observation. A job's own non-zero exit is its command's
/// answer, not a fault of ours, so it is none. Timeouts, signals, stops and
/// jobs that never started are faults, and each is its own cause; a row from
/// before the ledger named the failure keeps the old key. The pid in a
/// producer (`wrapper:<pid>`) never splits a cause: `producer` stops at `:`
/// for a backend whose second part is not a name.
fn of_action_failed(mut o: Obs, d: &Value) -> Option<Obs> {
    let p = producer(&d["producer"]);
    o.key = format!("action.failed:{p}");
    o.title = calls_failing(&p);
    o.note = format!(
        "action {} failed after {} ms{}",
        d["correlation_id"],
        d["duration_ms"],
        action_why(d)
    );
    if let Some(f) = d["failure"].as_str() {
        if f == "exit" {
            return None;
        }
        let f = word(&d["failure"]);
        o.key = format!("action.failed:{p}:{f}");
        o.title = format!("{}: {f}", o.title);
        o.note = format!("{} ({f})", o.note);
    }
    o.usd = d["cost_usd"].as_f64().unwrap_or(0.0);
    o.task_failed = true;
    Some(o)
}

/// What one ledger row shows, if it shows a problem.
pub fn of_row(pos: u64, row: &LedgerRow) -> Option<Obs> {
    let d = &row.data;
    let kind = row.kind.as_str();
    let mut o = obs(kind, pos, row);
    match kind {
        "action.failed" => return of_action_failed(o, d),
        "turn.failed" => {
            let r = reason_word(&d["reason"]);
            o.key = format!("turn.failed:{r}");
            o.title = format!("Turns failing with {r}");
            o.note = format!(
                "turn failed ({r}) after {} loops{}",
                d["loops"],
                reason_said(&d["reason"], &r)
            );
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
            // The floor under the sensitive names refusing a request is the
            // privacy policy working, with nothing sent: the provider never
            // failed, so there is no fault of its to file.
            if c == crate::data_gate::DATA_GATE_CLASS {
                return None;
            }
            o.key = format!("provider.error:{c}:{p}");
            o.title = errors_from(&p, &c);
            o.note = format!(
                "{p} {c} after {} ms: {}",
                d["elapsed_ms"],
                gist(&d["message"], 160)
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
            o.title = format!("Approvals refused for {a}");
            o.note = format!("{a} refused for {}", word(&d["who"]));
        }
        "self.sealed.drift" => of_drift(&mut o, d),
        "secrets.failed" | "mcp.failed" => {
            o.key = kind.into();
            o.title = failing_rows(kind).into();
            o.note = format!("{kind}: {}", word(&d["error"]));
            o.work = 3;
        }
        _ => return None,
    }
    Some(o)
}

/// The installed main fell below the sealed baseline in a family
/// (theseus-pw1q.21.7); the row holds counts and a family's name, never text.
/// The titles are the owner's plain words, not the ledger's labels (the
/// owner, 2026-10-22: "Harness tool calls failing", not "Calls from harness
/// fail"; theseus-jsdj5). `plain_title` reads an item filed under the old
/// words the same way.
fn calls_failing(producer: &str) -> String {
    let mut c = producer.chars();
    let up: String = c
        .next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default();
    format!("{up} tool calls failing")
}

fn errors_from(provider: &str, class: &str) -> String {
    format!("Errors from {provider}: {class}")
}

fn failing_rows(kind: &str) -> &'static str {
    match kind {
        "secrets.failed" => "Secret lookups failing",
        _ => "MCP server calls failing",
    }
}

/// A stored item's title in today's words: an item filed before
/// theseus-jsdj5 keeps its old title in its record, and every read of the
/// backlog (and the needs-you card's asker) shows it plain.
pub(crate) fn plain_title(title: &str) -> String {
    let one = |w: &str| !w.is_empty() && !w.contains(' ');
    if let Some(p) = title
        .strip_prefix("Calls from ")
        .and_then(|t| t.strip_suffix(" fail"))
    {
        if one(p) {
            return calls_failing(p);
        }
    }
    if let Some(r) = title.strip_prefix("Turns fail with ").filter(|r| one(r)) {
        return format!("Turns failing with {r}");
    }
    if let Some(a) = title.strip_prefix("Refused acts: ").filter(|a| one(a)) {
        return format!("Approvals refused for {a}");
    }
    if let Some(k) = title.strip_suffix(" rows keep coming") {
        if matches!(k, "secrets.failed" | "mcp.failed") {
            return failing_rows(k).into();
        }
    }
    if let Some((p, c)) = title
        .split_once(" answers ")
        .filter(|(p, c)| one(p) && one(c))
    {
        return errors_from(p, c);
    }
    title.to_string()
}

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

/// A turn whose first token came late. A long turn is work, not a defect
/// (the owner, 2026-10-25), so its length files nothing.
fn of_slow(mut o: Obs, d: &Value) -> Option<Obs> {
    // A row without `elapsed_ms` is not a finished turn's record.
    d["elapsed_ms"].as_u64()?;
    let first = d["first_token_ms"].as_u64().unwrap_or(0);
    let model = word(&d["model"]);
    if first > FIRST_TOKEN_BUDGET_MS {
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
    let (calls, failed_all) = (r["calls"].as_u64()?, r["failed"].as_u64().unwrap_or(0));
    // `failed` is all-time and never falls: key on the recent count, and on
    // the total only for a report stored before there was one.
    let failed = r["failed_recent"].as_u64().unwrap_or(failed_all);
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
    use super::*;
    use serde_json::json;

    fn title_of(kind: &str, data: Value) -> String {
        let row = LedgerRow {
            at_unix_ms: 1,
            kind: kind.into(),
            session_id: None,
            turn_id: None,
            data,
        };
        of_row(7, &row).unwrap().title
    }

    /// A backlog item's title is in plain words, and an item filed under the
    /// old words reads the same (the owner, 2026-10-22; theseus-jsdj5).
    #[test]
    fn a_backlog_title_is_plain_words_old_or_new() {
        let calls = title_of("action.failed", json!({"producer": "harness"}));
        assert_eq!(calls, "Harness tool calls failing");
        assert_eq!(plain_title("Calls from harness fail"), calls);
        let errors = title_of(
            "provider.error",
            json!({"class": "overloaded", "provider": "anthropic"}),
        );
        assert_eq!(errors, "Errors from anthropic: overloaded");
        assert_eq!(plain_title("anthropic answers overloaded"), errors);
        let turns = title_of("turn.failed", json!({"reason": "timeout"}));
        assert_eq!(plain_title("Turns fail with timeout"), turns);
        let refused = title_of("approval.refused", json!({"act": "fs.edit"}));
        assert_eq!(refused, "Approvals refused for fs.edit");
        assert_eq!(plain_title("Refused acts: fs.edit"), refused);
        for kind in ["secrets.failed", "mcp.failed"] {
            let t = title_of(kind, json!({"error": "x"}));
            assert_eq!(plain_title(&format!("{kind} rows keep coming")), t);
            assert!(!t.contains(kind), "{t}");
        }
        for kept in [
            "The import sync is failing",
            "Calls from two words fail",
            "Fix the tides",
        ] {
            assert_eq!(plain_title(kept), kept);
        }
    }

    fn provider_error(class: &str) -> LedgerRow {
        LedgerRow::named(
            "provider.error",
            None,
            None,
            json!({"class": class, "provider": "zai", "elapsed_ms": 0, "message": "refused"}),
        )
    }

    /// The floor under the sensitive names refusing a request to a restricted
    /// model is the privacy policy working, with nothing sent: it files no
    /// cause (`provider.error:data_gate:zai`, "zai answers data_gate"), while
    /// a real provider failure on the same provider still does.
    #[test]
    fn a_data_gate_refusal_is_no_providers_fault() {
        assert_eq!(of_row(1, &provider_error("data_gate")), None);
        let real = of_row(2, &provider_error("overloaded")).expect("a real failure");
        assert_eq!(real.key, "provider.error:overloaded:zai");
    }

    /// A call the daemon's stop kept back is no provider's error: it files
    /// no cause, where a real answer of the same provider does.
    #[test]
    fn a_call_the_stop_kept_back_is_no_cause() {
        let mut stopped = provider_error("stopping");
        stopped.data["provider"] = json!("anthropic");
        assert!(of_row(1, &stopped).is_none());
        let mut real = provider_error("overloaded");
        real.data["provider"] = json!("anthropic");
        let o = of_row(2, &real).expect("a real provider error");
        assert_eq!(o.key, "provider.error:overloaded:anthropic");
    }

    fn ended(data: Value) -> LedgerRow {
        LedgerRow::named("turn.ended", None, None, data)
    }

    /// A long turn is work, not a defect (the owner, 2026-10-25): its first
    /// token is what is judged, and a slow one is no longer masked by the
    /// turn's length.
    #[test]
    fn a_long_turn_with_a_late_first_token_files_the_first_token() {
        let row =
            ended(json!({"elapsed_ms": 400_000, "first_token_ms": 20_000, "model": "quill9"}));
        let o = of_row(1, &row).expect("a late first token");
        assert_eq!(o.key, "turn.first_token:quill9");
        assert!(o.note.contains("20000 ms"), "{}", o.note);
    }

    #[test]
    fn a_long_turn_with_a_fast_first_token_files_nothing() {
        let row = ended(json!({"elapsed_ms": 400_000, "first_token_ms": 500, "model": "quill9"}));
        assert_eq!(of_row(1, &row), None);
    }

    /// A short turn with a slow first token files as it always did.
    #[test]
    fn a_short_turn_with_a_late_first_token_still_files_it() {
        let row = ended(json!({"elapsed_ms": 15_000, "first_token_ms": 20_000, "model": "quill9"}));
        let o = of_row(1, &row).expect("a late first token");
        assert_eq!(o.key, "turn.first_token:quill9");
        assert_eq!(o.work, 3);
    }

    /// Reaching the loop cap is normal for long work (`on_max_loops` is
    /// `notify`), so its notice files no cause; old rows still arrive.
    #[test]
    fn a_loop_cap_notice_files_no_cause() {
        let row = LedgerRow::named(
            "loop.cap_reached",
            None,
            None,
            json!({"loops": 40, "max_loops": 40}),
        );
        assert_eq!(of_row(1, &row), None);
    }

    /// A `turn.ended` row with no `elapsed_ms` is not a finished turn's
    /// record: it files nothing, even with a late first token.
    #[test]
    fn a_turn_ended_row_without_elapsed_files_nothing() {
        let row = ended(json!({"first_token_ms": 20_000, "model": "quill9"}));
        assert_eq!(of_row(1, &row), None);
    }

    fn report_obs(report: Value) -> Option<Obs> {
        of_row(
            7,
            &LedgerRow::named("judge.report", None, None, json!({"report": report})),
        )
    }

    /// The report's `failed` is all-time and never falls, so keying on it
    /// re-filed one old burst every night (judge.failed:people.v1). The cause
    /// follows `failed_recent`; a report stored before it falls back to the
    /// total.
    #[test]
    fn an_old_burst_of_failed_judgments_is_not_filed_again() {
        let stopped = json!({"pack": "people.v1", "calls": 9000, "failed": 12, "failed_recent": 0});
        assert!(report_obs(stopped).is_none());
        let going = json!({"pack": "people.v1", "calls": 9000, "failed": 12, "failed_recent": 3});
        let o = report_obs(going).unwrap();
        assert_eq!(o.key, "judge.failed:people.v1");
        assert!(o.note.starts_with("3 of 9000"), "{}", o.note);
        let legacy = json!({"pack": "people.v1", "calls": 9000, "failed": 12});
        assert_eq!(report_obs(legacy).unwrap().key, "judge.failed:people.v1");
    }

    fn failed_job(pid: u32, failure: Option<&str>) -> LedgerRow {
        let mut data = json!({
            "correlation_id": "act_1", "producer": format!("wrapper:{pid}"),
            "duration_ms": 5, "outcome": "failed",
        });
        if let Some(f) = failure {
            data["failure"] = json!(f);
        }
        LedgerRow::named("action.failed", None, None, data)
    }

    /// A command's own non-zero exit is its answer, not a fault of ours.
    #[test]
    fn a_jobs_own_non_zero_exit_is_not_a_cause() {
        assert!(of_row(1, &failed_job(10, Some("exit"))).is_none());
    }

    /// A timeout and a job that never started are each a cause of their own,
    /// and the wrapper's pid never splits one.
    #[test]
    fn a_job_that_timed_out_or_never_started_is_its_own_cause_whatever_the_pid() {
        let key = |pid, f| of_row(1, &failed_job(pid, Some(f))).unwrap().key;
        assert_eq!(key(10, "timeout"), "action.failed:wrapper:timeout");
        assert_eq!(key(99, "timeout"), key(10, "timeout"));
        assert_eq!(key(10, "spawn"), "action.failed:wrapper:spawn");
        assert_ne!(key(10, "spawn"), key(10, "timeout"));
    }

    /// A row from before the ledger named failures keeps the old key.
    #[test]
    fn a_failed_row_without_a_failure_keeps_the_old_key() {
        let o = of_row(1, &failed_job(10, None)).unwrap();
        assert_eq!(o.key, "action.failed:wrapper");
        assert_eq!(o.title, "Wrapper tool calls failing");
    }

    fn failed_action_key(producer: &str) -> String {
        let row = LedgerRow::named(
            "action.failed",
            None,
            None,
            json!({"producer": producer, "duration_ms": 0, "correlation_id": "act_1"}),
        );
        of_row(1, &row)
            .expect("a failed action is an observation")
            .key
    }

    /// The first word of a producer put every in-process tool's failure in
    /// one cause, `harness`, which no single fix can close.
    #[test]
    fn failed_harness_calls_of_two_tools_are_two_causes() {
        assert_eq!(
            failed_action_key("harness:fs.read"),
            "action.failed:harness:fs.read"
        );
        assert_eq!(
            failed_action_key("harness:self.propose_change"),
            "action.failed:harness:self.propose_change"
        );
        assert_ne!(
            failed_action_key("harness:task.close"),
            failed_action_key("harness:fs.read")
        );
    }

    /// A provider and a hand's backend are names; a wrapper's pid and a
    /// reconciler's reason are not, so they stay one word.
    #[test]
    fn a_provider_is_named_but_a_wrapper_pid_is_not() {
        assert_eq!(
            failed_action_key("provider:anthropic"),
            "action.failed:provider:anthropic"
        );
        assert_eq!(
            failed_action_key("hand:lambda"),
            "action.failed:hand:lambda"
        );
        assert_eq!(failed_action_key("wrapper:4242"), "action.failed:wrapper");
        assert_eq!(
            failed_action_key("reconciler:lost"),
            "action.failed:reconciler"
        );
        assert_eq!(failed_action_key("hands:reaper"), "action.failed:hands");
        assert_eq!(failed_action_key("proc"), "action.failed:proc");
        assert_eq!(failed_action_key("harness"), "action.failed:harness");
    }

    fn note_of(kind: &str, data: Value) -> String {
        of_row(7, &LedgerRow::named(kind, None, None, data))
            .unwrap()
            .note
    }

    /// A failed post's note says where it went and why it was refused, so the
    /// item filed for it can tell one refusal from another. The kernel's own
    /// row says only `outcome: failed`, which the note already says, so a row
    /// with none of the rest reads as before.
    #[test]
    fn a_failed_post_note_names_its_target_and_reason() {
        let note = note_of(
            "action.failed",
            json!({"producer": "discord", "correlation_id": "out_1", "duration_ms": 0,
                   "tool": "outbox.post", "outbox": "discord:channel:42", "outcome": "failed",
                   "detail": {"error": "not bound here any more (channel:42)"}}),
        );
        for needle in [
            "out_1",
            "outbox.post",
            "discord:channel:42",
            "not bound here any more (channel:42)",
        ] {
            assert!(note.contains(needle), "{needle} missing from {note}");
        }
        assert!(!note.contains("outcome"), "{note}");
        let bare = note_of(
            "action.failed",
            json!({"producer": "proc", "correlation_id": "act_1", "duration_ms": 10,
                   "outcome": "failed"}),
        );
        assert_eq!(bare, "action \"act_1\" failed after 10 ms");
    }

    /// The producer's words are one line of at most 240 characters.
    #[test]
    fn a_failed_posts_reason_is_one_short_line() {
        let note = note_of(
            "action.failed",
            json!({"producer": "discord", "correlation_id": "out_2", "duration_ms": 0,
                   "detail": {"error": format!("refused\n  twice {}", "x".repeat(500))}}),
        );
        assert!(note.contains("refused twice xxx"), "{note}");
        assert!(!note.contains('\n'), "{note}");
        assert!(
            note.chars().count() < 340,
            "{} characters",
            note.chars().count()
        );
    }

    fn turn_failed(reason: &str) -> Obs {
        let row = LedgerRow::named(
            "turn.failed",
            None,
            None,
            json!({"loops": 2, "reason": reason, "cost_usd": 0.1}),
        );
        of_row(1, &row).expect("a failed turn is an observation")
    }

    /// A turn's `provider:<class>` reason was cut at its colon: every provider
    /// failure keyed `turn.failed:provider`, one cause for an invalid request,
    /// a rate limit and a timeout alike.
    #[test]
    fn a_failed_turn_keeps_the_providers_class_in_its_cause() {
        let invalid = turn_failed("provider:invalid_request");
        assert_eq!(invalid.key, "turn.failed:provider:invalid_request");
        assert_eq!(invalid.title, "Turns failing with provider:invalid_request");
        let limited = turn_failed("provider:rate_limited");
        assert_eq!(limited.key, "turn.failed:provider:rate_limited");
        assert_ne!(invalid.key, limited.key);
    }

    /// A reason with prose after its first colon, or none, keys as before.
    #[test]
    fn a_reason_with_prose_or_no_class_keys_as_before() {
        assert_eq!(
            turn_failed("timeout: the provider did not answer").key,
            "turn.failed:timeout"
        );
        assert_eq!(turn_failed("over_limit").key, "turn.failed:over_limit");
        assert_eq!(turn_failed("provider:").key, "turn.failed:provider");
        assert_eq!(turn_failed("").key, "turn.failed:unknown");
    }

    /// The provider's refusal reads in the evidence, not its first word.
    #[test]
    fn a_provider_errors_note_says_what_the_provider_refused() {
        let mut row = provider_error("invalid_request");
        row.data["provider"] = json!("anthropic");
        row.data["message"] = json!(
            "invalid request (400): invalid_request_error: messages.1: a tool use\nwas found without a result"
        );
        let o = of_row(2, &row).unwrap();
        assert_eq!(o.key, "provider.error:invalid_request:anthropic");
        assert!(
            o.note
                .contains("messages.1: a tool use was found without a result"),
            "{}",
            o.note
        );
    }

    /// A kernel failure's evidence says why the kernel refused: the key still
    /// groups every refusal as one cause, but the note carries the message
    /// (long ones cut), so the fixing run sees what to fix. A reason that is
    /// only its own key, or none, adds nothing to repeat.
    #[test]
    fn a_failed_turns_note_keeps_the_reason() {
        let o = turn_failed("kernel: execution exe_1 is cancelled, not runnable");
        assert_eq!(o.key, "turn.failed:kernel");
        assert_eq!(
            o.note,
            "turn failed (kernel) after 2 loops: kernel: execution exe_1 is cancelled, not runnable"
        );
        let long = turn_failed(&format!("kernel: {}", "x".repeat(500)));
        assert!(long.note.chars().count() < 260, "{}", long.note);
        assert_eq!(
            turn_failed("provider:invalid_request").note,
            "turn failed (provider:invalid_request) after 2 loops"
        );
        assert_eq!(turn_failed("").note, "turn failed (unknown) after 2 loops");
    }
}

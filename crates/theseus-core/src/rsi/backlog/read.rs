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
    of_row_in(pos, row, false)
}

/// Whether a session is a task's: its turns are the agent's own work (many
/// loops of tool calls, minutes long by design), not a chat turn.
fn is_task_session(store: &Store, session: &str) -> bool {
    store
        .get_session::<crate::session::SessionRecord>(session)
        .ok()
        .flatten()
        .is_some_and(|r| r.task.is_some())
}

/// [`of_row`], told whether the row is a task session's turn. A task's turn
/// is not held to the chat turn budget: the backlog's own fix tasks run for
/// minutes on their model, and counting them as slow turns files a fix task
/// for the slowness of the fix tasks, night after night.
fn of_row_in(pos: u64, row: &LedgerRow, task: bool) -> Option<Obs> {
    let d = &row.data;
    let kind = row.kind.as_str();
    let mut o = obs(kind, pos, row);
    match kind {
        "action.failed" => {
            let p = word(&d["producer"]);
            o.key = format!("action.failed:{p}");
            o.title = calls_failing(&p);
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
            o.title = format!("Turns failing with {r}");
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
            let (c, p) = (word(&d["class"]), word(&d["provider"]));
            o.key = format!("provider.error:{c}:{p}");
            o.title = errors_from(&p, &c);
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
        "turn.ended" => return of_slow(o, d, task),
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

/// A turn that ran past its budget, or whose first token did. A task's turn
/// has no wall-time budget (its first token still does).
fn of_slow(mut o: Obs, d: &Value, task: bool) -> Option<Obs> {
    let elapsed = d["elapsed_ms"].as_u64()?;
    let first = d["first_token_ms"].as_u64().unwrap_or(0);
    let model = word(&d["model"]);
    if elapsed > TURN_BUDGET_MS && !task {
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
            let task = row.kind == "turn.ended"
                && row
                    .session_id
                    .as_deref()
                    .is_some_and(|s| is_task_session(store, s));
            if let Some(o) = of_row_in(r.position, &row, task) {
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

    /// A task's long turn is its work, not a slow turn: the night's read
    /// files `turn.slow` for a chat turn past the budget and not for the
    /// backlog's own fix tasks, which would otherwise file themselves.
    #[test]
    fn a_task_sessions_long_turn_is_not_a_slow_turn() {
        use crate::session::{SessionRecord, TaskOf};
        use theseus_protocol::SessionKind;
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(&d.path().join("store")).unwrap();
        let chat = SessionRecord::new(SessionKind::Conversation, None);
        let mut task = SessionRecord::new(SessionKind::Task, None);
        task.task = Some(TaskOf {
            parent_session: chat.session_id.clone(),
            parent_execution: "exe_none".into(),
            by: "call_none".into(),
            target: None,
            arrangement: None,
            check: None,
            notify_done: false,
        });
        for s in [&chat, &task] {
            store.put_session(&s.session_id, s).unwrap();
        }
        let now = theseus_protocol::now_unix_ms();
        for (s, at) in [(&chat, now - 2_000), (&task, now - 1_000)] {
            let mut r = LedgerRow::named(
                "turn.ended",
                Some(&s.session_id),
                None,
                json!({"elapsed_ms": 600_000, "first_token_ms": 500, "model": "claudesonnet55"}),
            );
            r.at_unix_ms = at;
            store.append_ledger(&r).unwrap();
        }
        let got = from_ledger(&store, now - 10_000, now).unwrap();
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].key, "turn.slow:claudesonnet55");
        assert_eq!(got[0].at_ms, now - 2_000);
    }
}

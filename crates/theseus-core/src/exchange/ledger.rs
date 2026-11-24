//! The measuring row (`context.turn`, issue 44 step 1): one per turn in a
//! private place, written in the turn's last frame, ids, kinds and counts
//! only, never text. It answers the design's questions that need no model:
//! whether the place's previous exchange was in the context sent, the
//! person's newest exchange in each other private place, what the sent
//! context was made of, a cold start and its cause, the things the index's
//! extractor named, the judge's unread answers, gaps, overlaps, and how the
//! terminal's peer was classed. `report` sums the rows.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::exchange::{ExchangeAnswers, ExchangeReportParams, ExchangeReportResult};
use theseus_protocol::LedgerKind;
use theseus_store::pages::{ledger_kind, ledger_kind_session};
use theseus_store::{kinds, Page, Store as _};

use super::{exchange, newest, Exchange, Opener, Peer, Root, OWNER};
use crate::fact::Fact;
use crate::ledger::LedgerRow;
use crate::node::{Body, Origin};
use crate::provider::{json_len, ProviderRequest};
use crate::store::Store;
use crate::stub::Kind;

/// The bytes a token is estimated at (the design's figure, est.).
const BYTES_PER_TOKEN: f64 = 3.6;
/// A gap after a turn no person opened, within which the person's reply is
/// counted as answering it.
pub const ANSWERED_WITHIN_MS: u64 = 10 * 60_000;
/// An exchange still open after this long is a turn that never closed, not a
/// turn in progress.
const OPEN_FOR_MS: u64 = 6 * 3_600_000;
/// Why a row's previous exchange was missed, when the row can tell
/// (`prev_why`): the place's `/new` lay between them, or the first call was
/// never measured. Any other miss says nothing: a seal, a gate, or a real miss.
pub const WHY_NEW: &str = "new";
pub const WHY_NOT_MEASURED: &str = "not_measured";
/// The row's fields `add` counts by their value: kinds, classes and flags,
/// never a name or an id. A field a later build adds is counted as present
/// until it is named here.
const BY_VALUE: [&str; 11] = [
    "opened_by",
    "peer",
    "person",
    "prev",
    "prev_why",
    "prev_same_session",
    "overlap",
    "answered",
    "situation",
    "trigger",
    "cold",
];
/// The judge's four answers nothing reads, and its `kind`, by question id.
const JUDGED: [&str; 5] = [
    "fragment",
    "mentions_other_conversation",
    "wants_fresh_look",
    "addressed_task",
    "kind",
];

/// The sent context of a turn's first call, in tokens (est.), after
/// clearing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sent {
    /// The system blocks but the project's files.
    pub header: u64,
    pub tools: u64,
    pub files: u64,
    pub replies: u64,
    pub calls: u64,
    pub words: u64,
    pub results_whole: u64,
    pub results_cleared: u64,
    pub recall: u64,
    /// The rest of the person's side: harness lines, summaries, tail blocks.
    pub other: u64,
    /// The compiler's own estimate of the whole.
    pub total: u64,
}

fn tok(bytes: u64) -> u64 {
    (bytes as f64 / BYTES_PER_TOKEN).ceil() as u64
}

fn str_len(v: &Value) -> u64 {
    v.as_str().map_or(0, |s| s.len() as u64)
}

/// A `tool_result` block's bytes, as `Census` counts them.
fn result_len(b: &Value) -> u64 {
    match &b["content"] {
        Value::String(s) => s.len() as u64,
        Value::Array(inner) => inner
            .iter()
            .map(|x| match x.get("type").and_then(Value::as_str) {
                Some("text") => str_len(&x["text"]),
                Some("image" | "document") => 0,
                _ => json_len(x),
            })
            .sum(),
        Value::Null => 0,
        other => json_len(other),
    }
}

/// What the view holds: the compilation's prefix, and the tail after it.
pub struct View<'a> {
    pub includes: &'a [String],
    pub as_of: u64,
}

impl View<'_> {
    fn holds(&self, ids: &HashSet<&str>, position: u64, id: &str) -> bool {
        position > self.as_of || ids.contains(id)
    }
}

/// `request`'s parts, by the wire's own block types, with the person's
/// words and recall's notes read from the nodes the view holds (a turn's
/// pending recall among them). `cleared` are the results the compile
/// cleared, by node id; `files` the project files' bytes; `total` the
/// compiler's estimate; `json` its census's JSON bytes, of which what the
/// messages do not hold is the tools' schemas (no second walk of them).
pub fn sent(
    request: &ProviderRequest,
    nodes: &[(u64, crate::stub::Stub)],
    view: &View<'_>,
    (cleared, files, total, json): (&HashSet<&str>, u64, u64, u64),
) -> Sent {
    let ids: HashSet<&str> = view.includes.iter().map(String::as_str).collect();
    let (mut words, mut recall, mut cleared_calls) = (0, 0, HashSet::new());
    for (_, n) in nodes.iter().filter(|(p, n)| view.holds(&ids, *p, &n.id)) {
        match (n.kind, n.origin) {
            (Kind::UserMessage, Origin::Operator) => {
                if let Body::UserMessage { text, .. } = &n.body {
                    words += text.len() as u64;
                }
            }
            (Kind::Recall, _) => {
                if let Body::Recall { items, .. } = &n.body {
                    recall += items.iter().map(|r| r.tokens).sum::<u64>();
                }
            }
            (Kind::ToolResult, _) if cleared.contains(n.id.as_str()) => {
                if let Body::ToolResult { tool_use_id, .. } = &n.body {
                    cleared_calls.insert(tool_use_id.clone());
                }
            }
            _ => {}
        }
    }
    let mut s = Sent {
        total,
        ..Sent::default()
    };
    let (mut replies, mut calls, mut whole, mut gone, mut user) = (0, 0, 0, 0, 0);
    for m in &request.messages {
        let assistant = m["role"].as_str() == Some("assistant");
        for b in m["content"].as_array().into_iter().flatten() {
            match b.get("type").and_then(Value::as_str) {
                Some("text") if assistant => replies += str_len(&b["text"]),
                Some("thinking") => replies += str_len(&b["thinking"]),
                Some("text") => user += str_len(&b["text"]),
                Some("tool_use") => calls += str_len(&b["name"]) + json_len(&b["input"]),
                Some("tool_result") => match b["tool_use_id"].as_str() {
                    Some(id) if cleared_calls.contains(id) => gone += result_len(b),
                    _ => whole += result_len(b),
                },
                _ => {}
            }
        }
    }
    let system: u64 = request.system.iter().map(|b| str_len(&b["text"])).sum();
    s.header = tok(system.saturating_sub(files));
    s.files = tok(files);
    s.tools = tok(json.saturating_sub(calls + whole + gone));
    (s.replies, s.calls) = (tok(replies), tok(calls));
    (s.results_whole, s.results_cleared) = (tok(whole), tok(gone));
    (s.words, s.recall) = (tok(words), recall);
    s.other = tok(user.saturating_sub(words)).saturating_sub(recall);
    s
}

/// What a turn's first compile saw, held for its row.
#[derive(Debug, Clone, Default)]
pub struct Seen {
    pub compilation: String,
    pub sent: Sent,
    pub situation: Option<String>,
    pub trigger: Option<String>,
    /// The build's own check that the provider's cache had lapsed.
    pub cold: bool,
}

/// How the place's previous exchange stood in the context sent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Prev {
    /// The place had no exchange before.
    #[default]
    None,
    /// Its message or reply was in this request's transcript; for a view, the view showed it.
    Transcript,
    /// It ran in another session, and the place's tail block carried it (issue 43's block, gone with
    /// the transcript: an old row's value).
    PlaceTail,
    Missed,
}

/// The person's newest exchange in another private place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Elsewhere {
    pub place: String,
    pub exchange: String,
    pub age_ms: u64,
    pub in_view: bool,
}

/// One `context.turn` row.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Row {
    pub exchange: String,
    pub place: String,
    pub opened_by: Option<Opener>,
    pub peer: Option<Peer>,
    pub person: bool,
    pub ask_chars: Option<u64>,
    pub prev: Prev,
    pub prev_exchange: Option<String>,
    /// Why a `missed` previous exchange was missed, when the row can tell
    /// ([`WHY_NEW`], [`WHY_NOT_MEASURED`]); written only when set, and absent
    /// from the rows of a build before it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_why: Option<String>,
    pub prev_same_session: Option<bool>,
    pub gap_ms: Option<u64>,
    /// Another session of the place was mid-turn when this one began.
    pub overlap: bool,
    /// This person's message answered, within ten minutes, a turn no person
    /// opened: what had opened it.
    pub answered: Option<Opener>,
    pub elsewhere: Vec<Elsewhere>,
    pub sent: Sent,
    pub situation: Option<String>,
    pub trigger: Option<String>,
    pub cold: bool,
    pub cache_read: u64,
    pub cache_write: u64,
    /// The extractor's things in the message, and in the reply before it, by
    /// kind; none when the index did not answer in time.
    pub things: Option<BTreeMap<String, u64>>,
    pub reply_things: Option<BTreeMap<String, u64>>,
    /// The judge's answers by question, as recorded by the row's writing.
    pub judge: BTreeMap<String, String>,
}

impl Fact for Row {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContextTurn);

    fn row(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// The extractor's terms (`type:value`), counted by type.
pub fn by_kind(terms: &[String]) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for t in terms {
        if let Some((kind, _)) = t.split_once(':') {
            *out.entry(kind.to_string()).or_insert(0) += 1;
        }
    }
    out
}

/// Whether node `id` of `session` was in `view` of `session`'s request.
fn in_view(store: &Store, session: &str, view: &View<'_>, id: &str) -> bool {
    match store.inner().latest_by_key(kinds::NODE, id) {
        Ok(Some(r)) => {
            let ids: HashSet<&str> = view.includes.iter().map(String::as_str).collect();
            r.scope.as_deref() == Some(session) && view.holds(&ids, r.position, id)
        }
        _ => false,
    }
}

/// The exchanges a view compilation showed, in any slot (its front, window and beside block), as its view
/// record keeps them: none for another strategy's compilation, or a view whose record does not read.
fn shown_exchanges(
    store: &Store,
    c: &crate::compiler::Compilation,
) -> Option<Vec<crate::view::SourceRange>> {
    if c.strategy != crate::compiler::view::STRATEGY {
        return None;
    }
    let v = crate::view::record::read(store, &c.id).ok().flatten()?;
    Some(
        (v.shown.into_iter())
            .map(|s| s.piece.range)
            .filter(|r| matches!(r.space, crate::view::Space::Exchanges { .. }))
            .collect(),
    )
}

/// Whether `o` is in one of `shown`'s ranges: its place's, by the position its touch sorts at.
fn shown_holds(shown: &[crate::view::SourceRange], o: &Exchange) -> bool {
    let at = crate::view::Pos {
        at_ms: o.start_ms,
        id: o.turn_id.clone(),
    };
    shown.iter().any(|r| {
        matches!(&r.space, crate::view::Space::Exchanges { place } if *place == o.place)
            && r.first <= at
            && at <= r.last
    })
}

/// The place's exchanges around `ex`: the one before it, and whether another
/// session's was still running when it began.
fn around(store: &Store, ex: &Exchange) -> (Option<Exchange>, bool) {
    let touches = newest(store, Root::Place(&ex.place), 8).unwrap_or_default();
    let others: Vec<Exchange> = touches
        .iter()
        .filter(|t| t.exchange != ex.turn_id)
        .filter_map(|t| exchange(store, &t.exchange).ok().flatten())
        .collect();
    let overlap = others.iter().any(|o| {
        o.session_id != ex.session_id
            && o.start_ms < ex.start_ms
            && ex.start_ms - o.start_ms < OPEN_FOR_MS
            && o.end_ms.is_none_or(|end| end > ex.start_ms)
    });
    let prev = others.into_iter().find(|o| o.start_ms <= ex.start_ms);
    (prev, overlap)
}

/// The judge's recorded answers to the message `node`, from the classify
/// pack's rows of `sessions`.
fn judged(store: &Store, sessions: &[&str], node: &str) -> BTreeMap<String, String> {
    let page = Page {
        kind: kinds::LEDGER,
        tags: sessions
            .iter()
            .map(|s| ledger_kind_session(LedgerKind::JudgeCall.as_str(), s))
            .collect(),
        limit: 24,
        ..Page::default()
    };
    let Ok(Some(got)) = store.inner().page(&page) else {
        return BTreeMap::new();
    };
    for r in got.records.iter().rev() {
        let Ok(row) = r.decode::<LedgerRow>() else {
            continue;
        };
        let Ok(j) = serde_json::from_value::<theseus_judge::Judgment>(row.data) else {
            continue;
        };
        if j.pack != crate::judge::inbound::CLASSIFY_PACK
            || j.context["node"].as_str() != Some(node)
        {
            continue;
        }
        return j
            .answers
            .iter()
            .filter(|a| JUDGED.contains(&a.question.as_str()))
            .filter_map(|a| Some((a.question.clone(), a.ran()?.to_string())))
            .collect();
    }
    BTreeMap::new()
}

/// What the row needs beside the exchange.
pub struct Given<'a> {
    pub ex: &'a Exchange,
    pub seen: Option<&'a Seen>,
    pub peer: Option<Peer>,
    pub arrived_session: Option<&'a str>,
    pub ask_chars: Option<u64>,
    pub usage: &'a theseus_protocol::Usage,
    pub things: Option<BTreeMap<String, u64>>,
    pub reply_things: Option<BTreeMap<String, u64>>,
}

/// The row for a turn that ends now, read from the store's records.
pub fn row(store: &Store, g: Given<'_>) -> Row {
    let ex = g.ex;
    let compiled = g
        .seen
        .and_then(|s| store.get_compilation(&s.compilation).ok().flatten());
    let view = compiled.as_ref().map(|c| View {
        includes: &c.includes,
        as_of: c.as_of,
    });
    let seen_in = |id: &str| {
        view.as_ref()
            .is_some_and(|v| in_view(store, &ex.session_id, v, id))
    };
    // A view's "seen" is every exchange it showed: front, window and beside, by its record.
    let shown = compiled.as_ref().and_then(|c| shown_exchanges(store, c));
    let seen = |o: &Exchange, ids: &[Option<&str>]| match &shown {
        Some(s) => shown_holds(s, o),
        None => ids.iter().flatten().any(|id| seen_in(id)),
    };
    let (prev, overlap) = around(store, ex);
    let mut row = Row {
        exchange: ex.turn_id.clone(),
        place: ex.place.clone(),
        opened_by: Some(ex.opened_by),
        peer: g.peer,
        person: ex.person.is_some(),
        ask_chars: g.ask_chars,
        overlap,
        cache_read: g.usage.cache_read_input_tokens,
        cache_write: g.usage.cache_creation_input_tokens,
        things: g.things,
        reply_things: g.reply_things,
        ..Row::default()
    };
    if let Some(s) = g.seen {
        row.sent = s.sent.clone();
        (row.situation, row.trigger, row.cold) = (s.situation.clone(), s.trigger.clone(), s.cold);
    }
    if let Some(p) = &prev {
        let held = seen(p, &[p.ask.as_deref(), p.reply.as_deref()]);
        let other = p.session_id != ex.session_id;
        row.prev = match held {
            true => Prev::Transcript,
            false => Prev::Missed,
        };
        row.prev_why = match (held, g.seen) {
            (true, _) => None,
            (false, None) => Some(WHY_NOT_MEASURED.into()),
            (false, Some(_)) => after_new(store, ex, p).then(|| WHY_NEW.into()),
        };
        row.prev_exchange = Some(p.turn_id.clone());
        row.prev_same_session = Some(!other);
        let gap = ex.start_ms.saturating_sub(p.end_ms.unwrap_or(p.start_ms));
        row.gap_ms = Some(gap);
        let unprompted = p.opened_by != Opener::Message || p.person.is_none();
        if ex.person.is_some() && unprompted && gap <= ANSWERED_WITHIN_MS {
            row.answered = Some(p.opened_by);
        }
    }
    row.elsewhere = elsewhere(store, ex, &|o: &Exchange| seen(o, &[o.ask.as_deref()]));
    if let Some(ask) = &ex.ask {
        let mut sessions = vec![ex.session_id.as_str()];
        sessions.extend(g.arrived_session.filter(|s| *s != ex.session_id));
        row.judge = judged(store, &sessions, ask);
    }
    row
}

/// The person's newest exchange in each other private place, at most eight.
fn elsewhere(store: &Store, ex: &Exchange, seen_in: &dyn Fn(&Exchange) -> bool) -> Vec<Elsewhere> {
    let person = ex.person.as_deref().unwrap_or(OWNER);
    let mut places = HashSet::new();
    let mut out = Vec::new();
    for t in newest(store, Root::Person(person), 32).unwrap_or_default() {
        if t.exchange == ex.turn_id || out.len() >= 8 {
            continue;
        }
        let Ok(Some(o)) = exchange(store, &t.exchange) else {
            continue;
        };
        if o.place == ex.place || !o.private || !places.insert(o.place.clone()) {
            continue;
        }
        out.push(Elsewhere {
            in_view: seen_in(&o),
            age_ms: ex.start_ms.saturating_sub(o.start_ms),
            place: o.place,
            exchange: o.turn_id,
        });
    }
    out
}

fn count(r: &mut ExchangeReportResult, group: &str, value: &str) {
    *r.counts
        .entry(group.into())
        .or_default()
        .entry(value.into())
        .or_insert(0) += 1;
}

/// A gap's band, for the report.
fn band(gap: u64) -> &'static str {
    match gap {
        0..60_000 => "under_1m",
        60_000..600_000 => "under_10m",
        600_000..10_800_000 => "under_3h",
        _ => "over_3h",
    }
}

/// A message's length's band, for the report.
fn chars_band(n: u64) -> &'static str {
    match n {
        0..80 => "under_80",
        80..400 => "under_400",
        _ => "over_400",
    }
}

/// The judge's answer to `question`, as the report counts it: a task's id
/// only as whether there was one.
fn judged_value(question: &str, answer: &str) -> String {
    match (question, answer) {
        ("addressed_task", "none") => "none".into(),
        ("addressed_task", _) => "a_task".into(),
        _ => answer.into(),
    }
}

/// One row, added to the report by its own shape: each field a count of its
/// value under its name (a gap and a message's length by their bands, a map
/// by its keys, the judge's answers by question), `sent`'s parts summed, and
/// each other private place's exchange by whether it was in view. Ids are
/// skipped; the cache's tokens are the answers'. A field not named here or in
/// [`BY_VALUE`] is counted as present, never by its value.
pub(super) fn add(r: &mut ExchangeReportResult, row: &Value) {
    let Value::Object(fields) = row else {
        return;
    };
    let mut sum =
        |k: &str, n: &Value| *r.tokens.entry(k.into()).or_insert(0) += n.as_u64().unwrap_or(0);
    let mut counts = Vec::new();
    for (k, v) in fields {
        match (k.as_str(), v) {
            ("exchange" | "place" | "prev_exchange" | "cache_read" | "cache_write", _) => {}
            ("sent", Value::Object(parts)) => parts.iter().for_each(|(p, n)| sum(p, n)),
            ("gap_ms", Value::Number(n)) => {
                counts.push((k.clone(), band(n.as_u64().unwrap_or(0)).into()))
            }
            ("ask_chars", Value::Number(n)) => {
                counts.push((k.clone(), chars_band(n.as_u64().unwrap_or(0)).into()))
            }
            ("things" | "reply_things", Value::Null) => counts.push((k.clone(), "unknown".into())),
            ("elsewhere", Value::Array(xs)) => counts.extend(xs.iter().map(|e| {
                let seen = if e["in_view"] == true {
                    "in_view"
                } else {
                    "not_in_view"
                };
                (k.clone(), seen.into())
            })),
            ("things" | "reply_things" | "judge", Value::Object(m)) if m.is_empty() => {
                counts.push((k.clone(), "none".into()))
            }
            ("things" | "reply_things", Value::Object(m)) => {
                counts.extend(m.keys().map(|kind| (k.clone(), kind.clone())))
            }
            ("judge", Value::Object(m)) => counts.extend(m.iter().map(|(q, a)| match a {
                Value::String(a) if JUDGED.contains(&q.as_str()) => {
                    (format!("{k}.{q}"), judged_value(q, a))
                }
                _ => (k.clone(), "present".into()),
            })),
            (_, Value::Null) => counts.push((k.clone(), "none".into())),
            (_, Value::String(x)) if BY_VALUE.contains(&k.as_str()) => {
                counts.push((k.clone(), x.clone()))
            }
            (_, Value::Bool(b)) if BY_VALUE.contains(&k.as_str()) => {
                counts.push((k.clone(), b.to_string()))
            }
            _ => counts.push((k.clone(), "present".into())),
        }
    }
    for (group, value) in counts {
        count(r, &group, &value);
    }
}

/// The place's position of exchange `e`, as the view orders it.
fn pos(e: &Exchange) -> crate::view::Pos {
    crate::view::Pos {
        at_ms: e.start_ms,
        id: e.turn_id.clone(),
    }
}

/// Whether the place's latest `/new` lies between `prev` and `ex`: the view
/// leaves out what came before it, on purpose. Only a place's latest `/new`
/// is kept, so an older one is not seen here.
fn after_new(store: &Store, ex: &Exchange, prev: &Exchange) -> bool {
    match crate::ladder::new_mark(store, &ex.place) {
        Ok(Some(mark)) => pos(prev) <= mark && mark < pos(ex),
        _ => false,
    }
}

fn bump(m: &mut BTreeMap<String, u64>, k: &str) {
    *m.entry(k.into()).or_insert(0) += 1;
}

/// One row, added to the three answers: the context it sent when its first
/// call was measured; then, for a turn his own message opened, whether the
/// place's previous exchange was in it, and whether he named a thing.
fn answer(store: &Store, a: &mut ExchangeAnswers, row: &Row) {
    let measured = row.sent.total > 0;
    if measured {
        a.context_turns += 1;
        if let Ok(Value::Object(parts)) = serde_json::to_value(&row.sent) {
            for (k, n) in parts {
                *a.context.entry(k).or_insert(0) += n.as_u64().unwrap_or(0);
            }
        }
        a.cache_read += row.cache_read;
        a.cache_write += row.cache_write;
    }
    let Ok(Some(ex)) = exchange(store, &row.exchange) else {
        a.unreadable += 1;
        return;
    };
    if ex.opened_by != Opener::Message || ex.person.as_deref() != Some(OWNER) {
        return;
    }
    a.his_turns += 1;
    let previous = match row.prev {
        Prev::None => "first_here",
        _ if !measured => "not_measured",
        Prev::Transcript | Prev::PlaceTail => "shown",
        Prev::Missed => match row.prev_why.as_deref() {
            Some(WHY_NEW) => "after_new",
            Some(WHY_NOT_MEASURED) => "not_measured",
            // A row from before `prev_why`: the place's latest `/new`, best effort.
            None if (row.prev_exchange.as_deref())
                .and_then(|p| exchange(store, p).ok().flatten())
                .is_some_and(|p| after_new(store, &ex, &p)) =>
            {
                "after_new"
            }
            _ => "missed",
        },
    };
    bump(&mut a.previous, previous);
    if previous == "missed" {
        let session = match row.prev_same_session {
            Some(true) => "same",
            Some(false) => "other",
            None => "unknown",
        };
        bump(&mut a.missed_by_session, session);
    }
    let named = match &row.things {
        None => "unknown",
        Some(m) if m.values().any(|n| *n > 0) => "some",
        Some(_) => "none",
    };
    bump(&mut a.named, named);
    for (kind, _) in row.things.iter().flatten().filter(|(_, n)| **n > 0) {
        bump(&mut a.named_kinds, kind);
    }
}

/// `exchange.report`: the newest rows, none older than `since_ms`, summed,
/// and their answers.
pub fn report(store: &Store, p: &ExchangeReportParams) -> anyhow::Result<ExchangeReportResult> {
    let mut r = ExchangeReportResult::default();
    let mut a = ExchangeAnswers::default();
    let mut before = None;
    let limit = p.limit.unwrap_or(u64::MAX);
    'pages: loop {
        let page = Page {
            kind: kinds::LEDGER,
            tags: vec![ledger_kind(LedgerKind::ContextTurn.as_str())],
            before,
            limit: 512,
            ..Page::default()
        };
        let Some(got) = store.inner().page(&page)? else {
            anyhow::bail!("the index is still being built: ask again in a minute");
        };
        for rec in got.records.iter().rev() {
            if r.rows >= limit {
                break 'pages;
            }
            let Ok(lr) = rec.decode::<LedgerRow>() else {
                continue;
            };
            if p.since_ms.is_some_and(|since| lr.at_unix_ms < since) {
                break 'pages;
            }
            let Ok(row) = serde_json::from_value::<Row>(lr.data) else {
                continue;
            };
            r.rows += 1;
            r.to_ms = r.to_ms.or(Some(lr.at_unix_ms));
            r.from_ms = Some(lr.at_unix_ms);
            add(&mut r, &serde_json::to_value(&row).unwrap_or(Value::Null));
            answer(store, &mut a, &row);
        }
        match (got.more, got.first) {
            (true, Some(first)) => before = Some(first),
            _ => break,
        }
    }
    r.answers = Some(a);
    Ok(r)
}

/// The places bound now, by kind, as health lists them: the local surfaces,
/// DMs, channels bound private (and of those, any whom people besides the
/// owner can view), and shared channels. Counts, never a channel's name.
pub fn bound_now(health: &theseus_protocol::PlacesHealth) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for p in &health.places {
        let kind = match (p.place.as_str(), p.class) {
            (t, _) if t.starts_with("discord:dm:") => "dm",
            (t, crate::places::PlaceClass::Private) if t.starts_with("discord:") => {
                match p.others.as_ref().is_some_and(|o| !o.is_empty()) {
                    true => "channel_private_seen_by_others",
                    false => "channel_private",
                }
            }
            (t, _) if t.starts_with("discord:") => "channel_shared",
            _ => "local",
        };
        *out.entry(kind.to_string()).or_insert(0) += 1;
    }
    out
}

//! `memory.lookup` (theseus-w9qv, fix C): a read of memory the model calls
//! when it wants one, beside what the compiler recalls for the turn.
//!
//! - **What it asks**: `words` go through recall's own pipeline (the index,
//!   the filters with the place rule first, the pack), kept to a span of
//!   time and to a book's or a topic's sessions when those are given; with
//!   no words, `when`, `book` and `topic` list the imported episodes as
//!   `import.sessions` does (its catalog, a query of about 12 ms), each with
//!   its summary. Small pages (8 items by default, at most 25), a `cursor`
//!   for the next, and a token budget (`budget_tokens`, 1,500 by default)
//!   that ends a page early and says where the next begins.
//! - **What each item carries**: its as-of date (an imported episode's span,
//!   a native node's creation, in the daemon's time zone), its session id,
//!   its source, its sensitivity, and its text, veiled for `personal` and
//!   `partner-confidential` exactly as the cockpit's Context page veils it
//!   (`VEILED`): the labels show, the text does not. The owner reads a
//!   veiled text in the cockpit; nothing here opens one.
//! - **Private places only.** Never offered in a shared place (it is no
//!   public tool: `places::offered`), refused there by the gate if called
//!   (`places::refusal`), and checked again here against the asking
//!   session's class and its place (`TurnRunner::place_of`): a shared place
//!   gets nothing of it.
//! - **Costs nothing until called**: no state, no warm read; the core is
//!   reached through a weak handle given at build (`Board::attach`).

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_memory::recall::Place;
use theseus_protocol::import::{ImportSessionsParams, ImportedEpisode};
use theseus_protocol::Plan;
use theseus_tools::{
    AsyncMediaRun, Backend, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

use crate::config::memory::MemoryArm;
use crate::recall::when::When;
use crate::Core;

/// The tool's name, and its family.
pub const LOOKUP: &str = "memory.lookup";
pub const FAMILY: &str = "memory";
/// Its name, as the config's `[policy.tools]` checks it.
pub const NAMES: [&str; 1] = [LOOKUP];

/// The sensitivities whose text is veiled, as the cockpit's Context page
/// veils them (`cockpit/src/lib/explorer.ts`'s `VEILED`).
pub const VEILED: [&str; 2] = ["personal", "partner-confidential"];
/// An imported item whose episode's labels could not be read: its text is
/// veiled too, since its sensitivity is not known (fail closed).
pub const UNREAD: &str = "labels unread";

fn veils(sensitivity: Option<&str>) -> bool {
    sensitivity.is_some_and(|s| VEILED.contains(&s) || s == UNREAD)
}

/// A page's items by default, and at most.
const PAGE: u64 = 8;
const PAGE_MAX: u64 = 25;
/// The answer's token budget by default, and at most.
const BUDGET: u64 = 1_500;
const BUDGET_MAX: u64 = 6_000;
/// The longest an index's answer is waited for.
const DEADLINE: Duration = Duration::from_secs(2);
/// The most sessions a book or a topic narrows a search of words to.
const SESSIONS_MAX: u64 = 500;

/// Why a shared place gets nothing.
pub const SHARED: &str = "memory.lookup reads the owner's memory, and this place is shared: \
     people besides the owner read it, so nothing of memory is looked up here";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    #[serde(default)]
    words: Option<String>,
    #[serde(default)]
    when: Option<String>,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    book: Option<String>,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    limit: Option<u64>,
    #[serde(default)]
    cursor: Option<u64>,
    #[serde(default)]
    budget_tokens: Option<u64>,
}

fn some(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

impl Args {
    fn parse(input: &Value) -> Result<Self, String> {
        let a: Self =
            serde_json::from_value(input.clone()).map_err(|e| format!("bad input: {e}"))?;
        if [&a.words, &a.when, &a.from, &a.to, &a.book, &a.topic]
            .iter()
            .all(|s| some(s).is_none())
        {
            return Err("name something to look up: words, when (\"March 2026\", \"last week\"), from and to (2026-03-01), a book, or a topic".into());
        }
        Ok(a)
    }

    /// The span `when`, `from` and `to` name, read as a turn's words are, as
    /// of `now_ms`: `from`'s start to `to`'s end.
    fn span(&self, now_ms: u64) -> Result<Option<When>, String> {
        let read = |field: &str, s: &str| {
            crate::recall::when::read_local(s, now_ms)
                .ok_or_else(|| format!("{field} {s:?} names no time: write 2026-03, 2026-03-14, March 2026, or last week"))
        };
        let mut w = match some(&self.when) {
            Some(s) => Some(read("when", s)?),
            None => None,
        };
        let from = some(&self.from).map(|s| read("from", s)).transpose()?;
        let to = some(&self.to).map(|s| read("to", s)).transpose()?;
        if from.is_some() || to.is_some() {
            let said = [from.as_ref(), to.as_ref()]
                .iter()
                .flatten()
                .map(|w| w.said.as_str())
                .collect::<Vec<_>>()
                .join(" to ");
            let mut span = w.take().unwrap_or(When {
                from_ms: None,
                to_ms: None,
                said: String::new(),
            });
            if let Some(f) = from {
                span.from_ms = f.from_ms;
            }
            if let Some(t) = to {
                span.to_ms = t.to_ms;
            }
            span.said = match span.said.is_empty() {
                true => said,
                false => format!("{}, {said}", span.said),
            };
            w = Some(span);
        }
        if let Some(s) = &w {
            if let (Some(f), Some(t)) = (s.from_ms, s.to_ms) {
                if t <= f {
                    return Err(format!("{} ends before it begins", s.said));
                }
            }
        }
        Ok(w)
    }
}

/// The registry's `memory.lookup`: its name, schema and plan, for the gate.
/// The runtime runs it ([`Board::run`]).
pub struct MemoryLookup;

impl Tool for MemoryLookup {
    fn name(&self) -> &'static str {
        LOOKUP
    }
    fn description(&self) -> &'static str {
        "Look up the owner's memory (past conversations and imported history) by words, time, book or topic. Items give date, session, source, sensitivity and text; personal and partner-confidential text is veiled. Pass the cursor it gives for more."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "words": {"type": "string"},
                "when": {"type": "string", "description": "\"March 2026\", \"2026-03-14\", \"last week\""},
                "from": {"type": "string", "description": "first day or month"},
                "to": {"type": "string", "description": "last day or month"},
                "book": {"type": "string"},
                "topic": {"type": "string"},
                "limit": {"type": "integer", "maximum": PAGE_MAX},
                "cursor": {"type": "integer"},
                "budget_tokens": {"type": "integer", "maximum": BUDGET_MAX}
            },
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a = Args::parse(input)?;
        a.span(theseus_protocol::now_unix_ms())?;
        let parts: Vec<String> = [
            some(&a.words).map(|w| format!("{w:?}")),
            some(&a.when).map(str::to_string),
            some(&a.from).map(|f| format!("from {f}")),
            some(&a.to).map(|t| format!("to {t}")),
            some(&a.book).map(|b| format!("the {b} book")),
            some(&a.topic).map(|t| format!("topic {t}")),
        ]
        .into_iter()
        .flatten()
        .collect();
        Ok(Plan {
            summary: format!("look up memory: {}", parts.join(", ")),
            ..Default::default()
        })
    }
    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "memory.lookup runs through the runtime, which reads the memory",
        ))))
    }
}

/// What `memory.lookup` reads: the core, given once it is built.
#[derive(Default)]
pub struct Board {
    core: OnceLock<Weak<Core>>,
}

impl Board {
    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    /// The call, in its own task: refused in a shared place, by the turn's
    /// class and again by the session's place.
    pub fn run(&self, tc: &crate::toolrun::TurnCtx<'_>, input: &Value) -> AsyncMediaRun {
        let fail = |why: &str| -> AsyncMediaRun {
            Box::pin(std::future::ready(Err(ToolFailure::new(why))))
        };
        if tc.class != crate::places::PlaceClass::Private {
            return fail(SHARED);
        }
        let Some(core) = self.core.get().and_then(Weak::upgrade) else {
            return fail("memory.lookup is not ready: the core is not built");
        };
        let a = match Args::parse(input) {
            Ok(a) => a,
            Err(e) => return fail(&e),
        };
        let session = tc.session_id.to_string();
        Box::pin(async move {
            lookup(core, session, a)
                .await
                .map(|o| (o, None, None))
                .map_err(ToolFailure::new)
        })
    }
}

/// A call as the runtime makes it, for a session: tests reach the tool's
/// own check this way, past the gate.
#[cfg(test)]
pub(crate) async fn call(
    core: Arc<Core>,
    session: &str,
    input: Value,
) -> Result<ToolOutput, String> {
    lookup(core, session.to_string(), Args::parse(&input)?).await
}

/// The page lines of recall's admitted items, as `lookup` renders them:
/// tests reach the catalog's miss this way.
#[cfg(test)]
pub(crate) async fn rendered(
    core: &Arc<Core>,
    admitted: &[theseus_protocol::memory::RecallItem],
) -> Result<String, String> {
    let items = items_of(core, admitted).await?;
    Ok(items
        .iter()
        .enumerate()
        .map(|(i, it)| render(i as u64 + 1, it))
        .collect())
}

/// One item of a page, as the answer says it.
struct Item {
    when: String,
    session_id: String,
    source: String,
    sensitivity: Option<String>,
    book: Option<String>,
    text: String,
}

async fn lookup(core: Arc<Core>, session: String, a: Args) -> Result<ToolOutput, String> {
    if !matches!(core.runner.place_of(&session), Place::Private) {
        return Err(SHARED.into());
    }
    let now = theseus_protocol::now_unix_ms();
    let when = a.span(now)?;
    let limit = a.limit.unwrap_or(PAGE).clamp(1, PAGE_MAX);
    let offset = a.cursor.unwrap_or(0);
    let budget = a.budget_tokens.unwrap_or(BUDGET).clamp(100, BUDGET_MAX);
    // A search of words asks for one past the page, to know there is more.
    let (head, items, total) = match some(&a.words) {
        Some(words) => {
            let k = offset + limit + 1;
            by_words(&core, &session, words.to_string(), &a, when.as_ref(), k).await?
        }
        None => by_catalog(&core, &a, when.as_ref(), offset, limit).await?,
    };
    let counted = some(&a.words).is_none();
    let items: Vec<Item> = match some(&a.words) {
        Some(_) => items.into_iter().skip(offset as usize).collect(),
        None => items,
    };
    let mut text = head;
    let mut shown = 0u64;
    let mut used = text.len() as u64 / 4;
    for (i, it) in items.iter().take(limit as usize).enumerate() {
        let line = render(offset + i as u64 + 1, it);
        let cost = line.len() as u64 / 4;
        if shown > 0 && used + cost > budget {
            break;
        }
        text.push_str(&line);
        used += cost;
        shown += 1;
    }
    let next = offset + shown;
    let more = next < total;
    let of = match counted {
        true => format!(" of {total}"),
        false => String::new(),
    };
    text.push_str(&match (shown, more) {
        (0, _) => "\nNothing on this page.".to_string(),
        (_, true) => format!(
            "\nShown {}–{next}{of}. More: call again with cursor {next}.",
            offset + 1
        ),
        (_, false) => format!("\nShown {}–{next}{of}; that is all.", offset + 1),
    });
    let ids: Vec<&str> = items
        .iter()
        .take(shown as usize)
        .map(|i| i.session_id.as_str())
        .collect();
    let veiled = items
        .iter()
        .take(shown as usize)
        .filter(|i| veils(i.sensitivity.as_deref()))
        .count();
    Ok(ToolOutput {
        text,
        meta: json!({
            "mode": if some(&a.words).is_some() { "words" } else { "catalog" },
            "when": when.as_ref().map(theseus_protocol::memory::RecallWhen::from),
            "total": total, "shown": shown, "cursor": offset,
            "next": more.then_some(next), "veiled": veiled, "sessions": ids,
            "tokens": used,
        }),
    })
}

/// A date in the daemon's time zone, `2026-03-14`.
fn day(ms: u64) -> String {
    crate::judge::spend::local_day(ms)
}

fn span_words(start_ms: u64, end_ms: u64) -> String {
    let (a, b) = (day(start_ms), day(end_ms));
    if a == b {
        a
    } else {
        format!("{a} to {b}")
    }
}

fn render(n: u64, it: &Item) -> String {
    let mut head = format!("\n{n}. {} · {} · {}", it.when, it.session_id, it.source);
    if let Some(s) = &it.sensitivity {
        head.push_str(&format!(" · {s}"));
    }
    if let Some(b) = &it.book {
        head.push_str(&format!(" · book {b}"));
    }
    let veiled = veils(it.sensitivity.as_deref());
    let body = match veiled {
        true => format!(
            "[{} · veiled: its text stays veiled here, as on the cockpit's Context page; the owner opens it there]",
            it.sensitivity.as_deref().unwrap_or_default().replace('-', " ")
        ),
        false => it.text.split_whitespace().collect::<Vec<_>>().join(" "),
    };
    format!("{head}\n   {body}")
}

fn what(a: &Args, when: Option<&When>) -> String {
    let mut w = Vec::new();
    if let Some(s) = when {
        let edge = |ms: Option<u64>, open: &str| ms.map_or(open.to_string(), day);
        // The end is exclusive: say its last day.
        let last = s.to_ms.map(|t| t.saturating_sub(1));
        w.push(format!(
            "in {} ({} to {}, the daemon's time zone, UTC{})",
            s.said,
            edge(s.from_ms, "the start"),
            edge(last, "today"),
            offset_words()
        ));
    }
    if let Some(b) = some(&a.book) {
        w.push(format!("in the {b} book"));
    }
    if let Some(t) = some(&a.topic) {
        w.push(format!("on topic {t}"));
    }
    w.join(", ")
}

fn offset_words() -> String {
    let off = crate::wake::local(theseus_protocol::now_unix_ms()).offset_secs;
    let sign = if off < 0 { '-' } else { '+' };
    let off = off.unsigned_abs();
    format!("{sign}{:02}:{:02}", off / 3600, off % 3600 / 60)
}

/// The catalog's episodes: `import.sessions`, newest first.
async fn by_catalog(
    core: &Arc<Core>,
    a: &Args,
    when: Option<&When>,
    offset: u64,
    limit: u64,
) -> Result<(String, Vec<Item>, u64), String> {
    let p = ImportSessionsParams {
        book: some(&a.book).map(str::to_string),
        topic: some(&a.topic).map(str::to_string),
        from_ms: when.and_then(|w| w.from_ms),
        // The catalog's end is inclusive.
        to_ms: when.and_then(|w| w.to_ms).map(|t| t.saturating_sub(1)),
        offset: Some(offset),
        limit: Some(limit),
        summaries: true,
        ..Default::default()
    };
    let r = catalog(core, p).await?;
    let head = format!(
        "memory.lookup: {} imported episode{} {}, newest first (episodes overlapping the span).",
        r.total,
        if r.total == 1 { "" } else { "s" },
        what(a, when)
    );
    let items = r
        .episodes
        .into_iter()
        .map(|e| episode_item(&e, None))
        .collect();
    Ok((head, items, r.total))
}

fn episode_item(e: &ImportedEpisode, text: Option<String>) -> Item {
    Item {
        when: span_words(e.start_ms, e.end_ms),
        session_id: e.session_id.clone(),
        source: format!("imported from {}", e.source),
        sensitivity: Some(e.sensitivity.clone()),
        book: e.book.clone(),
        text: text
            .or_else(|| e.summary_text.clone())
            .or_else(|| e.title.clone())
            .unwrap_or_default(),
    }
}

/// Recall's pipeline over `words`, as a turn of this session's would run
/// it (the place rule first), kept to the span and to a book's or a topic's
/// sessions: the first `k` it admits.
async fn by_words(
    core: &Arc<Core>,
    session: &str,
    words: String,
    a: &Args,
    when: Option<&When>,
    k: u64,
) -> Result<(String, Vec<Item>, u64), String> {
    // A book or a topic: its sessions, from the catalog.
    let (sessions, narrowed) = match narrowed_to(core, a).await? {
        Some(n) => n,
        None => {
            return Ok((
                format!("memory.lookup: no imported episode is {}.", what(a, None)),
                Vec::new(),
                0,
            ))
        }
    };
    let memory = &core.runner.memory;
    let in_context = core
        .store
        .session_nodes(session)
        .map_err(|e| format!("this session could not be read: {e:#}"))?
        .into_iter()
        .map(|(_, n)| n.id)
        .collect();
    let deadline = Duration::from_millis(memory.cfg().recall_deadline_ms).max(DEADLINE);
    let k = (k as usize).clamp(1, 100);
    let mut begun = memory.begin_within(
        words.clone(),
        None,
        k,
        MemoryArm::Baseline,
        deadline,
        when.cloned(),
        sessions,
    );
    begun.max_items = Some(k);
    let answer = begun.answer().await;
    let scene = crate::recall::Scene {
        mode: "lookup",
        session_id: Some(session),
        turn_id: None,
        place: Place::Private,
        in_context,
        labeled: memory.labeled(&core.store).map_err(|e| format!("{e:#}"))?,
        budget_tokens: Some(BUDGET_MAX * 4),
        science: memory.science_for(MemoryArm::Baseline),
        activation: None,
    };
    let m = memory.manifest(
        &scene,
        &begun,
        answer,
        |s| core.runner.place_of(s),
        |ids| crate::recall::links(&core.store, ids),
        true,
    );
    // `words_only`: the vector search was late or failed, and the word
    // sources answered (recall-fallback); their hits are an answer.
    if m.outcome != "ran" && m.outcome != crate::recall::WORDS_ONLY {
        return Err(format!(
            "the index did not answer ({}{}): try again, or look up by when, book or topic, which reads no index",
            m.outcome,
            m.why.as_deref().map(|w| format!(": {w}")).unwrap_or_default()
        ));
    }
    let items = items_of(core, &m.admitted).await?;
    let total = items.len() as u64;
    let head = format!(
        "memory.lookup: recall's items for {words:?}{}{narrowed}, best first.",
        match what(a, when) {
            w if w.is_empty() => String::new(),
            w => format!(" {w}"),
        }
    );
    Ok((head, items, total))
}

/// A book's or a topic's sessions, from the catalog, and what the answer
/// says of a cut; none when nothing is in it. No book or topic: no
/// narrowing.
async fn narrowed_to(core: &Arc<Core>, a: &Args) -> Result<Option<(Vec<String>, String)>, String> {
    if some(&a.book).is_none() && some(&a.topic).is_none() {
        return Ok(Some((Vec::new(), String::new())));
    }
    let p = ImportSessionsParams {
        book: some(&a.book).map(str::to_string),
        topic: some(&a.topic).map(str::to_string),
        limit: Some(SESSIONS_MAX),
        ..Default::default()
    };
    let r = catalog(core, p).await?;
    if r.episodes.is_empty() {
        return Ok(None);
    }
    let narrowed = match r.total > SESSIONS_MAX {
        true => format!(
            " (searched the newest {SESSIONS_MAX} of its {} episodes)",
            r.total
        ),
        false => String::new(),
    };
    Ok(Some((
        r.episodes.into_iter().map(|e| e.session_id).collect(),
        narrowed,
    )))
}

/// `import.sessions`, on the blocking pool, as a private place asks it.
async fn catalog(
    core: &Arc<Core>,
    p: ImportSessionsParams,
) -> Result<theseus_protocol::import::ImportSessionsResult, String> {
    let c = core.clone();
    tokio::task::spawn_blocking(move || c.import_sessions(&p, true))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("the imported episodes could not be read: {e:#}"))
}

/// Recall's admitted items as the answer says them: an imported one with
/// its episode's labels (one read of the catalog for all) and its message's
/// own date; a native one with its node's.
async fn items_of(
    core: &Arc<Core>,
    admitted: &[theseus_protocol::memory::RecallItem],
) -> Result<Vec<Item>, String> {
    // The imported sessions' labels, in one read.
    let ids: Vec<String> = admitted
        .iter()
        .map(|i| i.session_id.clone())
        .filter(|s| crate::import::is_imported(s))
        .collect();
    let mut episodes: BTreeMap<String, ImportedEpisode> = BTreeMap::new();
    if !ids.is_empty() {
        let p = ImportSessionsParams {
            ids,
            erased: true,
            limit: Some(500),
            ..Default::default()
        };
        let r = catalog(core, p).await?;
        episodes = r
            .episodes
            .into_iter()
            .map(|e| (e.session_id.clone(), e))
            .collect();
    }
    let mut items = Vec::new();
    for i in admitted {
        let text = i.text.clone().unwrap_or_default();
        let item = match episodes.get(&i.session_id) {
            Some(e) => {
                let mut it = episode_item(e, Some(text));
                // The message's own date, inside its episode's span.
                if let Ok(Some((_, n))) = core.store.get_node(&i.node_id) {
                    it.when = format!(
                        "{} (episode {})",
                        day(n.created_at_ms),
                        span_words(e.start_ms, e.end_ms)
                    );
                }
                it
            }
            // An imported session the catalog did not name: its labels
            // are unknown, so its text stays veiled.
            None if crate::import::is_imported(&i.session_id) => Item {
                when: String::new(),
                session_id: i.session_id.clone(),
                source: "imported".into(),
                sensitivity: Some(UNREAD.into()),
                book: None,
                text,
            },
            None => {
                let at = core
                    .store
                    .get_node(&i.node_id)
                    .ok()
                    .flatten()
                    .map_or(String::new(), |(_, n)| day(n.created_at_ms));
                Item {
                    when: at,
                    session_id: i.session_id.clone(),
                    source: format!("this harness ({})", i.kind),
                    sensitivity: None,
                    book: None,
                    text,
                }
            }
        };
        items.push(item);
    }
    Ok(items)
}

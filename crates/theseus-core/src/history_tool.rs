//! `context.history` (issue 44; design §6, rung 3): the history tool, offered on every turn, by which the model
//! extends the conversation it can see when it thinks it is missing something. The interface is the view's
//! (`view::history`); this module is its implementation over the exchange ledger (`crate::exchange`).
//!
//! - **Bare**, it returns the [`PAGE_EXCHANGES`] exchanges of this place that come before the oldest exchange the
//!   model sees in full (`page::in_view`). A page's `next` cursor, passed back as `before`, gives the ones
//!   before those, with no horizon. One line an exchange: its date, the place, whose words, the person's words
//!   (cut by code at 300 characters), the `did` line or the reply's beginning, and a handle.
//! - **Open**: a handle gives the exchange in spoken form, cut at about 3,000 tokens (`open`).
//! - **Narrowing**: `place` (any other place), `thing` (its timeline in the touch index), `words` (the words
//!   index, searched over the sessions on that timeline, then a filter on the walk), and a time span: `when`,
//!   or `from` / `to` ([`span_of`]: an explicit argument's reader, `recall::when::read_local`, never recall's
//!   reading of English in a person's message). A span's page holds exactly its exchanges, newest first,
//!   paged by `before`; an empty span says so; beside `words`, `place` or `thing` it narrows them.
//! - **Every place, for every reader** (anything goes, 2026-11-22): no presence, place class, seal or move
//!   keeps an exchange off a page or out of an open.
//! - **Where** ([`anchor`]): a message's turn is anchored where the message arrived; any other turn where its
//!   reply is delivered (`Outbox::speaks_at`, a wake's or a report's target too); a route that cannot be read
//!   is no place, and its bare page is the unknown place's.
//! - **Bounded**: [`LOOKS_PER_TURN`] pages and [`OPENS_PER_TURN`] opens a turn, each told plainly when used;
//!   "history is exhausted" is told apart.
//! - **Offered everywhere**: a public tool (`places::public_tool`), so a shared place has it too. Its family,
//!   `history`, loads by default in a DM and a channel; a terminal's, a task's and a headless run's first
//!   request has no room for it under its budget (`first_request`), so there it is declared and deferred, one
//!   search away, and the self card names it. Costs nothing until called: the core is reached through a weak
//!   handle given at build (`Board::attach`).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_memory::recall::Place;
use theseus_protocol::Plan;
use theseus_tools::{
    AsyncMediaRun, Backend, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

use crate::config::memory::MemoryArm;
use crate::exchange::{self, Exchange, Opener};
use crate::recall::when::When;
use crate::view::history::{HistoryArgs, HISTORY_TOOL, LOOKS_PER_TURN, PAGE_EXCHANGES};
use crate::view::timeline::Cursor;
use crate::view::{Anchor, PointOfView, Present};
use crate::Core;

pub(crate) mod open;
pub(crate) mod page;
#[cfg(test)]
mod tests;

/// Its name, as the config's `[policy.tools]` checks it.
pub const NAMES: [&str; 1] = [HISTORY_TOOL];
/// Exchanges a turn may open.
pub const OPENS_PER_TURN: u32 = 12;

/// The turns a words filter may name, at most.
const WORDS_K: usize = 100;
const DEADLINE: Duration = Duration::from_secs(2);
/// Turns whose counts are kept before finished ones are let go.
const TURNS_KEPT: usize = 256;
/// What a resume token's turn id begins with.
const RESUME: &str = "resume:";

fn parse(input: &Value) -> Result<HistoryArgs, String> {
    serde_json::from_value(input.clone()).map_err(|e| format!("bad input: {e}"))
}

/// The time span a call names, read as of `now_ms` by the reader an explicit argument takes
/// (`recall::when::read_local`): `when`, or `from` and `to` (each read as a span, `from` giving its start and
/// `to` its end). None when none is given; a span that does not read is told in words, never guessed.
pub(crate) fn span_of(a: &HistoryArgs, now_ms: u64) -> Result<Option<When>, String> {
    let some = |s: &Option<String>| {
        s.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let read = |s: &str| {
        crate::recall::when::read_local(s, now_ms).ok_or_else(|| {
            format!(
                "\"{}\" does not read as a time: name a day, a date or a span (\"yesterday\", \"2026-11-20\", \
                 \"last week\", \"from 3 to 5 November\")",
                page::flat(s)
            )
        })
    };
    match (some(&a.when), some(&a.from), some(&a.to)) {
        (None, None, None) => Ok(None),
        (Some(w), None, None) => Ok(Some(read(&w)?)),
        (Some(_), _, _) => Err("name a span with when, or with from and to, not both".into()),
        (None, from, to) => {
            let start = from.as_deref().map(read).transpose()?;
            let end = to.as_deref().map(read).transpose()?;
            let said = match (&from, &to) {
                (Some(f), Some(t)) => format!("from {f} to {t}"),
                (Some(f), None) => format!("since {f}"),
                (None, Some(t)) => format!("before the end of {t}"),
                (None, None) => unreachable!("one of them is given"),
            };
            Ok(Some(When {
                said,
                from_ms: start.and_then(|w| w.from_ms),
                to_ms: end.and_then(|w| w.to_ms),
            }))
        }
    }
}

/// The registry's `context.history`: its name, schema and plan, for the gate. The runtime runs it
/// ([`Board::run`]).
pub struct HistoryTool;

impl Tool for HistoryTool {
    fn name(&self) -> &'static str {
        HISTORY_TOOL
    }
    fn description(&self) -> &'static str {
        "This place's earlier history, 100 exchanges a page before those you see; before pages back; open reads one; when (or from/to) a time span."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "before": {"type": "object"},
                "open": {"type": "string"},
                "words": {"type": "string"},
                "thing": {"type": "string"},
                "place": {"type": "string"},
                "when": {"type": "string"},
                "from": {"type": "string"},
                "to": {"type": "string"}
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
        let a = parse(input)?;
        let summary = match (&a.open, &a.before) {
            (Some(h), _) => format!("open exchange {}", page::flat(&h.0)),
            (None, Some(_)) => "read further back through history".into(),
            (None, None) => "read back through history".into(),
        };
        Ok(Plan {
            summary,
            ..Default::default()
        })
    }
    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "context.history runs through the runtime, which reads the ledger",
        ))))
    }
}

/// One turn's use of the tool: its looks and opens, and the resume positions of walks stopped by their bound.
#[derive(Debug, Default, Clone)]
pub struct Use {
    pub looks: u32,
    pub opens: u32,
    resumes: Vec<Cursor>,
}

/// What `context.history` reads: the core, once built, and each turn's use.
#[derive(Default)]
pub struct Board {
    core: OnceLock<Weak<Core>>,
    turns: Mutex<HashMap<String, Use>>,
}

/// What a call does to its turn's counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Count {
    Look,
    Open,
}

impl Board {
    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    /// `turn`'s use before this call, and this call counted. Past [`TURNS_KEPT`] turns, only turns `done`
    /// says have finished are let go: a running turn's counts are never dropped.
    pub fn count(&self, turn: &str, what: Count, done: &dyn Fn(&str) -> bool) -> Use {
        let mut m = self.turns.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() >= TURNS_KEPT && !m.contains_key(turn) {
            m.retain(|t, _| !done(t));
        }
        let u = m.entry(turn.into()).or_default();
        let was = u.clone();
        match what {
            Count::Look => u.looks = u.looks.saturating_add(1),
            Count::Open => u.opens = u.opens.saturating_add(1),
        }
        was
    }

    /// An opaque token for `at`, kept for `turn`.
    fn keep(&self, turn: &str, at: Cursor) -> Cursor {
        let mut m = self.turns.lock().unwrap_or_else(|e| e.into_inner());
        let u = m.entry(turn.into()).or_default();
        u.resumes.push(at);
        Cursor {
            at_ms: 0,
            turn_id: format!("{RESUME}{}", u.resumes.len() - 1),
        }
    }

    /// The raw position a caller's `before` names: a token of this turn's, or the cursor itself.
    fn position(&self, turn: &str, before: Option<&Cursor>) -> Result<Option<Cursor>, String> {
        let Some(c) = before else { return Ok(None) };
        let Some(i) = c.turn_id.strip_prefix(RESUME).filter(|_| c.at_ms == 0) else {
            return Ok(Some(c.clone()));
        };
        let m = self.turns.lock().unwrap_or_else(|e| e.into_inner());
        i.parse::<usize>()
            .ok()
            .and_then(|i| m.get(turn)?.resumes.get(i).cloned())
            .map(Some)
            .ok_or_else(|| "that token belongs to another turn: call again without before".into())
    }

    /// The call, in its own task.
    pub fn run(self: &Arc<Self>, tc: &crate::toolrun::TurnCtx<'_>, input: &Value) -> AsyncMediaRun {
        let fail = |why: &str| -> AsyncMediaRun {
            Box::pin(std::future::ready(Err(ToolFailure::new(why))))
        };
        let Some(core) = self.core.get().and_then(Weak::upgrade) else {
            return fail("context.history is not ready: the core is not built");
        };
        let a = match parse(input) {
            Ok(a) => a,
            Err(e) => return fail(&e),
        };
        let pov = pov_of(&core, tc.session_id, tc.turn_id, tc.dir, tc.class);
        let (board, session, turn) = (
            self.clone(),
            tc.session_id.to_string(),
            tc.turn_id.to_string(),
        );
        Box::pin(async move {
            answer(&core, &board, (&session, &turn), &pov, &a)
                .await
                .map(|o| (o, None, None))
                .map_err(ToolFailure::new)
        })
    }
}

/// Where a turn is anchored: where its message arrived; else where its reply is delivered
/// (`Outbox::speaks_at`); none when that route cannot be read.
pub fn anchor(
    core: &Core,
    session: &str,
    ex: Option<&Exchange>,
    dir: Option<&str>,
) -> Option<String> {
    if let Some(e) = ex.filter(|e| e.opened_by == Opener::Message) {
        return (e.place != exchange::rebuild::UNKNOWN).then(|| e.place.clone());
    }
    core.runner
        .outbox
        .speaks_at(session)
        .ok()
        .map(|t| exchange::place_of(t.as_deref(), dir))
}

/// The turn's point of view: where it is anchored, and who wrote its message. A task's or a self session's
/// anchor is the one its view is assembled under (`view_gate::Verified`). Every reader reads every place, so
/// its presence is the widest (`Open`); `_class` is read no more, and goes with the sweep.
pub(crate) fn pov_of(
    core: &Core,
    session: &str,
    turn: &str,
    dir: Option<&str>,
    _class: crate::places::PlaceClass,
) -> PointOfView {
    let ex = exchange::exchange(&core.store, turn).ok().flatten();
    if crate::view_gate::worker(&core.runner, session) {
        return worker_pov(core, session, ex.as_ref());
    }
    let here = anchor(core, session, ex.as_ref(), dir);
    let person = ex
        .as_ref()
        .filter(|e| e.opened_by == Opener::Message)
        .and_then(|e| e.person.clone());
    PointOfView {
        anchor: Anchor::Place(here.unwrap_or_else(|| exchange::rebuild::UNKNOWN.into())),
        person,
        present: Present::Open,
        at_hand: None,
        now_ms: theseus_protocol::now_unix_ms(),
    }
}

/// A task's or a self session's point of view: its view's anchor (`view_gate::Verified`).
fn worker_pov(core: &Core, session: &str, ex: Option<&Exchange>) -> PointOfView {
    let v = crate::view_gate::Verified::of_session(&core.runner, session);
    let v = v.pov();
    PointOfView {
        anchor: Anchor::Place(v.anchor.clone()),
        person: (ex.filter(|e| e.opened_by == Opener::Message)).and_then(|e| e.person.clone()),
        present: Present::Open,
        at_hand: None,
        now_ms: theseus_protocol::now_unix_ms(),
    }
}

/// Whether `turn`'s exchange has ended.
fn ended(core: &Core, turn: &str) -> bool {
    exchange::exchange(&core.store, turn)
        .ok()
        .flatten()
        .is_some_and(|e| e.end_ms.is_some())
}

/// A call's answer, for `session`'s `turn` seen from `pov`.
pub(crate) async fn answer(
    core: &Arc<Core>,
    board: &Board,
    (session, turn): (&str, &str),
    pov: &PointOfView,
    a: &HistoryArgs,
) -> Result<ToolOutput, String> {
    let Anchor::Place(here) = &pov.anchor else {
        return Err("context.history reads a place's history, and this turn has none".into());
    };
    let done = |t: &str| ended(core, t);
    let used = board.count(
        turn,
        if a.open.is_some() {
            Count::Open
        } else {
            Count::Look
        },
        &done,
    );
    let span = span_of(a, pov.now_ms)?;
    let mut lens = page::Lens {
        store: &core.store,
        here: here.clone(),
        in_view: page::in_view(&core.store, session, here).map_err(|e| format!("{e:#}"))?,
        words: None,
        span: span.clone(),
        // A fork's history holds nothing an ancestor said at or after its point (`fork::cut`): a cut that
        // cannot be read fails the call plainly.
        cut: crate::fork::cut::Cut::of(&core.store, session).map_err(|e| format!("{e:#}"))?,
    };
    if let Some(h) = &a.open {
        if used.opens >= OPENS_PER_TURN {
            return Ok(ToolOutput {
                text: format!(
                    "This turn's allowance of opens is used: {OPENS_PER_TURN} exchanges opened. Do not open \
                     more this turn: tell the person what you found and offer to read further."
                ),
                meta: json!({"open": null, "opens": used.opens, "of": OPENS_PER_TURN}),
            });
        }
        let text = open::open(&lens, h)?;
        return Ok(ToolOutput {
            text,
            meta: json!({"open": page::flat(&h.0), "opens": used.opens + 1, "of": OPENS_PER_TURN}),
        });
    }
    let before = board.position(turn, a.before.as_ref())?;
    if let Some(w) = a.words.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
        let (root, _) = lens.root(a).map_err(|e| format!("{e:#}"))?;
        let scope = lens.scope(&root).map_err(|e| format!("{e:#}"))?;
        lens.words = Some(found(core, w, scope).await?);
    }
    let walked = lens
        .walk(a, before, used.looks)
        .map_err(|e| format!("{e:#}"))?;
    let resume = walked.resume.map(|at| board.keep(turn, at));
    let p = walked.page;
    Ok(ToolOutput {
        text: page::render(&p, (here, span.as_ref()), used.looks, resume.as_ref()),
        meta: json!({
            "lines": p.lines.len(), "end": p.end, "next": p.next.as_ref().or(resume.as_ref()),
            "look": used.looks + 1, "of": LOOKS_PER_TURN, "page": PAGE_EXCHANGES,
        }),
    })
}

/// The turns the words index finds for `words` among `scope`, the sessions on the timeline walked: the search
/// is scoped to them before it ranks anything.
pub(crate) async fn found(
    core: &Arc<Core>,
    words: &str,
    scope: HashSet<String>,
) -> Result<HashSet<String>, String> {
    if scope.is_empty() {
        return Ok(HashSet::new());
    }
    let memory = &core.runner.memory;
    let deadline = Duration::from_millis(memory.cfg().recall_deadline_ms).max(DEADLINE);
    let mut sessions: Vec<String> = scope.iter().cloned().collect();
    sessions.sort();
    let mut begun = memory.begin_within(
        words.to_string(),
        None,
        WORDS_K,
        MemoryArm::Baseline,
        deadline,
        None,
        sessions,
    );
    begun.max_items = Some(WORDS_K);
    let answer = begun.answer().await;
    let scene = crate::recall::Scene {
        mode: "context.history",
        session_id: None,
        turn_id: None,
        place: Place::Private,
        in_context: Default::default(),
        labeled: memory.labeled(&core.store).map_err(|e| format!("{e:#}"))?,
        budget_tokens: None,
        max_items: None,
        science: memory.science_for(MemoryArm::Baseline),
        persona: None,
        category: None,
    };
    let m = memory.manifest(
        &scene,
        &begun,
        answer,
        |s| core.runner.place_of(s),
        |ids| crate::recall::links(&core.store, ids),
        true,
    );
    if m.outcome != "ran" && m.outcome != crate::recall::WORDS_ONLY {
        return Err(format!(
            "the words index did not answer ({}): call again without words to read the history in order",
            m.outcome
        ));
    }
    Ok(m.admitted
        .iter()
        .filter(|i| scope.contains(&i.session_id))
        .filter_map(|i| core.store.get_node(&i.node_id).ok().flatten())
        .filter_map(|(_, n)| n.turn_id)
        .collect())
}

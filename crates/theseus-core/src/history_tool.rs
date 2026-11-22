//! `context.history` (issue 44; design §6, rung 3): the history tool, offered on every turn, by which the model
//! extends the conversation it can see when it thinks it is missing something. The interface is the view's
//! (`view::history`); this module is its implementation over the exchange ledger (`crate::exchange`).
//!
//! - **Bare**, it returns the [`PAGE_EXCHANGES`] exchanges of this place that come before the oldest exchange the
//!   model sees in full (`page::in_view`). A page's `next` cursor, passed back as `before`, gives the ones
//!   before those, with no horizon. One line an exchange: its date, the place, whose words, the person's words
//!   (cut by code at 300 characters), the `did` line or the reply's beginning, and a handle.
//! - **Open**: a handle gives the exchange in spoken form, cut at about 3,000 tokens (`open`).
//! - **Narrowing**: `place` (another private place, only where the owner is alone), `thing` (its timeline in
//!   the touch index), `words` (the words index, searched only over the sessions this point of view may read
//!   on that timeline, then a filter on the walk).
//! - **Who is present** ([`presence`]): the gate's reading (`view_gate::present`): owner alone only on the
//!   terminal, the web page or the owner's direct conversation, for an input carrying his in-person mark; a
//!   channel, private or not, is never alone; a wake's or a report's turn is not known (nothing records the
//!   presence of the turn that set it up), so it reads nothing in a private place.
//! - **Where** ([`anchor`]): a message's turn is anchored where the message arrived; any other turn where its
//!   reply is delivered (`Outbox::speaks_at`, a wake's or a report's target too); a route that cannot be read
//!   is no place, and reads nothing.
//! - **The rules** (`page::Lens::admits`), on every line and every open: the place rule, and the seal, fail
//!   closed: an exchange is read only when its session is proven neither sealed nor seal-pending
//!   (`Core::ladder_hidden`), the asking session itself aside. Paging, counts and endings run over admitted
//!   exchanges alone (`page`). [`LOOKS_PER_TURN`] pages and [`OPENS_PER_TURN`] opens a turn, each told plainly
//!   when used; "history is exhausted" is told apart.
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

/// The registry's `context.history`: its name, schema and plan, for the gate. The runtime runs it
/// ([`Board::run`]).
pub struct HistoryTool;

impl Tool for HistoryTool {
    fn name(&self) -> &'static str {
        HISTORY_TOOL
    }
    fn description(&self) -> &'static str {
        "This place's earlier history, 100 exchanges a page before those you see; before pages back; open reads one."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "before": {"type": "object"},
                "open": {"type": "string"},
                "words": {"type": "string"},
                "thing": {"type": "string"},
                "place": {"type": "string"}
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

/// Who is present for a turn, by the gate's reading (`view_gate::present`): the session's place by today's
/// rule, the surface the message arrived at, and its in-person mark; a turn with no message (a wake, a report,
/// a continuation) is not known. Never more than the turn's own class allows. The one function to swap for
/// the gate's own when it computes the view's presence itself.
pub fn presence(
    core: &Core,
    session: &str,
    ex: Option<&Exchange>,
    class: crate::places::PlaceClass,
) -> Present {
    use theseus_memory::gate::Present as G;
    let message = ex.filter(|e| e.opened_by == Opener::Message);
    let target = message.map(|e| e.place.as_str());
    let input = match message.and_then(|e| e.ask.as_deref()) {
        Some(node) => crate::view_gate::Input::Message(node),
        None => crate::view_gate::Input::SetUp(None),
    };
    let place = crate::view_gate::RecallPlaces::default().of_session(&core.runner, session);
    let g = crate::view_gate::present(&place, target, input, &core.store);
    let p = match g {
        G::OwnerAlone => Present::OwnerAlone,
        G::PrivateChannel => Present::Private,
        G::Shared => Present::Shared,
        G::Open => Present::Open,
        G::Unknown => Present::Unknown,
    };
    match (class, p) {
        (crate::places::PlaceClass::Shared, Present::OwnerAlone | Present::Private) => {
            Present::Shared
        }
        (_, p) => p,
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

/// The turn's point of view.
pub(crate) fn pov_of(
    core: &Core,
    session: &str,
    turn: &str,
    dir: Option<&str>,
    class: crate::places::PlaceClass,
) -> PointOfView {
    let ex = exchange::exchange(&core.store, turn).ok().flatten();
    let here = anchor(core, session, ex.as_ref(), dir);
    let present = match here {
        Some(_) => presence(core, session, ex.as_ref(), class),
        None => Present::Unknown,
    };
    let person = ex
        .as_ref()
        .filter(|e| e.opened_by == Opener::Message)
        .and_then(|e| e.person.clone());
    PointOfView {
        anchor: Anchor::Place(here.unwrap_or_else(|| exchange::rebuild::UNKNOWN.into())),
        person,
        present,
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
    if pov.present == Present::Unknown {
        return Err(
            "context.history reads nothing here: who can see this conversation cannot be established".into(),
        );
    }
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
    let places = crate::view_gate::RecallPlaces::default();
    let private = |s: &str| places.of_session(&core.runner, s) == Place::Private;
    let hidden = |s: &str| s != session && core.ladder_hidden(s);
    let mut lens = page::Lens {
        store: &core.store,
        here: here.clone(),
        present: pov.present,
        hidden: &hidden,
        private: &private,
        in_view: page::in_view(&core.store, session).map_err(|e| format!("{e:#}"))?,
        words: None,
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
        text: page::render(&p, here, used.looks, resume.as_ref()),
        meta: json!({
            "lines": p.lines.len(), "end": p.end, "next": p.next.as_ref().or(resume.as_ref()),
            "look": used.looks + 1, "of": LOOKS_PER_TURN, "page": PAGE_EXCHANGES,
        }),
    })
}

/// The turns the words index finds for `words` among `scope`, the sessions this point of view may read on the
/// timeline walked: the search is scoped to them before it ranks anything, so what may not be read can
/// neither crowd out nor change what may.
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
        threads: None,
        persona: None,
        category: None,
    };
    // Each session's place is read as its own, so recall's place rule drops none of the scope: the scope is
    // already what this point of view may read.
    let m = memory.manifest(
        &scene,
        &begun,
        answer,
        |s| match scope.contains(s) {
            true => Place::Private,
            false => Place::Unknown,
        },
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

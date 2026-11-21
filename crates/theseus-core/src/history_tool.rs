//! `context.history` (issue 44; design §6, rung 3): the history tool, offered on every turn, by which the model
//! extends the conversation it can see when it thinks it is missing something. The interface is the view's
//! (`view::history`); this module is its implementation over the exchange ledger (`crate::exchange`).
//!
//! - **Bare**, it returns the [`PAGE_EXCHANGES`] exchanges of this place that come before the oldest exchange the
//!   model sees in full: in today's transcript assembly, before the newest exchange the compilation cut or
//!   cleared anything of (`page::in_view`). A page's `next` cursor, passed back as `before`, gives the ones
//!   before those, with no horizon. One line an exchange: its date, the place, the person's words (cut by code
//!   at 300 characters), the `did` line or the reply's beginning, and a handle.
//! - **Open**: a handle gives the exchange in spoken form, cut at about 3,000 tokens (`open`).
//! - **Narrowing**: `place` (another private place, only where the owner is alone), `thing` (its timeline in
//!   the touch index), `words` (the words index, as a filter on the walk).
//! - **The rules** (`page::Lens::admits`): every line and every open passes the place rule (in company, and
//!   wherever the owner is not alone, this place alone) and refuses a sealed or seal-pending thread's session.
//!   The shared gate (`view::gate`) is not in this branch's base yet, so the two checks are applied here
//!   through the place rule (`TurnRunner::place_of`) and the seal lens (`compiler::place_digest::held`).
//!   [`LOOKS_PER_TURN`] pages a turn, then a typed "allowance used"; "history is exhausted" is told apart.
//! - **Offered everywhere**: a public tool (`places::public_tool`) in the `tools` family, which is never
//!   deferred (`tools_defer::family::hideable`). Costs nothing until called: the core is reached through a weak
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
use crate::exchange::{self, OwnerMark, OWNER};
use crate::view::history::{History, HistoryArgs, HISTORY_TOOL, LOOKS_PER_TURN, PAGE_EXCHANGES};
use crate::view::{Anchor, Arrival, Audience, PointOfView, Snapshot};
use crate::Core;

pub(crate) mod open;
pub(crate) mod page;
#[cfg(test)]
mod tests;

/// Its name, as the config's `[policy.tools]` checks it.
pub const NAMES: [&str; 1] = [HISTORY_TOOL];

/// The turns a words filter may name, at most.
const WORDS_K: usize = 100;
const DEADLINE: Duration = Duration::from_secs(2);
/// Turns whose looks are counted at once.
const TURNS_KEPT: usize = 256;

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
        "Read back further through this conversation's history. Bare: the 100 exchanges before the oldest one you see in full, one line each with a handle; pass a page's before to go further back; open a handle to read that exchange."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "before": {"type": "object", "description": "The cursor a page ended with"},
                "open": {"type": "string", "description": "A line's handle: that exchange, as said"},
                "words": {"type": "string", "description": "Only exchanges that say these words"},
                "thing": {"type": "string", "description": "Only exchanges that named a thing (type:value)"},
                "place": {"type": "string", "description": "Another private place's history"}
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
            (Some(h), _) => format!("open exchange {}", h.0),
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

/// What `context.history` reads: the core, once built, and how many pages each turn has read.
#[derive(Default)]
pub struct Board {
    core: OnceLock<Weak<Core>>,
    looks: Mutex<HashMap<String, u32>>,
}

/// One frozen read for the point of view (`view::Snapshot`): the ledger's timeline and the clock.
struct Now<'a> {
    ledger: page::Ledger<'a>,
    now_ms: u64,
}

impl Snapshot for Now<'_> {
    fn now_ms(&self) -> u64 {
        self.now_ms
    }
    fn timeline(&self) -> &dyn crate::view::Timeline {
        &self.ledger
    }
    fn at_hand(&self, _: &Anchor) -> Option<crate::view::AtHand> {
        None
    }
}

/// The turn's point of view, from what the ledger recorded of its exchange: its place (else the session's own,
/// as the ledger would write it), its person, and `owner alone` only for a private place and an input carrying
/// the owner's mark. A turn the ledger did not record (a task's, the ledger off) is no more than private.
pub(crate) fn pov_of(tc: &crate::toolrun::TurnCtx<'_>) -> PointOfView {
    let ex = exchange::exchange(tc.store, tc.turn_id).ok().flatten();
    let here = ex.as_ref().map_or_else(
        || {
            exchange::place_of(
                tc.outbox
                    .try_target(tc.session_id)
                    .ok()
                    .flatten()
                    .as_deref(),
                tc.dir,
            )
        },
        |e| e.place.clone(),
    );
    let mark = ex
        .as_ref()
        .and_then(|e| e.ask.as_deref())
        .and_then(|n| exchange::mark(tc.store, n).ok().flatten());
    let person = mark.as_ref().and_then(OwnerMark::person);
    let arrival = Arrival {
        anchor: Anchor::Place(here),
        owner_marked: person.as_deref() == Some(OWNER),
        person,
        audience: match tc.class {
            crate::places::PlaceClass::Private => Audience::Private,
            crate::places::PlaceClass::Shared => Audience::Shared,
        },
    };
    let now = Now {
        ledger: page::Ledger(tc.store),
        now_ms: theseus_protocol::now_unix_ms(),
    };
    PointOfView::compute(&arrival, &now)
}

impl Board {
    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    /// This turn's pages so far, and one more counted.
    fn look(&self, turn: &str) -> u32 {
        let mut m = self.looks.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() >= TURNS_KEPT && !m.contains_key(turn) {
            m.clear();
        }
        let n = m.entry(turn.into()).or_insert(0);
        let was = *n;
        *n = n.saturating_add(1);
        was
    }

    /// The call, in its own task.
    pub fn run(&self, tc: &crate::toolrun::TurnCtx<'_>, input: &Value) -> AsyncMediaRun {
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
        let pov = pov_of(tc);
        let session = tc.session_id.to_string();
        let looks = match a.open {
            Some(_) => 0,
            None => self.look(tc.turn_id),
        };
        Box::pin(async move {
            answer(&core, &session, &pov, &a, looks)
                .await
                .map(|o| (o, None, None))
                .map_err(ToolFailure::new)
        })
    }
}

/// The sessions of every sealed or seal-pending thread, the asking session's own aside.
fn held(core: &Core, session: &str) -> HashSet<String> {
    core.threads()
        .all()
        .filter(|t| crate::compiler::place_digest::held(t))
        .flat_map(|t| core.thread_sessions(t))
        .filter(|s| s != session)
        .collect()
}

/// A call's answer, for `session` seen from `pov`.
pub(crate) async fn answer(
    core: &Arc<Core>,
    session: &str,
    pov: &PointOfView,
    a: &HistoryArgs,
    looks: u32,
) -> Result<ToolOutput, String> {
    let Anchor::Place(here) = &pov.anchor else {
        return Err("context.history reads a place's history, and this turn has none".into());
    };
    let words = match a.words.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
        Some(w) if a.open.is_none() => Some(found(core, w, pov, here).await?),
        _ => None,
    };
    let private = |s: &str| core.runner.place_of(s) == Place::Private;
    let lens = page::Lens {
        store: &core.store,
        here: here.clone(),
        present: pov.present,
        held: held(core, session),
        private: &private,
        in_view: page::in_view(&core.store, session).map_err(|e| format!("{e:#}"))?,
        words,
    };
    if let Some(h) = &a.open {
        let text = open::open(&lens, h)?;
        return Ok(ToolOutput {
            text,
            meta: json!({"open": h.0}),
        });
    }
    let p = lens.page(pov, a, looks).map_err(|e| format!("{e:#}"))?;
    Ok(ToolOutput {
        text: page::render(&p, here, looks),
        meta: json!({
            "lines": p.lines.len(), "end": p.end, "next": p.next, "look": looks + 1,
            "of": LOOKS_PER_TURN, "page": PAGE_EXCHANGES,
        }),
    })
}

/// The turns whose words the words index finds for `words`, through recall's pipeline with the place rule
/// first, as this place may draw on it: the filter a page walks with.
async fn found(
    core: &Arc<Core>,
    words: &str,
    pov: &PointOfView,
    here: &str,
) -> Result<HashSet<String>, String> {
    let memory = &core.runner.memory;
    let deadline = Duration::from_millis(memory.cfg().recall_deadline_ms).max(DEADLINE);
    let mut begun = memory.begin_within(
        words.to_string(),
        None,
        WORDS_K,
        MemoryArm::Baseline,
        deadline,
        None,
        Vec::new(),
    );
    begun.max_items = Some(WORDS_K);
    let answer = begun.answer().await;
    let place = match pov.present {
        crate::view::Present::OwnerAlone | crate::view::Present::Private => Place::Private,
        _ => Place::Shared(here.to_string()),
    };
    let scene = crate::recall::Scene {
        mode: "context.history",
        session_id: None,
        turn_id: None,
        place,
        in_context: Default::default(),
        labeled: memory.labeled(&core.store).map_err(|e| format!("{e:#}"))?,
        budget_tokens: None,
        max_items: None,
        science: memory.science_for(MemoryArm::Baseline),
        activation: None,
        threads: None,
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
        .filter_map(|i| core.store.get_node(&i.node_id).ok().flatten())
        .filter_map(|(_, n)| n.turn_id)
        .collect())
}

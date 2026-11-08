//! `card.observed` (theseus-6f8t0): how a turn reaches what the live observer
//! has kept about the owner beyond the current day, for when he asks what it
//! has learned about him.
//!
//! The observer keeps a self-fact said in passing as an unconfirmed
//! `card.obs.<id>` note (`card/observe.rs`), and the nightly live pass turns
//! it into a question he answers. Until he answers, the note is on no card,
//! so a turn asked "what have you learned about me today" had nothing to
//! name: the note was reachable only from the CLI, the RPC and the cockpit.
//!
//! Two things reach them now, divided by span. The day's open notes ride in
//! the request's tail as a bounded block (`card/noted.rs`), which is what
//! makes the answer to that question certain rather than a matter of the
//! model electing to look. This tool is the read for what the block does not
//! carry: a longer span, or more notes than its eight lines hold.
//!
//! What the wall around the notes is for is kept whole:
//!
//! - **Nothing past the day's block enters a prompt.** The notes are still
//!   never a node, never indexed and never recalled, and no span beyond
//!   today is in a turn's system or tail. Only the tool's contract is in the
//!   request, as `memory.lookup`'s is for the memory it does not carry
//!   either; a turn that does not ask reads none.
//! - **The owner's own places only.** Refused unless he is alone there
//!   (`capture::owner_alone`): a shared place, and a private place others
//!   write in, get nothing. The gate refuses it in a shared place too, since
//!   it is no public tool (`places::offered`).
//! - **His own conversation only, never a task's session.** A task's place is
//!   its parent's, so the place checks pass one through; the day's block is
//!   kept out of a task by a guard of its own (`turn/compile_step.rs`), and
//!   the writer by another (`card/observe.rs`). This is that guard, for the
//!   read.
//! - **Never to a model of restricted origin.** The same rule the day's
//!   block keeps (`card/noted.rs`, the turn's `restricted_profile`): these
//!   are the owner's personal words, unconfirmed and uncurated. The data
//!   gate cannot stand in for it — it routes a *labelled* session off such a
//!   model, and these notes raise no label, being never nodes and never
//!   indexed, so the protection they would need is the one thing they never
//!   create.
//! - **His words, not a portrait.** The answer is the kept notes verbatim,
//!   each with the day it was kept and the conversation it was said in, and
//!   says plainly that they are unconfirmed and on no card yet. Nothing is
//!   inferred here, and nothing becomes a record without his answer.
//! - **Costs nothing until called**: no state and no read, the core reached
//!   through a weak handle given at build (`Board::attach`).

use std::sync::{Arc, OnceLock, Weak};

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_protocol::{ObservationState, Plan};

use crate::recall::when::When;
use theseus_tools::{
    AsyncMediaRun, Backend, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

use crate::judge::spend::local_day;
use crate::rpc::Core;

/// The tool's name and family.
pub const OBSERVED: &str = "card.observed";
pub const FAMILY: &str = "card";
/// Its name, as the config's `[policy.tools]` checks it.
pub const NAMES: [&str; 1] = [OBSERVED];

/// The notes one call names, by default and at most.
const PAGE: usize = 12;
const PAGE_MAX: usize = 40;
/// The span a call takes when it names none: what he asked about.
const TODAY: &str = "today";

/// Why a place that is not the owner's own gets nothing.
pub const NOT_ALONE: &str =
    "card.observed reads the owner's card, which is theirs alone: this place is shared, \
     or others write in it, so nothing of the card is read here";

/// Why a task's session gets nothing, as the day's block gets none either
/// (`turn/compile_step.rs`): a task's words are its own, not the owner's
/// conversation, though its place is the one it reports into.
pub const NOT_THIS_SESSION: &str =
    "card.observed reads what the owner said in their own conversation: this is a task's \
     session, so nothing of the card is read here";

/// Why a model of restricted origin gets nothing, as the day's block gives
/// one nothing either (`card/noted.rs`).
pub const NOT_THIS_MODEL: &str =
    "card.observed reads the owner's own unconfirmed words about themselves, which no model \
     of restricted origin is given: this turn runs on one, so nothing of the card is read here";

/// What the answer says the notes are, before the lines.
const HEAD: &str = "Kept about the owner since the exchange it was said in, from the owner's own \
words. None is on the card yet and none is confirmed: the owner is asked about these, and only \
their answer keeps one. Name them when asked what you have learned about them; do not treat one \
as standing.";

/// What the answer says when nothing was kept.
const NONE: &str = "Nothing has been kept about the owner in that time.";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    #[serde(default)]
    when: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

impl Args {
    fn parse(input: &Value) -> Result<Self, String> {
        let a: Self =
            serde_json::from_value(input.clone()).map_err(|e| format!("bad input: {e}"))?;
        a.span(theseus_protocol::now_unix_ms())?;
        Ok(a)
    }

    fn limit(&self) -> usize {
        self.limit.unwrap_or(PAGE).clamp(1, PAGE_MAX)
    }

    fn words(&self) -> &str {
        self.when
            .as_deref()
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .unwrap_or(TODAY)
    }

    /// The span the call takes, read as a turn's words are: today when it
    /// names none. `memory.lookup`'s reader, so the two take the same words.
    fn span(&self, now_ms: u64) -> Result<When, String> {
        let w = self.words();
        crate::recall::when::read_local(w, now_ms).ok_or_else(|| {
            format!("when {w:?} names no time: write today, this week, 2026-03-14 or March 2026")
        })
    }
}

/// The registry's `card.observed`: its name, schema and plan, for the gate.
/// The runtime runs it ([`Board::run`]).
pub struct CardObserved;

impl Tool for CardObserved {
    fn name(&self) -> &'static str {
        OBSERVED
    }
    fn description(&self) -> &'static str {
        "What you have kept about the owner from their own words, newest first: the unconfirmed notes the live observer made at the end of an exchange, which are on their card only once they answer. Read it when they ask what you have learned or noticed about them, or what you know about them lately; for what was said or done before, use memory_lookup instead. Their own places only. Read-only."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "when": {"type": "string",
                    "description": "The span, in the daemon's time zone, as memory_lookup takes it: \"today\" (the default), \"this week\", \"2026-03-14\", \"March 2026\""},
                "limit": {"type": "integer", "minimum": 1, "maximum": PAGE_MAX,
                    "description": "Notes at most, 12 by default"}
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
        Ok(Plan {
            summary: format!("read what was kept about the owner: {}", a.words()),
            ..Default::default()
        })
    }
    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "card.observed runs through the runtime, which reads the card",
        ))))
    }
}

/// What the turn asking is, as the wall around the notes reads it: where its
/// words go and which profile it runs on. [`Board::run`] takes both from the
/// turn's `TurnCtx`; a test builds them, so both reach the same [`refusal`].
/// Whether the session is a task's is not here: [`is_task`] reads that from
/// the session's own record, so no caller can get it wrong.
#[derive(Clone)]
pub(crate) struct Asking {
    pub class: crate::places::PlaceClass,
    /// The profile the turn runs on (`TurnCtx::target`). `None` is a turn
    /// whose model cannot be named, which is refused as a restricted one is:
    /// `restricted_profile` already counts a profile that does not resolve.
    pub profile: Option<String>,
}

/// Whether `session` is a task's, read from its own record as
/// `turn/compile_step.rs` reads it for the day's block. A record that does
/// not read is taken for one: the wall closes on what it cannot see.
fn is_task(core: &Core, session: &str) -> bool {
    match core
        .store
        .get_session::<crate::session::SessionRecord>(session)
    {
        Ok(Some(s)) => s.task.is_some(),
        _ => true,
    }
}

/// Why this turn reads none of the notes, if it reads none: the whole wall,
/// in one place, so the runtime and a test cannot drift apart. Each clause
/// is the one `card/noted.rs` keeps for the day's block.
fn refusal(core: &Core, who: &Asking, session: &str) -> Option<&'static str> {
    if who.class != crate::places::PlaceClass::Private {
        return Some(NOT_ALONE);
    }
    if is_task(core, session) {
        return Some(NOT_THIS_SESSION);
    }
    if who
        .profile
        .as_deref()
        .is_none_or(|p| core.runner.restricted_profile(p))
    {
        return Some(NOT_THIS_MODEL);
    }
    if !core.owner_alone_in(session) {
        return Some(NOT_ALONE);
    }
    None
}

/// What `card.observed` reads: the core, given once it is built.
#[derive(Default)]
pub struct Board {
    core: OnceLock<Weak<Core>>,
}

impl Board {
    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    /// The call, in its own task. The whole wall is [`refusal`], read from
    /// the turn: its place's class and the session's place, its profile's
    /// origin, and whether it is a task's.
    pub fn run(&self, tc: &crate::toolrun::TurnCtx<'_>, input: &Value) -> AsyncMediaRun {
        let fail = |why: &str| -> AsyncMediaRun {
            Box::pin(std::future::ready(Err(ToolFailure::new(why))))
        };
        let Some(core) = self.core.get().and_then(Weak::upgrade) else {
            return fail("card.observed is not ready: the core is not built");
        };
        let who = Asking {
            class: tc.class,
            profile: tc.target.map(|t| t.profile.clone()),
        };
        let a = match Args::parse(input) {
            Ok(a) => a,
            Err(e) => return fail(&e),
        };
        let session = tc.session_id.to_string();
        // The wall and the read both read the store, and an async block keeps
        // them inside the spawned task rather than on the dispatch's thread.
        Box::pin(async move {
            observed(core, &who, session, a)
                .map(|o| (o, None, None))
                .map_err(ToolFailure::new)
        })
    }
}

/// A call as the runtime makes it, for a session and the turn asking: tests
/// reach the tool's own wall this way, past the gate.
#[cfg(test)]
pub(crate) fn call(
    core: Arc<Core>,
    who: &Asking,
    session: &str,
    input: Value,
) -> Result<ToolOutput, String> {
    observed(core, who, session.to_string(), Args::parse(&input)?)
}

/// [`Asking`] for an ordinary turn of the owner's own, on `profile`: what a
/// test varies one clause of.
#[cfg(test)]
pub(crate) fn owners_turn(profile: &str) -> Asking {
    Asking {
        class: crate::places::PlaceClass::Private,
        profile: Some(profile.to_string()),
    }
}

/// A note's kind in a word, as the CLI's listing names it.
fn kind_word(k: theseus_protocol::ObservationKind) -> String {
    serde_json::to_value(k)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The read: the open notes of the span, newest first, with the head that
/// says what they are, behind the whole wall ([`refusal`]).
fn observed(core: Arc<Core>, who: &Asking, session: String, a: Args) -> Result<ToolOutput, String> {
    if let Some(why) = refusal(&core, who, &session) {
        return Err(why.to_string());
    }
    let now = theseus_protocol::now_unix_ms();
    let span = a.span(now)?;
    let all = theseus_store::blocking(|| {
        crate::card::state::observations(&core.store, Some(ObservationState::Open))
    })
    .map_err(|e| format!("the card's notes do not read: {e:#}"))?;
    // `observations` is newest first already; the window is the call's.
    let open = all.len();
    let mine: Vec<&theseus_protocol::CardObservation> = all
        .iter()
        .filter(|o| {
            span.from_ms.is_none_or(|f| o.at_ms >= f) && span.to_ms.is_none_or(|t| o.at_ms < t)
        })
        .take(a.limit())
        .collect();
    let mut text = String::new();
    if mine.is_empty() {
        text.push_str(NONE);
    } else {
        text.push_str(HEAD);
        text.push_str("\n\n");
        for (i, o) in mine.iter().enumerate() {
            text.push_str(&format!(
                "{}. [{}] {} — {} ({}, said in {})\n",
                i + 1,
                local_day(o.at_ms),
                kind_word(o.kind),
                o.text,
                o.id,
                o.session
            ));
        }
    }
    Ok(ToolOutput {
        text,
        meta: json!({"observations": mine.len(), "open": open, "when": span.said}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arguments, without a core: a call that names no span takes today,
    /// a page is capped, and an unreadable span is the model's mistake.
    #[test]
    fn a_call_that_names_no_span_takes_today() {
        let now = theseus_protocol::now_unix_ms();
        let a = Args::parse(&json!({})).unwrap();
        assert_eq!((a.words(), a.limit()), (TODAY, PAGE));
        let span = a.span(now).unwrap();
        let from = span.from_ms.unwrap();
        assert!(from <= now && now - from < 86_400_000, "{from} {now}");
        assert_eq!(local_day(from), local_day(now), "this local day");
        assert!(span.to_ms.is_none_or(|t| t > now), "{span:?}");

        // A span of its own, and the page's cap.
        let a = Args::parse(&json!({"when": "March 2026", "limit": 500})).unwrap();
        assert_eq!(a.limit(), PAGE_MAX, "the page is capped");
        let march = a.span(now).unwrap();
        assert!(march.to_ms.unwrap() < now, "{march:?}");

        // Nonsense names no time, and a stray key is refused.
        let why = Args::parse(&json!({"when": "the moon"})).unwrap_err();
        assert!(why.contains("names no time"), "{why}");
        assert!(Args::parse(&json!({"days": 2})).is_err(), "no stray keys");
    }

    /// The tool's own contract, which the gate and the deferral read.
    #[test]
    fn the_tool_is_a_read_in_its_own_family() {
        let t = CardObserved;
        assert_eq!(t.name(), OBSERVED);
        assert_eq!(t.family(), FAMILY);
        assert_eq!(t.wire_name(), "card_observed");
        assert!(matches!(t.class(), ToolClass::Read));
        assert!(!crate::places::offered(
            crate::places::PlaceClass::Shared,
            OBSERVED,
            &[]
        ));
        // Deferred everywhere, in a family of its own with a line that says
        // what it holds: the question is not one of every five sessions, so
        // it is found by a search and not carried in every request.
        use crate::tools_defer::family;
        assert_eq!(family::of_wire(&t.wire_name()), family::CARD);
        assert!(family::hideable(family::CARD));
        assert!(family::FAMILIES.iter().any(|(f, _)| *f == family::CARD));
    }
}

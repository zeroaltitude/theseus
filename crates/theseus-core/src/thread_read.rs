//! `thread.read` (theseus-f2g0): the model reads another thread on purpose.
//! One thread's facts reach another only when relevant or asked, and always
//! named: this is the asked way. The answer is the thread's card and, for a
//! `query`, its top recall hits, each cited by thread and position
//! (`[thread «title», 2026-10-08, @4210]`); nothing of it is kept in the
//! asking thread's prefix beyond the result.
//!
//! - **Private places only**, as `memory.lookup` is: never offered in a shared
//!   place, refused there by the gate, and checked again here.
//! - **A sealed thread asks the owner first**: [`gate`] raises the call to
//!   `approve` when its thread is sealed or a seal is proposed, an approval
//!   question on the asking turn's call. The owner's yes is the only way in.
//! - **Costs nothing until called**: the core is reached through a weak
//!   handle given at build (`Board::attach`).

use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_memory::recall::Place;
use theseus_protocol::threads::ThreadInfo;
use theseus_protocol::Plan;
use theseus_tools::{
    AsyncMediaRun, Backend, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

use crate::config::memory::MemoryArm;
use crate::policy::{Decision, Posture};
use crate::Core;

/// The tool's name and family.
pub const READ: &str = "thread.read";
pub const FAMILY: &str = "thread";
/// Its name, as the config's `[policy.tools]` checks it.
pub const NAMES: [&str; 1] = [READ];

/// The hits a read shows, and their longest text.
const HITS: usize = 6;
const DEADLINE: Duration = Duration::from_secs(2);

/// Why a shared place gets nothing.
pub const SHARED: &str = "thread.read opens another thread, and threads are the owner's alone: \
     this place is shared, so no thread is read here";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    thread: String,
    #[serde(default)]
    query: Option<String>,
}

impl Args {
    fn parse(input: &Value) -> Result<Self, String> {
        let a: Self =
            serde_json::from_value(input.clone()).map_err(|e| format!("bad input: {e}"))?;
        match a.thread.trim().is_empty() {
            true => Err("name the thread to read: its slug or its title".into()),
            false => Ok(a),
        }
    }

    fn query(&self) -> Option<&str> {
        self.query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
    }
}

/// The thread `name` means: a slug, else a title (any case).
fn find<'a>(board: &'a crate::threads::Board, name: &str) -> Option<&'a ThreadInfo> {
    let name = name.trim();
    board.get(name).or_else(|| {
        let lower = name.to_lowercase();
        board.all().find(|t| t.title.to_lowercase() == lower)
    })
}

/// The registry's `thread.read`: its name, schema and plan, for the gate.
/// The runtime runs it ([`Board::run`]).
pub struct ThreadRead;

impl Tool for ThreadRead {
    fn name(&self) -> &'static str {
        READ
    }
    fn description(&self) -> &'static str {
        "Read another thread's card and, for a query, its top notes (cited by thread and position), when the work needs what that thread knows; a sealed thread asks the owner first."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "thread": {"type": "string", "description": "The thread's slug or title"},
                "query": {"type": "string", "description": "What to find in it"}
            },
            "required": ["thread"],
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
            summary: match a.query() {
                Some(q) => format!("read the thread {}: {q:?}", a.thread.trim()),
                None => format!("read the thread {}", a.thread.trim()),
            },
            ..Default::default()
        })
    }
    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "thread.read runs through the runtime, which reads the threads",
        ))))
    }
}

/// What `thread.read` reads: the core, given once it is built.
#[derive(Default)]
pub struct Board {
    core: OnceLock<Weak<Core>>,
}

impl Board {
    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    fn core(&self) -> Option<Arc<Core>> {
        self.core.get().and_then(Weak::upgrade)
    }

    /// The call, in its own task: refused in a shared place.
    pub fn run(&self, tc: &crate::toolrun::TurnCtx<'_>, input: &Value) -> AsyncMediaRun {
        let fail = |why: &str| -> AsyncMediaRun {
            Box::pin(std::future::ready(Err(ToolFailure::new(why))))
        };
        if tc.class != crate::places::PlaceClass::Private {
            return fail(SHARED);
        }
        let Some(core) = self.core() else {
            return fail("thread.read is not ready: the core is not built");
        };
        let a = match Args::parse(input) {
            Ok(a) => a,
            Err(e) => return fail(&e),
        };
        let session = tc.session_id.to_string();
        Box::pin(async move {
            read(core, session, a)
                .await
                .map(|o| (o, None, None))
                .map_err(ToolFailure::new)
        })
    }
}

/// A call as the runtime makes it, for a session: tests reach the tool's own
/// check this way, past the gate.
#[cfg(test)]
pub(crate) async fn call(
    core: Arc<Core>,
    session: &str,
    input: Value,
) -> Result<ToolOutput, String> {
    read(core, session.to_string(), Args::parse(&input)?).await
}

/// The gate's step (theseus-f2g0): a `thread.read` of a sealed thread, or of
/// one whose seal is proposed, waits for the owner. It never loosens the
/// call's own decision.
pub(crate) fn gate(
    rt: &crate::toolrun::ToolRuntime,
    tool: &dyn Tool,
    input: &Value,
    d: Decision,
) -> Decision {
    if tool.name() != READ {
        return d;
    }
    let Some(core) = rt.thread_read.core() else {
        return d;
    };
    let Ok(a) = Args::parse(input) else {
        return d;
    };
    let board = core.threads();
    match find(&board, &a.thread) {
        Some(t) if t.sealed.is_some() || t.seal_pending.is_some() => d.at_least(
            Posture::Approve,
            &format!(
                "the thread {} is sealed (or a seal is proposed): the owner decides whether another thread reads it",
                t.slug
            ),
            "thread seal",
            READ,
            &format!("read the sealed thread {}", t.slug),
        ),
        _ => d,
    }
}

async fn read(core: Arc<Core>, session: String, a: Args) -> Result<ToolOutput, String> {
    if !matches!(core.runner.place_of(&session), Place::Private) {
        return Err(SHARED.into());
    }
    let board = core.threads();
    let Some(t) = find(&board, &a.thread).cloned() else {
        let names: Vec<&str> = board.all().map(|t| t.slug.as_str()).take(20).collect();
        return Err(format!(
            "no thread {:?}: the threads are {}",
            a.thread.trim(),
            if names.is_empty() {
                "none yet".to_string()
            } else {
                names.join(", ")
            }
        ));
    };
    let mut text = card_text(&t);
    let mut shown = 0usize;
    if let Some(q) = a.query() {
        let sessions = core.thread_sessions(&t);
        if sessions.is_empty() {
            text.push_str("\nIt has no session yet, so there are no notes to search.");
        } else {
            let hits = hits(&core, q, sessions).await?;
            shown = hits.len();
            for (i, h) in hits.iter().enumerate() {
                text.push_str(&format!(
                    "\n{}. [thread «{}», {}, @{}] {}",
                    i + 1,
                    t.title,
                    h.day,
                    h.position,
                    h.text
                ));
            }
            if shown == 0 {
                text.push_str(&format!("\nNothing in it matched {q:?}."));
            }
        }
    } else {
        text.push_str("\nPass a query to search its notes.");
    }
    Ok(ToolOutput {
        text,
        meta: json!({"thread": t.slug, "sealed": t.sealed.is_some(), "shown": shown}),
    })
}

/// The card, in words.
fn card_text(t: &ThreadInfo) -> String {
    let mut s = format!(
        "Thread «{}» ({}, {}){}",
        t.title,
        t.slug,
        t.state.as_str(),
        if t.sealed.is_some() {
            " 🔒 sealed"
        } else {
            ""
        }
    );
    if !t.goal.is_empty() {
        s.push_str(&format!("\nGoal: {}", t.goal));
    }
    if t.card.is_empty() {
        s.push_str("\nIts card is empty.");
    } else {
        s.push_str("\nCard:");
        for l in &t.card {
            s.push_str(&format!("\n- {l}"));
        }
    }
    s
}

struct Hit {
    day: String,
    position: u64,
    text: String,
}

/// The top notes for `query` in `sessions`, through recall's pipeline with the
/// place rule first, whole.
async fn hits(core: &Arc<Core>, query: &str, sessions: Vec<String>) -> Result<Vec<Hit>, String> {
    let memory = &core.runner.memory;
    let deadline = Duration::from_millis(memory.cfg().recall_deadline_ms).max(DEADLINE);
    let mut begun = memory.begin_within(
        query.to_string(),
        None,
        HITS * 4,
        MemoryArm::Baseline,
        deadline,
        None,
        sessions,
    );
    begun.max_items = Some(HITS);
    let answer = begun.answer().await;
    let scene = crate::recall::Scene {
        mode: "thread.read",
        session_id: None,
        turn_id: None,
        place: Place::Private,
        in_context: Default::default(),
        labeled: memory.labeled(&core.store).map_err(|e| format!("{e:#}"))?,
        budget_tokens: None,
        science: memory.science_for(MemoryArm::Baseline),
        activation: None,
        threads: None,
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
            "the index did not answer ({}): the thread's card is above, its notes could not be searched",
            m.outcome
        ));
    }
    Ok(m.admitted
        .iter()
        .take(HITS)
        .map(|i| {
            let at = core
                .store
                .get_node(&i.node_id)
                .ok()
                .flatten()
                .map_or(0, |(_, n)| n.created_at_ms);
            Hit {
                day: crate::recall::render::utc_day(at),
                position: i.position,
                text: i
                    .text
                    .as_deref()
                    .unwrap_or_default()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            }
        })
        .collect())
}

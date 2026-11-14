//! **What the owner says in chat is acted on by a tool the model chooses to
//! call, never by matching the text of his message** (chat-acts-are-tools,
//! S43-1; the owner, 2026-11-13 23:27 and 23:32, Discord message
//! `1571050236367995101`).
//!
//! > *"5.0.2 seems like a misfire feature: we do want things to be executable
//! > from chat, but this is NOT a matching thing -- it's a surface area of
//! > tools that models can choose to call in response to the prompt and the
//! > tool list (and theoretically, they could be triggered by a jev
//! > classificaiton too)"*
//!
//! > *"i don't think these should be exact word matches, but clearly
//! > articulated tools sensitive to the question context that the model can
//! > choose to call as it judges the human response."*
//!
//! Three routes used to read the TEXT of his message and act on a match:
//! `rpc/answer_in_words.rs` (a yes to a waiting question), `rpc/allow_in_words.rs`
//! (his S17-D6 "run what was originally suggested") and, on a branch that does
//! not land, `sync_in_words`. All three are gone. **After this module no code
//! path acts on a substring or token match of his message.** What replaces
//! them is three tools:
//!
//! - [`ANSWER`] answers the question waiting here, with one of its answers.
//! - [`ALLOW`] lets the held call run as it was first proposed.
//! - [`SYNC`] asks for an import sync now.
//!
//! **Context-sensitivity is by construction, not by prose.** None of the three
//! is in a place's kept catalog ([`conditional`] holds them out of
//! `toolrun::catalogs`); each turn's [`Board::offers`] puts in only those that
//! apply *to this session, now*, with a description naming what is waiting or
//! what is held. A turn with nothing waiting is offered no answer tool at all,
//! so there is nothing for a model to call by mistake.
//!
//! **The buttons, the slash commands and the CLI are untouched.** `work.answer`,
//! `undo.allow`, `theseus undo allow` and `import.sync.request` are the same
//! methods they were; this is a fourth way in, not a replacement for the three.
//!
//! **The structural rule.** [`ALLOW`] acts only in a turn started by the
//! owner's own message. That is not read off the message's words: at
//! `turn.submit` the harness stages [`Said`] — who spoke, what they said, and
//! whether `Core::owner_input` counts it as the owner's own typed input — and
//! the offer step binds it to the turn it started ([`Heard::claim`]). A turn
//! that no message of his started has nothing bound, so [`Board::said`]
//! returns nothing and the tool refuses. A wake, a task report, a job's turn
//! and an MCP client's reach none of this.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};

use serde_json::{json, Value};
use theseus_tools::Tool as _;

use crate::approval::Answerer;
use crate::Core;

pub mod judged;
mod run;
mod tools;

#[cfg(test)]
mod tests;

pub use run::{held_here, waiting_here};
pub(crate) use tools::named_question;

/// The family the three share, as the deferred tools' board counts families.
pub const FAMILY: &str = "chat";

/// The tool that answers the question waiting in this conversation.
pub const ANSWER: &str = "chat.answer_question";
/// The tool that lets a held call run as it was first proposed.
pub const ALLOW: &str = "chat.allow_held_call";
/// The tool that asks for an OpenClaw import sync now.
pub const SYNC: &str = "chat.import_sync";

/// The three, as `[policy.tools]` names them.
pub const NAMES: [&str; 3] = [ANSWER, ALLOW, SYNC];

/// Whether `name` is a chat act: **never in a place's kept catalog**, and put
/// into a turn's request only by [`Board::offers`], which knows what is
/// waiting here. A kept catalog is shared by every session of a place, and
/// these three are true of one session at one moment.
pub fn conditional(name: &str) -> bool {
    NAMES.contains(&name)
}

/// Whether `wire` (a response's tool name) is a chat act.
pub fn is_chat_act_wire(wire: &str) -> bool {
    NAMES
        .iter()
        .any(|n| wire == theseus_tools::wire_name(n).as_str())
}

/// The message that started a turn: who said it, what it said, and whether it
/// is the owner's own typed input (`Core::owner_input`: the owner, from a
/// private place, no job's `opened_from` and no prompt behind it).
#[derive(Debug, Clone)]
pub struct Said {
    pub text: String,
    pub who: Answerer,
    pub owner_input: bool,
}

/// One staged message and the turn that claimed it.
#[derive(Debug, Clone)]
struct Pending {
    said: Said,
    turn: Option<String>,
}

/// The messages staged at `turn.submit`, one per session, waiting for the turn
/// they started. In memory only: the fact is true of exactly one turn, and a
/// restart has no turn to tell. The shape is `yes_fact::Staged`'s, for the
/// same reason — a turn claims it once and every loop of that turn reads the
/// same thing.
#[derive(Default)]
pub struct Heard {
    by_session: Mutex<HashMap<String, Pending>>,
}

impl Heard {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Pending>> {
        self.by_session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Stage `said` for the next turn in `session`. An unclaimed message is
    /// replaced: the newer one is what the turn runs on.
    pub(crate) fn stage(&self, session: &str, said: Said) {
        self.lock()
            .insert(session.to_string(), Pending { said, turn: None });
    }

    /// Bind the staged message to `turn`, and return it. The first turn to ask
    /// claims it; a later turn clears it and is given nothing, which is what
    /// keeps a message from reaching a turn it did not start.
    ///
    /// Called by the offer step alone, and only for a turn a message started
    /// (never a continuation): see [`Board::offers`].
    fn claim(&self, session: &str, turn: &str) -> Option<Said> {
        let mut held = self.lock();
        let p = held.get_mut(session)?;
        match p.turn.as_deref() {
            None => {
                p.turn = Some(turn.to_string());
                Some(p.said.clone())
            }
            Some(t) if t == turn => Some(p.said.clone()),
            Some(_) => {
                held.remove(session);
                None
            }
        }
    }

    /// How many sessions hold a staged message (the tests').
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lock().len()
    }
}

/// What one turn was OFFERED, kept so that the call acts on exactly that.
///
/// **This is not a cache, it is the contract.** A question is not listed by
/// `confirm_list` once a turn is running in its session, so a tool that asked
/// again at call time would find nothing and refuse every time -- which is the
/// whole difference between a route read at `turn.submit`, BEFORE the turn
/// runs, and a tool called DURING it. The matcher this replaced never met the
/// problem because it acted before the turn began. So the offer step reads
/// what is waiting and what is held once, at the turn's start, puts it in the
/// description the model reads, and keeps it here; the call then acts on the
/// same list the model was shown, and nothing else. `confirm_action` and
/// `undo_allow` still decide for themselves whether the thing can still be
/// acted on, so a question answered from a button in between is refused there,
/// by its own rules.
#[derive(Debug, Clone)]
pub(crate) struct Offered {
    pub turn: String,
    pub said: Said,
    pub waiting: Vec<theseus_protocol::ConfirmRequest>,
    pub held: Vec<theseus_protocol::work::undo::HeldCall>,
}

/// What the chat acts reach: the core, given once it is built
/// ([`Board::attach`], as `thread_read::Board` is given one), the messages
/// heard at `turn.submit`, and what each session's current turn was offered.
#[derive(Default)]
pub struct Board {
    core: OnceLock<Weak<Core>>,
    pub heard: Heard,
    offered: Mutex<HashMap<String, Offered>>,
}

impl Board {
    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    pub(crate) fn core(&self) -> Option<Arc<Core>> {
        self.core.get().and_then(Weak::upgrade)
    }

    /// What `turn` in `session` was offered, when the offer step ran for it.
    /// **This is the structural rule**: a turn no message of the owner's
    /// started was offered nothing and finds nothing here, so a call made in
    /// one -- a wake, a task's report, a model naming a tool it never saw --
    /// is refused.
    pub(crate) fn offered(&self, session: &str, turn: &str) -> Option<Offered> {
        let kept = self.offered.lock().unwrap_or_else(PoisonError::into_inner);
        kept.get(session).filter(|o| o.turn == turn).cloned()
    }

    /// The chat acts this turn is offered, as wire definitions, sorted by name.
    ///
    /// `continuation` is the turn's own read of "no message of a person's
    /// started this" (`Turn::continuation` or `self_authored`): such a turn is
    /// offered nothing and claims nothing, so a wake cannot take a message the
    /// owner meant for the turn before it.
    ///
    /// `class` is where the turn's words go. A shared place is offered none of
    /// the three: a question waiting for the owner, a hold he alone may
    /// release and his own import are all private acts.
    pub fn offers(
        &self,
        session: &str,
        turn: &str,
        class: crate::places::PlaceClass,
        continuation: bool,
    ) -> Vec<Value> {
        if continuation || class != crate::places::PlaceClass::Private {
            return vec![];
        }
        let Some(core) = self.core() else {
            return vec![];
        };
        let Some(said) = self.heard.claim(session, turn) else {
            return vec![];
        };
        let waiting = run::waiting_here(&core, session).unwrap_or_else(|e| {
            tracing::warn!(session, error = %format!("{e:#}"),
                "the chat acts: what waits here could not be read, so no answer tool is offered");
            vec![]
        });
        let held = match said.owner_input {
            true => run::held_here(&core, session).unwrap_or_else(|e| {
                tracing::warn!(session, error = %format!("{e:#}"),
                    "the chat acts: what is held here could not be read, so no allow tool is offered");
                vec![]
            }),
            // The allow tool is his alone, so a turn that is not his own typed
            // input is offered it whatever is held.
            false => vec![],
        };
        let mut defs = Vec::new();
        if !waiting.is_empty() {
            defs.push(definition(
                ANSWER,
                &tools::answer_description(&waiting),
                tools::AnswerQuestion.input_schema(),
            ));
        }
        if !held.is_empty() {
            defs.push(definition(
                ALLOW,
                &tools::allow_description(&held),
                tools::AllowHeldCall.input_schema(),
            ));
        }
        if let Some(d) = Self::sync_offer(&core) {
            defs.push(d);
        }
        self.offered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                session.to_string(),
                Offered {
                    turn: turn.to_string(),
                    said,
                    waiting,
                    held,
                },
            );
        defs.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        defs
    }

    /// The sync tool, where the OpenClaw import is configured at all. **No new
    /// place rule**: `import.sync.request`'s own `judge_act(Act::Import)` is
    /// what decides whether the ask counts, exactly as it does for the CLI,
    /// the web UI and the RPC, and it is asked again when the tool runs.
    fn sync_offer(core: &Core) -> Option<Value> {
        core.cfg.import.openclaw.state_dir.as_ref()?;
        Some(definition(
            SYNC,
            tools::ImportSync.description(),
            tools::ImportSync.input_schema(),
        ))
    }
}

/// One tool's wire definition, as `Registry::definitions_of` writes one.
fn definition(name: &str, description: &str, schema: Value) -> Value {
    json!({
        "name": theseus_tools::wire_name(name),
        "description": description,
        "input_schema": schema,
    })
}

/// Register the three with the runtime's registry: they are ordinary tools to
/// the gate, the ledger and `tools.find`, and only their *offering* is
/// conditional.
pub(crate) fn register(r: &mut theseus_tools::Registry) {
    r.register(Arc::new(tools::AnswerQuestion));
    r.register(Arc::new(tools::AllowHeldCall));
    r.register(Arc::new(tools::ImportSync));
}

//! What the three chat acts do, and the one gate each takes.
//!
//! Every one of them goes through the verb the buttons and the CLI already
//! take — [`Core::confirm_action_with`], [`Core::undo_allow_taken`],
//! [`Core::sync_request`] — so **nothing about what any of these acts MEANS is
//! widened here: only which inputs reach them.** `judge_act` judges a call
//! made from a tool exactly as it judges one typed at a terminal, and it is
//! asked about the OWNER, from the message that started the turn, never about
//! the model.

use anyhow::Result;
use serde_json::{json, Value};
use theseus_protocol::work::undo::{allowed_words, HeldCall, SHORTEST_ID};
use theseus_protocol::ConfirmRequest;

use super::tools::{allow_args, answer_args, named_hold, named_question};
use super::{Board, Said, ALLOW};
use crate::toolrun::holds::Holds;
use crate::toolrun::TurnCtx;
use crate::Core;

/// What a chat act gives back: the words the model reads, and the meta the
/// ledger and the web UI read.
type Done = Result<(String, Value), String>;

/// Every question waiting that the owner is being asked here: this session's,
/// and those of a session that answers at this one's place (a task's own asks
/// where its parent does). A place of its own and nothing else: an answer
/// given in one conversation never answers another's.
///
/// Moved here whole from `rpc/answer_in_words.rs`, which is gone: what it
/// decided about SCOPE was right and is unchanged. Only its reading of the
/// owner's words went.
pub fn waiting_here(core: &Core, session: &str) -> Result<Vec<ConfirmRequest>> {
    let here = core.outbox.target(session);
    Ok(core
        .confirm_list()?
        .into_iter()
        .filter(|q| {
            q.session_id == session || (here.is_some() && core.outbox.target(&q.session_id) == here)
        })
        .collect())
}

/// Every call held that the owner is being offered here, by the same rule.
/// Waiting for an allowance only: a hold he has already allowed is not
/// offered again. Moved here whole from `rpc/allow_in_words.rs`.
pub fn held_here(core: &Core, session: &str) -> Result<Vec<HeldCall>> {
    let here = core.outbox.target(session);
    Ok(Holds::read(&core.store)?
        .list(None, false)
        .into_iter()
        .filter(|h| h.allowed.is_none())
        .filter(|h| {
            h.session_id == session || (here.is_some() && core.outbox.target(&h.session_id) == here)
        })
        .collect())
}

impl Board {
    /// The core and the message that started this turn, or the refusal the
    /// model reads. **The structural rule lives here**: a call made in a turn
    /// no message of the owner's started — a wake, a task's report, a model
    /// naming a tool it was never offered — gets nothing.
    fn standing(&self, tc: &TurnCtx<'_>) -> Result<(std::sync::Arc<Core>, Said), String> {
        let core = self
            .core()
            .ok_or("this tool is not ready: the core is not built")?;
        let said = self.said(tc.session_id, tc.turn_id).ok_or(
            "this tool acts only in a turn the operator's own message started, and no message of \
             theirs started this one. Nothing was done.",
        )?;
        Ok((core, said))
    }

    /// Answer the question waiting here with the answer the operator gave.
    pub(crate) fn answer(&self, tc: &TurnCtx<'_>, input: &Value) -> Done {
        let (core, said) = self.standing(tc)?;
        let a = answer_args(input)?;
        let waiting = waiting_here(&core, tc.session_id)
            .map_err(|e| format!("what is waiting here could not be read: {e:#}"))?;
        let q = one_of(&waiting, a.question.as_deref(), |q| {
            q.correlation_id.clone()
        })
        .map_err(|e| e.said("question", "waiting", &waiting, named_question))?;
        let note = a.note.as_deref().map(str::trim).filter(|t| !t.is_empty());
        let answer = a.answer.trim();
        let name = named_question(q);
        let out = match crate::rpc::answer::kind_of(q) {
            theseus_protocol::work::QuestionKind::Ask => {
                core.answer_ask_act(q, answer, note, said.who.clone())
            }
            _ => core.answer_plain_act(q, answer, note, said.who.clone()),
        }?;
        // What the harness made of it, for the rest of this turn
        // (`yes_fact.rs`): the block says what was answered and what was not,
        // so a turn cannot read a silence as either.
        core.stage_answer_fact(tc.session_id, &name, &waiting, q);
        Ok((
            format!("{out} ({name}). Nothing else waiting here was answered."),
            json!({"chat_act": {"answered": q.correlation_id, "answer": answer}}),
        ))
    }

    /// Let the held call run as it was first proposed.
    ///
    /// Two layers, in this order, and the order is the owner's ruling:
    /// **the model decides**, guided by the tool's description, and then the
    /// `allow_said.v1` judge is asked as defence in depth. A `no` at its bar
    /// refuses with a reason the model can read; a judge that cannot be
    /// reached leaves the call STANDING and the ledger row records that it was
    /// not judged (see `chat_acts::judged`).
    pub(crate) async fn allow(&self, tc: &TurnCtx<'_>, input: &Value) -> Done {
        let (core, said) = self.standing(tc)?;
        if !said.owner_input {
            return Err(
                "this tool acts only on the operator's own message, from a private \
                        place, and this turn's message is not one. Nothing was allowed."
                    .into(),
            );
        }
        let a = allow_args(input)?;
        let held = held_here(&core, tc.session_id)
            .map_err(|e| format!("what is held here could not be read: {e:#}"))?;
        let h = one_of(&held, a.hold.as_deref(), |h| h.id.clone())
            .map_err(|e| e.said("hold", "held", &held, named_hold))?
            .clone();
        let judged = match super::judged::says_run(&core, &said.text, &h, &held).await {
            super::judged::Verdict::No(why) => {
                return Err(format!(
                    "The operator's message does not read as an instruction to run the held call \
                     ({}). Nothing was allowed and the hold stands. {why} Ask them plainly \
                     whether to run it as it was first proposed.",
                    named_hold(&h)
                ))
            }
            super::judged::Verdict::Yes => Some(true),
            // NOT `None`. `None` is the TYPED routes, which ask no second
            // reading at all; `Some(false)` is this route saying that it asked
            // and no judge answered. The fail-open rule owes the ledger that
            // difference, and a row that simply omitted the field would read
            // as `undo.allow` over the wire.
            super::judged::Verdict::Unjudged => Some(false),
        };
        let allowed = core
            .undo_allow_taken(
                &h.id,
                said.who.clone(),
                crate::rpc::Taken {
                    route: Some(ALLOW),
                    judged,
                },
            )
            .map_err(|e| format!("the hold was not allowed: {e:#}"))?;
        Ok((
            allowed_words(&allowed),
            json!({"chat_act": {"allowed": allowed.id, "judged": judged}}),
        ))
    }

    /// Ask for an OpenClaw import sync now.
    ///
    /// **The owner check is `import.sync.request`'s own and no other**:
    /// `judge_act(Act::Import)`, which falls to `places::owner_in_private` —
    /// the CLI, the web UI, a DM with an owner, a thread's own virtual place,
    /// or a guild channel the bindings file bound private. No new place rule
    /// is invented here, and the actor judged is the operator who spoke, never
    /// the model.
    pub(crate) fn sync(&self, tc: &TurnCtx<'_>, input: &Value) -> Done {
        let (core, said) = self.standing(tc)?;
        let _: super::tools::SyncArgs = theseus_tools::parse(input).map_err(|e: String| e)?;
        core.import_act(&said.who)?;
        let started = core.sync_request_for(Some(tc.session_id), &said.who.label);
        Ok((
            started.say.clone(),
            json!({"chat_act": {"sync_started": started.started, "at_ms": started.at_ms}}),
        ))
    }
}

/// Why exactly one of something could not be picked.
pub(crate) enum NotOne {
    /// Nothing of the kind is here at all.
    None,
    /// A name that matches nothing here.
    Unknown(String),
    /// More than one, and the words name none or several.
    Which(Vec<usize>),
}

impl NotOne {
    /// The refusal the model reads, naming each candidate so the next turn can
    /// ask the operator which they mean. **Nothing is ever guessed**: the rule
    /// `undo.allow` takes by id is the rule here.
    fn said<T>(self, field: &str, kind: &str, all: &[T], name: impl Fn(&T) -> String) -> String {
        let list = |ix: &[usize]| {
            ix.iter()
                .filter_map(|i| all.get(*i))
                .map(|t| format!("\n- {}", name(t)))
                .collect::<String>()
        };
        match self {
            NotOne::None => format!("Nothing is {kind} in this conversation. Nothing was done."),
            NotOne::Unknown(w) => format!(
                "`{w}` names nothing {kind} here. Nothing was done. What is {kind} here:{}",
                list(&(0..all.len()).collect::<Vec<_>>())
            ),
            NotOne::Which(ix) => format!(
                "`{field}` names {} of the {} {kind} here, so which one is meant cannot be read. \
                 Nothing was done — ask the operator which they mean:{}",
                match ix.len() {
                    n if n == all.len() => "all".to_string(),
                    n => n.to_string(),
                },
                all.len(),
                list(&ix)
            ),
        }
    }
}

/// Whether `id` is what `want` names: the whole id, or an end of
/// [`SHORTEST_ID`] characters or more, lower case — **exactly**
/// `work::undo::pick`'s rule, applied to a question's correlation id as well
/// as to a hold's, so the two vocabularies cannot drift. `pick` itself is
/// typed to `HeldCall`, which a `ConfirmRequest` is not; the rule is one line
/// and is stated once here rather than fabricated holds being passed to it.
fn names(id: &str, want: &str) -> bool {
    let id = id.to_lowercase();
    id == want || id.ends_with(want)
}

/// The one of `all` that `name` means. With no name and exactly one
/// candidate, that one; with no name and several, a refusal. **Nothing is
/// ever guessed** — the rule that produced the wrong task being abandoned on
/// a bare yes (S17) is the rule here.
fn one_of<'a, T>(
    all: &'a [T],
    name: Option<&str>,
    id: impl Fn(&T) -> String,
) -> Result<&'a T, NotOne> {
    if all.is_empty() {
        return Err(NotOne::None);
    }
    let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) else {
        return match all.len() {
            1 => Ok(&all[0]),
            _ => Err(NotOne::Which((0..all.len()).collect())),
        };
    };
    let want = name.to_lowercase();
    if want.len() < SHORTEST_ID {
        return Err(NotOne::Unknown(name.to_string()));
    }
    let whole: Vec<usize> = (0..all.len())
        .filter(|i| id(&all[*i]).to_lowercase() == want)
        .collect();
    let ix = match whole.is_empty() {
        false => whole,
        true => (0..all.len())
            .filter(|i| names(&id(&all[*i]), &want))
            .collect(),
    };
    match ix.as_slice() {
        [] => Err(NotOne::Unknown(name.to_string())),
        [i] => Ok(&all[*i]),
        many => Err(NotOne::Which(many.to_vec())),
    }
}

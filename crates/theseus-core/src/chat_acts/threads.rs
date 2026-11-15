//! The two thread acts (issue 13, the owner's ruling of 2026-11-14 11:37: an
//! act on what a person says is a tool the model calls or a slash command,
//! never a match of the message's words): seal this conversation's thread, and
//! move the message the router placed wrongly to the thread it was for.
//!
//! They replace `seal_by_word` and `moved_by_words`, which read the message
//! before any turn ran. Each is a tool of the chat family, in no place's kept
//! catalog: [`Board::offers`] puts it in only where the act is possible, with
//! the thread or the choices named in its description.
//!
//! - **`chat.seal_thread`** is offered in the session of a thread that is not
//!   sealed. It seals that thread through `thread.seal`, judged as any thread
//!   act is, as the owner who spoke, and says what had crossed.
//! - **`chat.move_to_thread`** is offered where the router placed a message
//!   (before this one) and another live thread, or home, exists. The model
//!   cannot be allowed to move at once: the move tells the wrong thread to
//!   disregard the message and stops a turn running there, which can be the
//!   turn making the call. So the call stages the move (the placed message's
//!   turn is fixed at the offer, never the correcting message) and
//!   `Core::carry_out_move` runs it when the turn ends.
//!
//! The slash commands are `/thread seal` and `/move` in Discord, and
//! `theseus thread seal` and `theseus thread move`: the same verbs, at once.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_protocol::threads::{crossed_words, ThreadSealParams};
use theseus_protocol::Plan;
use theseus_tools::{parse, Backend, Retry, Tool, ToolClass, ToolCtx};

use super::{Board, MOVE, SEAL};
use crate::approval::Answerer;
use crate::toolrun::TurnCtx;
use crate::Core;

type Done = Result<(String, Value), String>;

/// The thread a seal would seal: this conversation's own, not sealed yet.
#[derive(Debug, Clone)]
pub(crate) struct Sealable {
    pub slug: String,
    pub title: String,
}

/// A message the router placed that could go elsewhere: its turn, the place's
/// own session (what `thread.move` finds it by), where it ran, and where it
/// could go as `(slug or "home", title)`.
#[derive(Debug, Clone)]
pub(crate) struct Movable {
    pub turn: String,
    pub home: String,
    pub from: String,
    pub targets: Vec<(String, String)>,
}

/// A move the model asked for, to run when its turn ends.
#[derive(Debug, Clone)]
pub struct StagedMove {
    pub turn: String,
    pub home: String,
    pub to: String,
    pub who: Answerer,
}

/// The moves staged by session, until the turn that staged them ends.
#[derive(Default)]
pub struct Staged {
    by_session: Mutex<HashMap<String, StagedMove>>,
}

impl Staged {
    fn put(&self, session: &str, m: StagedMove) {
        self.by_session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(session.to_string(), m);
    }

    fn take(&self, session: &str) -> Option<StagedMove> {
        self.by_session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(session)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SealArgs {
    /// The thread's slug, as the description names it: the conversation's own.
    thread: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveArgs {
    to: String,
}

/// `chat.seal_thread`: the thread's slug, which must be the conversation's own
/// (the description names it), so a string of a message's or a model's never
/// picks another thread to seal.
pub struct SealThread;

impl Tool for SealThread {
    fn name(&self) -> &'static str {
        SEAL
    }
    fn description(&self) -> &'static str {
        "Seal the thread of this conversation, when the operator asks for it to be sealed. \
         A sealed thread's notes never come into another thread, and another thread reading \
         it asks the operator first. Name the thread by its slug."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"thread": {"type": "string",
                "description": "The slug of this conversation's thread, as the description names it."}},
            "required": ["thread"],
            "additionalProperties": false,
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Harness
    }
    fn retry(&self) -> Retry {
        // A second call finds the thread sealed and says so.
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a: SealArgs = parse(input)?;
        let slug = a.thread.trim();
        if slug == "home" || !crate::fact::thread_route::is_target(slug) {
            return Err(format!("{slug:?} is not a thread's slug"));
        }
        Ok(Plan {
            summary: format!("seal the thread {slug}"),
            ..Default::default()
        })
    }
}

/// `chat.move_to_thread`: where the message the router placed should have
/// gone, a thread's slug or `home`.
pub struct MoveToThread;

impl Tool for MoveToThread {
    fn name(&self) -> &'static str {
        MOVE
    }
    fn description(&self) -> &'static str {
        "Move the message the router placed in the wrong thread to the thread the operator \
         says it was for, or to home. It is run again there when this turn ends."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"to": {"type": "string",
                "description": "A thread's slug from the list in this tool's description, or `home`."}},
            "required": ["to"],
            "additionalProperties": false,
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Harness
    }
    fn retry(&self) -> Retry {
        // A second call replaces the staged move with the same one.
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a: MoveArgs = parse(input)?;
        if !crate::fact::thread_route::is_target(a.to.trim()) {
            return Err(format!(
                "{:?} is neither a thread's slug nor `home`",
                a.to.trim()
            ));
        }
        Ok(Plan {
            summary: format!("move the placed message to {}", a.to.trim()),
            ..Default::default()
        })
    }
}

/// The seal tool's description for this turn: the thread it would seal.
pub(crate) fn seal_description(s: &Sealable) -> String {
    format!(
        "Seal the thread of this conversation, «{}» ({}), when the operator asks for it to be \
         sealed. A sealed thread's notes never come into another thread, and another thread \
         reading it asks the operator first. Pass `{}` as the thread. Call it only when they \
         ask: do not seal a thread on a message that merely mentions sealing.",
        s.title, s.slug, s.slug
    )
}

/// The move tool's description for this turn: where the message can go.
pub(crate) fn move_description(m: &Movable) -> String {
    let list: String = m
        .targets
        .iter()
        .map(|(slug, title)| format!("\n- `{slug}`: {title}"))
        .collect();
    format!(
        "Move the message the router placed in {} (the one before this) to the thread the \
         operator says it was for, when they say it went to the wrong one. It is run again \
         there when this turn ends, so answer them in a line saying it is moving. Call it \
         only when they say so: a message that merely names a thread does not ask for a \
         move. It can go to:{list}",
        match m.from.as_str() {
            "home" => "home".to_string(),
            slug => format!("`{slug}`"),
        }
    )
}

impl Board {
    /// The thread of `session`, when it has one that is not sealed.
    pub(crate) fn seal_offer(core: &Core, session: &str) -> Option<Sealable> {
        let board = core.threads();
        let t = board.of_session(session)?;
        t.sealed.is_none().then(|| Sealable {
            slug: t.slug.clone(),
            title: t.title.clone(),
        })
    }

    /// The message the router placed before `turn` in the place whose own
    /// session is `home`, with where it could go;
    /// none when threads are off, nothing was placed, or there is nowhere
    /// else to put it.
    pub(crate) fn move_offer(core: &Core, home: &str, turn: &str) -> Option<Movable> {
        if !core.cfg.threads.enabled {
            return None;
        }
        let routed = core.runner.threads.route.find_routed_before(home, turn)?;
        let from = routed.to.clone().unwrap_or_else(|| "home".into());
        let board = core.threads();
        let mut targets: Vec<(String, String)> = board
            .live(core.runner.class_of(home))
            .into_iter()
            .map(|t| (t.slug.clone(), t.title.clone()))
            .collect();
        targets.push(("home".into(), "home".into()));
        targets.retain(|(slug, _)| *slug != from);
        (!targets.is_empty()).then(|| Movable {
            turn: routed.turn,
            home: routed.home,
            from,
            targets,
        })
    }

    /// Seal the conversation's thread, as the owner who spoke.
    pub(crate) fn seal_thread(&self, tc: &TurnCtx<'_>, input: &Value) -> Done {
        let (core, offered) = self.standing(tc)?;
        let a: SealArgs = parse(input)?;
        let s = offered
            .sealable
            .as_ref()
            .ok_or("There is no unsealed thread of this conversation to seal. Nothing was done.")?;
        if a.thread.trim() != s.slug {
            return Err(format!(
                "`{}` is not this conversation's thread: it is `{}`. Nothing was done.",
                a.thread.trim(),
                s.slug
            ));
        }
        let params = ThreadSealParams {
            slug: s.slug.clone(),
            reason: Some("sealed by the owner, in the thread".into()),
            ..Default::default()
        };
        let r = core
            .thread_seal(&params, offered.said.who.clone())
            .map_err(|e| format!("the thread was not sealed: {}", e.message))?;
        let mut note = format!(
            "The owner sealed this thread ({}): its notes never come into another thread, and \
             another thread reading it asks the owner first.",
            s.slug
        );
        for w in crossed_words(&r.crossed) {
            note.push_str(&format!(" {w}."));
        }
        Ok((note, json!({"chat_act": {"sealed": s.slug}})))
    }

    /// Stage the move of the placed message; it runs when this turn ends.
    pub(crate) fn move_to_thread(&self, tc: &TurnCtx<'_>, input: &Value) -> Done {
        let (_core, offered) = self.standing(tc)?;
        let a: MoveArgs = parse(input)?;
        let m = offered
            .movable
            .as_ref()
            .ok_or("No message was placed in a thread that could be moved. Nothing was moved.")?;
        let to = a.to.trim();
        let Some((_, title)) = m.targets.iter().find(|(slug, _)| slug == to) else {
            let list: String = m
                .targets
                .iter()
                .map(|(slug, _)| format!("\n- `{slug}`"))
                .collect();
            return Err(format!(
                "`{to}` is not a place the message can go. Nothing was moved. It can go to:{list}"
            ));
        };
        self.staged.put(
            tc.session_id,
            StagedMove {
                turn: m.turn.clone(),
                home: m.home.clone(),
                to: to.to_string(),
                who: offered.said.who.clone(),
            },
        );
        Ok((
            format!(
                "It will move to {title} when this turn ends, and be run there as it was first \
                 said. Tell the operator in a line that it is moving."
            ),
            json!({"chat_act": {"move": to, "turn": m.turn}}),
        ))
    }

    /// The move this turn staged, taken once, for the end of the turn.
    pub fn take_move(&self, session: &str) -> Option<StagedMove> {
        self.staged.take(session)
    }
}

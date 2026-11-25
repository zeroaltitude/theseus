//! `self.file`, the tool (theseus-cux0o): the chat half of a person's own
//! filing, so handing the self loop a finding never means dropping to a
//! command line.
//!
//! The owner's standing principle, stated twice in five minutes on
//! 2026-11-12: minimise the cases where a person must drop to a command line
//! to perform a Theseus command. `theseus self file` alone would have been
//! exactly such a case, so the chat route is part of this row's bar and not a
//! follow-up. It calls [`crate::rpc::Core::self_file`] and nothing else: one
//! filing path, one mark, one frame, whichever surface asks.
//!
//! **What holds it, all of it already there:**
//!
//! - **Registered only while `[self] mode = "act"`**, exactly as
//!   `self.evidence` is ([`crate::rsi::evidence::register`]). With the loop
//!   off the tool is in no catalog, so the default config's tool set, its
//!   request goldens and its token censuses are byte for byte what they were.
//!   That is also the card's own shape: the filing path lands behind the
//!   existing self-improvement config gate, off by default.
//! - **It is no [`crate::places::public_tool`]**, so a shared place is never
//!   offered it and the gate refuses it there.
//! - **No [`theseus_protocol::Plan::authority`]** (issue 57: the owner wants
//!   approvals only for what is dangerous and cannot be undone): the call
//!   follows `self.file`'s own posture like any tool, and the owner is told
//!   once, with how to undo it (`self.drop`). He can still make it ask with
//!   `[policy.tools] "self.file" = "approve"`.
//! - **The owner's own place only**, the same wall `card.observed` keeps
//!   ([`crate::card::tool`]): a private place he is alone in
//!   ([`Core::owner_alone_in`]) and never a task's session. That is what
//!   makes the chat route *his* by construction rather than by a name a
//!   model could write: nobody else writes in such a place, so the filing
//!   can only be his, and an agent never has one -- an agent runs in a task
//!   or a job session, which this wall refuses, and files through the CLI or
//!   the RPC where `rpc/server.rs`'s `answerer` already marks it
//!   `agent:<name>`. So both halves of the owner's decision hold, with no
//!   mechanism of their own.
//!
//! **What is NOT built here, said plainly:** the *individual* behind a tap on
//! a shared surface is not carried into a tool's continuation (the gate's
//! answerer reaches `Core::confirm_action`, not the call's own run). This
//! route does not need it, because the wall above leaves only one person it
//! can be; a route that widened past an owner-alone place would need
//! `owner_ask`'s mechanism, where the answer is kept as a record the
//! continuation reads back.
//!
//! The core is reached through a weak handle given at build
//! ([`Board::attach`]), so the tool costs nothing until it is called.

use std::sync::{Arc, OnceLock, Weak};

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_protocol::Plan;
use theseus_tools::{
    AsyncMediaRun, Backend, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

use crate::owner_note::told::Undo;
use crate::rpc::Core;

pub const FILE: &str = "self.file";

/// Register `self.file`, only while `[self] mode = "act"`: the same rule
/// `self.evidence` is registered under, so no default catalog moves.
pub fn register(cfg: &crate::Config, r: &mut theseus_tools::Registry) {
    super::open_work::register(cfg, r);
    if cfg.self_improve.mode == theseus_protocol::rsi::SelfMode::Act {
        r.register(Arc::new(SelfFile));
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    finding: String,
    #[serde(default)]
    title: Option<String>,
}

impl Args {
    fn parse(input: &Value) -> Result<Self, String> {
        let a: Self = serde_json::from_value(input.clone())
            .map_err(|e| format!("self.file takes {{ finding, title? }}: {e}"))?;
        // The same reading the CLI and the RPC get, so a refusal reads the
        // same words on every surface.
        super::filed::finding_of(&a.finding)?;
        Ok(a)
    }
}

pub struct SelfFile;

impl Tool for SelfFile {
    fn name(&self) -> &str {
        FILE
    }

    fn description(&self) -> &str {
        "Put a finding on Theseus's own self-improvement backlog, so the self loop works on it. \
         `finding` is what is wrong, in the words of the person who found it; `title` is the one \
         line the backlog lists it by. The item is marked for good as a person's choice, with who \
         filed it, and it is never confused with one Theseus mined from its own record. The owner \
         is told of each filing, with how to drop it, and with self-improvement off the item sits \
         and nothing runs. Use it when a person names a defect in Theseus itself; use \
         self.propose_change to offer a fix for an item that is already on the backlog."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "finding": {
                    "type": "string",
                    "description": "What is wrong, in the finder's own words."
                },
                "title": {
                    "type": "string",
                    "description": "The one line the backlog lists it by; with none, the finding's first sentence."
                }
            },
            "required": ["finding"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        // It writes: one item onto the backlog.
        ToolClass::Write
    }

    fn backend(&self) -> Backend {
        // Dispatched by name in `toolrun/inproc.rs`, which only an async tool reaches.
        Backend::Async
    }

    fn retry(&self) -> Retry {
        // Two filings of one finding are two items by design (the cause key
        // is the item's own), so a repeat must never be made silently.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a = Args::parse(input)?;
        let title = super::filed::title_of(&a.finding, a.title.as_deref());
        // No `authority`: the call follows the tool's own posture, and the
        // owner is told of the filing with its undo (issue 57).
        Ok(Plan {
            summary: format!("file a self-improvement item: {title}"),
            ..Default::default()
        })
    }
}

/// The core behind the tool, held weakly and given at build.
#[derive(Default)]
pub struct Board {
    core: OnceLock<Weak<Core>>,
}

impl Board {
    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    /// The call, in its own task: the filing writes a frame, so it goes
    /// through [`theseus_store::blocking`] rather than holding a runtime
    /// worker across the disk.
    pub fn run(&self, tc: &crate::toolrun::TurnCtx<'_>, input: &Value) -> AsyncMediaRun {
        let fail = |why: String| -> AsyncMediaRun {
            Box::pin(std::future::ready(Err(ToolFailure::new(why))))
        };
        let Some(core) = self.core.get().and_then(Weak::upgrade) else {
            return fail("self.file is not ready: the core is not built".into());
        };
        let a = match Args::parse(input) {
            Ok(a) => a,
            Err(e) => return fail(e),
        };
        let private = tc.class == crate::places::PlaceClass::Private;
        let session = tc.session_id.to_string();
        Box::pin(async move {
            let out = theseus_store::blocking(move || filed(&core, private, &session, a))
                .map_err(ToolFailure::new)?;
            Ok((out, None, None))
        })
    }
}

/// The wall, then the filing. One function so a test reaches both past the
/// gate, as `card::tool::call` does.
fn filed(core: &Core, private: bool, session: &str, a: Args) -> Result<ToolOutput, String> {
    if let Some(why) = refusal(core, private, session) {
        return Err(why.to_string());
    }
    let f = core
        .self_file_in_chat(&a.finding, a.title.as_deref(), OWNER)
        .map_err(|e| format!("the filing could not be written: {e:#}"))?;
    // The finding's words are the model's, so the note names the item, not them.
    told(
        core,
        session,
        "A finding was filed for the self loop",
        "Theseus put a finding on its own self-improvement backlog from this chat",
        &format!(
            "Item {} is on the backlog at rank {}. The self loop picks it up on its next pass.",
            f.item, f.rank
        ),
        &Undo::SelfDrop(f.item.clone()),
    );
    Ok(ToolOutput {
        text: format!(
            "Filed {} ({}) on the self-improvement backlog at rank {}, by {}. It is marked a person's choice for good, and the loop picks it up on its next pass. `theseus self move {} 1` puts it first.",
            f.item, f.title, f.rank, f.filed_by, f.item
        ),
        meta: json!({"item": f.item, "cause": f.cause, "rank": f.rank}),
    })
}

/// The note's second line: which conversation did it, and what it did.
fn by(core: &Core, session: &str, did: &str) -> String {
    let rec: Option<crate::session::SessionRecord> = core.store.get_session(session).ok().flatten();
    let named = rec.and_then(|r| r.label.filter(|l| !l.is_empty()).or(r.title));
    let here = crate::judge::onto::words::in_conversation(named.as_deref());
    format!("{here}, {did}")
}

/// The owner told once of what a chat tool did on his behalf (issue 57), with
/// its undo: the shared note (`owner_note::told`), whose Undo is `undo`'s own
/// method as the presser. `did` and `body` are the tool's own words: no word
/// the model wrote, and no secret. A note that cannot be posted is logged;
/// the act stands.
pub(crate) fn told(core: &Core, session: &str, title: &str, did: &str, body: &str, undo: &Undo) {
    let by = by(core, session, did);
    let staged = crate::owner_note::told::stage(&core.outbox, Some(session), title, &by, body, undo)
        .and_then(|(post, records)| {
            core.store.append(&records)?;
            core.outbox.posted(&post);
            Ok(())
        });
    if let Err(e) = staged {
        tracing::warn!(session, error = %format!("{e:#}"), "self: the owner's note was not posted");
    }
}

/// The owner told once of what a chat tool did on his behalf that has no undo
/// (issue 57: a pause, which only does less): an FYI note to the operator,
/// where announcements go when a place for them is bound. `did` and `body`
/// are the tool's own words. A note that cannot be posted is logged.
pub(crate) fn tell(core: &Core, session: &str, title: &str, did: &str, body: &str) {
    let text = crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        title,
        &by(core, session, did),
        body,
    );
    let posted = core.outbox.to_operator(
        Some(session),
        json!({"kind": "notice", "text": text, crate::outbox::ANNOUNCE_KEY: true}),
    );
    if let Err(e) = posted {
        tracing::warn!(session, error = %format!("{e:#}"), "self: the owner's note was not posted");
    }
}

/// Who a chat filing is by. It is a constant and not a name the model wrote:
/// [`refusal`] leaves only one person the filing can be.
pub const OWNER: &str = "the owner";

const NOT_ALONE: &str = "self.file files on the owner's behalf, so it is only for a place \
                         he is alone in: no shared place, and no private place anyone else \
                         writes in. `theseus self file` files one from a shell.";
const NOT_THIS_SESSION: &str = "self.file is not for a task's session: a task cannot file \
                                on the owner's behalf. Report the finding, and he files it.";

/// Why a session may not act for the owner in chat.
pub(crate) enum Wall {
    /// Not a private place the owner is alone in.
    NotAlone,
    /// A task's session.
    Task,
}

/// The whole wall, read from the turn: its place's class, and whether the
/// owner is alone in the session's place and it is not a task's. Shared by
/// every tool that acts as the owner in chat (`self.file`, `self.pause`).
pub(crate) fn wall(core: &Core, private: bool, session: &str) -> Option<Wall> {
    if !private {
        return Some(Wall::NotAlone);
    }
    // A task's session: read as a task when the record cannot be read, so a
    // failed read refuses rather than files.
    let task = match core
        .store
        .get_session::<crate::session::SessionRecord>(session)
    {
        Ok(Some(s)) => s.task.is_some(),
        _ => true,
    };
    if task {
        return Some(Wall::Task);
    }
    if !core.owner_alone_in(session) {
        return Some(Wall::NotAlone);
    }
    None
}

/// `self.file`'s refusal, in its own words.
fn refusal(core: &Core, private: bool, session: &str) -> Option<&'static str> {
    wall(core, private, session).map(|w| match w {
        Wall::NotAlone => NOT_ALONE,
        Wall::Task => NOT_THIS_SESSION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schema_is_closed_and_asks_only_for_a_finding() {
        let s = SelfFile.input_schema();
        assert_eq!(s["additionalProperties"], json!(false));
        assert_eq!(s["required"], json!(["finding"]));
        assert!(s["properties"]["title"].is_object());
    }

    #[test]
    fn bad_input_is_refused_with_the_same_words_the_cli_gives() {
        let e = Args::parse(&json!({})).unwrap_err();
        assert!(e.contains("self.file takes"), "{e}");
        let e = Args::parse(&json!({"finding": "  "})).unwrap_err();
        assert!(e.contains("your own words"), "{e}");
        let e = Args::parse(&json!({"finding": "x".repeat(4_001)})).unwrap_err();
        assert!(e.contains("4001"), "{e}");
        // And a good one parses, so the refusals above are not everything.
        let a = Args::parse(&json!({"finding": "the gate runs clippy twice"})).unwrap();
        assert_eq!(a.finding, "the gate runs clippy twice");
        assert_eq!(a.title, None);
    }

    /// The plan sets no `authority` (issue 57): the call follows its own
    /// posture, and its summary names the title.
    #[test]
    fn the_plan_follows_the_tools_posture_and_names_the_title() {
        let ctx = ToolCtx::default();
        let p = SelfFile
            .plan(
                &json!({"finding": "The digest folds two kinds. And more."}),
                &ctx,
            )
            .unwrap();
        assert_eq!(
            p.summary,
            "file a self-improvement item: The digest folds two kinds."
        );
        assert_eq!(p.authority, None, "no ask at every posture");
        // A given title is what both lines say.
        let p = SelfFile
            .plan(
                &json!({"finding": "x happens", "title": "x happens sometimes"}),
                &ctx,
            )
            .unwrap();
        assert!(p.summary.ends_with("x happens sometimes"), "{}", p.summary);
    }
}

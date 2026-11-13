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
//! - **[`theseus_protocol::Plan::authority`] is set**, so the gate asks at
//!   every posture, as the floor does, and the filing waits for the owner's
//!   own tap. That is what "filing validated as him" means on this route:
//!   the model can propose a filing, and only a person's yes writes one.
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

use crate::rpc::Core;

pub const FILE: &str = "self.file";
/// The dispatch family (`toolrun.rs`).
pub const FAMILY: &str = FILE;

/// Register `self.file`, only while `[self] mode = "act"`: the same rule
/// `self.evidence` is registered under, so no default catalog moves.
pub fn register(cfg: &crate::Config, r: &mut theseus_tools::Registry) {
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
         filed it, and it is never confused with one Theseus mined from its own record. It waits \
         for the owner's yes before it is filed, and with self-improvement off the item sits and \
         nothing runs. Use it when a person names a defect in Theseus itself; use \
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
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // Two filings of one finding are two items by design (the cause key
        // is the item's own), so a repeat must never be made silently.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a = Args::parse(input)?;
        let title = super::filed::title_of(&a.finding, a.title.as_deref());
        Ok(Plan {
            summary: format!("file a self-improvement item: {title}"),
            // The gate asks at every posture, so a person's own yes is what
            // puts work into the self loop -- never the model's word alone.
            authority: Some(format!(
                "put a finding on Theseus's own self-improvement backlog: {title}"
            )),
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
    Ok(ToolOutput {
        text: format!(
            "Filed {} ({}) on the self-improvement backlog at rank {}, by {}. It is marked a              person's choice for good, and the loop picks it up on its next pass. `theseus self              move {} 1` puts it first.",
            f.item, f.title, f.rank, f.filed_by, f.item
        ),
        meta: json!({"item": f.item, "cause": f.cause, "rank": f.rank}),
    })
}

/// Who a chat filing is by. It is a constant and not a name the model wrote:
/// [`refusal`] leaves only one person the filing can be.
pub const OWNER: &str = "the owner";

const NOT_ALONE: &str = "self.file files on the owner's behalf, so it is only for a place \
                         he is alone in: no shared place, and no private place anyone else \
                         writes in. `theseus self file` files one from a shell.";
const NOT_THIS_SESSION: &str = "self.file is not for a task's session: a task cannot file \
                                on the owner's behalf. Report the finding, and he files it.";

/// The whole wall, read from the turn: its place's class, and whether the
/// owner is alone in the session's place and it is not a task's.
fn refusal(core: &Core, private: bool, session: &str) -> Option<&'static str> {
    if !private {
        return Some(NOT_ALONE);
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
        return Some(NOT_THIS_SESSION);
    }
    if !core.owner_alone_in(session) {
        return Some(NOT_ALONE);
    }
    None
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

    #[test]
    fn the_plan_asks_at_every_posture_and_names_the_title() {
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
        let a = p.authority.expect("a filing waits for a person's own yes");
        assert!(a.contains("The digest folds two kinds."), "{a}");
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

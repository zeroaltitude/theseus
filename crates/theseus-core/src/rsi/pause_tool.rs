//! `self.pause`, the tool (issue 6, theseus-7glm6): the chat half of pausing the loop's new builds,
//! so a person who says "pause the self-improvement builds while I run the benchmarks" never has
//! to drop to a command line. It calls [`crate::rpc::Core::self_pause`] and nothing else, as
//! `theseus self pause` and the RPC method do.
//!
//! It is held exactly as `self.file` is (`rsi::backlog::tool`): registered only while `[self] mode =
//! "act"`; no public tool, so never offered in a shared place; no `Plan::authority` (issue 57), so
//! the call follows `self.pause`'s own posture and the owner is told once (an unpause with its undo,
//! a pause again); and the owner's own place only, never a task's session
//! (`backlog::tool::wall`), so the call can only be the owner's. A pause set from
//! chat is the same pause: a state of the running daemon, cleared by `unpause` or a restart.

use std::sync::{Arc, OnceLock, Weak};

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_protocol::Plan;
use theseus_tools::{
    AsyncMediaRun, Backend, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

use super::backlog::tool::{tell, wall, Wall};
use crate::rpc::Core;

pub const PAUSE: &str = "self.pause";

/// Who a chat pause is by: a constant, not a name the model wrote (the wall leaves only the owner).
const OWNER: &str = "the owner";

/// Register `self.pause`, only while `[self] mode = "act"`: the rule `self.file` is registered under.
pub fn register(cfg: &crate::Config, r: &mut theseus_tools::Registry) {
    if cfg.self_improve.mode == theseus_protocol::rsi::SelfMode::Act {
        r.register(Arc::new(SelfPause));
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct Args {
    pub(crate) action: String,
    #[serde(default)]
    pub(crate) why: Option<String>,
}

impl Args {
    pub(crate) fn parse(input: &Value) -> Result<Self, String> {
        let a: Self = serde_json::from_value(input.clone()).map_err(|e| {
            format!("self.pause takes {{ action: \"pause\" or \"unpause\", why? }}: {e}")
        })?;
        match a.action.as_str() {
            "pause" | "unpause" => Ok(a),
            other => Err(format!(
                "self.pause's action is \"pause\" or \"unpause\", not {other:?}"
            )),
        }
    }

    fn clears(&self) -> bool {
        self.action == "unpause"
    }
}

pub struct SelfPause;

impl Tool for SelfPause {
    fn name(&self) -> &str {
        PAUSE
    }

    fn description(&self) -> &str {
        "Pause or unpause the new builds Theseus starts to improve itself, short of its kill switch. \
         `action` is \"pause\" or \"unpause\"; `why` is a few words. While paused, no new self build \
         starts; builds already running finish, and reviews and joins go on. A pause is cleared by \
         unpausing or by a restart of the daemon. The owner is told of each change, and it is only \
         for the owner's own conversation. Use it when the owner asks for the loop to stop starting \
         builds for a while (a benchmark is running, say); use self.file for a defect in Theseus itself."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["pause", "unpause"],
                    "description": "pause new self builds, or let them start again."
                },
                "why": {
                    "type": "string",
                    "description": "Why, in a few words (for a pause)."
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        // It changes a state of the daemon.
        ToolClass::Write
    }

    fn backend(&self) -> Backend {
        // Dispatched by name in `toolrun/inproc.rs`, which only an async tool reaches.
        Backend::Async
    }

    fn retry(&self) -> Retry {
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a = Args::parse(input)?;
        let summary = match a.clears() {
            true => "let new self-improvement builds start again".to_string(),
            false => "pause new self-improvement builds".to_string(),
        };
        // No `authority`: the call follows the tool's own posture, and the owner is told (issue 57).
        Ok(Plan {
            summary,
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

    pub fn run(&self, tc: &crate::toolrun::TurnCtx<'_>, input: &Value) -> AsyncMediaRun {
        let fail = |why: String| -> AsyncMediaRun {
            Box::pin(std::future::ready(Err(ToolFailure::new(why))))
        };
        let Some(core) = self.core.get().and_then(Weak::upgrade) else {
            return fail("self.pause is not ready: the core is not built".into());
        };
        let a = match Args::parse(input) {
            Ok(a) => a,
            Err(e) => return fail(e),
        };
        let private = tc.class == crate::places::PlaceClass::Private;
        let session = tc.session_id.to_string();
        Box::pin(async move {
            let out = theseus_store::blocking(move || paused(&core, private, &session, &a))
                .map_err(ToolFailure::new)?;
            Ok((out, None, None))
        })
    }
}

const NOT_ALONE: &str = "self.pause acts for the owner, so it is only for a place he is alone in: \
                         no shared place, and no private place anyone else writes in. \
                         `theseus self pause` does it from a shell.";
const NOT_THIS_SESSION: &str = "self.pause is not for a task's session: a task cannot act for the \
                                owner. Say so in your report, and he pauses it.";

/// The wall, then the pause. One function so a test reaches both past the gate.
pub(crate) fn paused(
    core: &Core,
    private: bool,
    session: &str,
    a: &Args,
) -> Result<ToolOutput, String> {
    if let Some(w) = wall(core, private, session) {
        return Err(match w {
            Wall::NotAlone => NOT_ALONE,
            Wall::Task => NOT_THIS_SESSION,
        }
        .to_string());
    }
    let r = core.self_pause_apply(a.clears(), a.why.as_deref(), OWNER);
    // Told once, when it changed anything; the reason is the model's words, so it is not quoted.
    match (a.clears(), r.changed) {
        (false, true) => tell(
            core,
            session,
            "New self builds are paused",
            "Theseus paused its new self-improvement builds from this chat",
            "No new self build starts until they are unpaused or Theseus restarts. Builds already \
             running finish.",
        ),
        (true, true) => tell(
            core,
            session,
            "New self builds may start again",
            "Theseus unpaused its self-improvement builds from this chat",
            "The pause is cleared: the self loop may start new builds.\nUndo: `theseus self pause`",
        ),
        _ => {}
    }
    let text = match (a.clears(), r.changed, &r.words) {
        (false, true, Some(w)) => format!("New self builds are {w}. Unpause lets them start."),
        (false, false, Some(w)) => format!("Already {w}."),
        (true, true, _) => "Unpaused: new self builds may start again.".to_string(),
        (true, false, _) => "Nothing was paused.".to_string(),
        (false, _, None) => "The pause was not set.".to_string(),
    };
    Ok(ToolOutput {
        text,
        meta: json!({"paused": r.paused, "changed": r.changed}),
    })
}

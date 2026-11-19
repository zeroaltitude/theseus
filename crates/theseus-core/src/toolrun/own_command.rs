//! A shell command that is Theseus's own look, answered with the tool that does it (issue 28).
//!
//! Asked to look at itself, a turn ran `theseus health` in a shell although `look.theseus` was
//! there: what it learned came back as terminal text, the command was judged as a shell command,
//! and the model never learned the tool existed. The daemon answers such a command **before it
//! runs**, with the tool's name, in code and in no prompt: `theseus_protocol::own_command` is the
//! rule (what is answered, and the table of verbs), and this is where it is applied.
//!
//! Its walls, each a reason the command runs as it did:
//! - only `proc.run`, and only a line whose one program is `theseus` (see the rule's file); a
//!   command that is not Theseus's own is never refused here, and an act, a look with no tool,
//!   and a verb the table does not know run as they do;
//! - only where the answer is usable. The tools are the owner's own conversation's, behind
//!   `config.show`'s wall: a job, a task, the self-improvement loop, an MCP client's session
//!   and a shared place are not answered, since the tool is refused them and the command is the
//!   only look they have;
//! - only where the tool is registered (a daemon started from a config).
//!
//! It is not the floor and touches no part of `policy.rs`: a shell command that names the daemon's
//! program or the vault's is held as it was, and this runs before the gate is asked.

use anyhow::Result;
use serde_json::json;
use theseus_protocol::own_command::{answer_input, Answered};

use super::{CallOutcome, ResultNode, ToolRuntime, TurnCtx};
use crate::node::ResultStatus;
use crate::provider::ToolUse;

/// The tool a shell command is answered with, when it is: the rule's answer, where the tool is
/// registered and the wall lets this turn call it.
fn answer_for(rt: &ToolRuntime, tc: &TurnCtx<'_>, call: &ToolUse) -> Option<Answered> {
    let answered = answer_input(&call.input)?;
    rt.tool_by_wire(&theseus_tools::wire_name(answered.tool))?;
    crate::config_change::wall::refusal(tc)
        .is_none()
        .then_some(answered)
}

/// What a tool gives, in words that are true of it: the answer to a command is a look that covers it,
/// and says where it covers less.
fn gives(tool: &str, verb: &str) -> &'static str {
    match tool {
        crate::look::THESEUS if verb.starts_with("ontology") => {
            "the ontology only when the call names it: `{\"what\": [\"ontology\"]}`; a call with no \
             `what` gives profiles, providers, health and the build, not the ontology"
        }
        crate::look::WORK => {
            "what it shows of Theseus's work: the conversations running and the daemon's own jobs, \
             every conversation's tasks, what waits on the owner, projects, routines and what is \
             coming, this conversation's first"
        }
        _ => "the same look",
    }
}

impl ToolRuntime {
    /// A `proc.run` that is Theseus's own look, answered with the tool's name and not run; none for
    /// any other call.
    pub(super) fn answer_own_command(
        &self,
        tc: &TurnCtx<'_>,
        tool: &str,
        call: &ToolUse,
    ) -> Result<Option<CallOutcome>> {
        if tool != "proc.run" {
            return Ok(None);
        }
        let Some(a) = answer_for(self, tc, call) else {
            return Ok(None);
        };
        // The count of these over a week says whether the habit is fading: the verb, the tool and the
        // kind of place, no command text.
        let place = crate::tools_defer::family::PlaceKind::of(
            tc.task.is_some(),
            tc.outbox.target(tc.session_id).as_deref(),
            false,
        );
        tc.record(&crate::fact::tool::ShellAnswered {
            verb: &a.verb,
            tool: a.tool,
            place: place.as_str(),
        });
        let wire = theseus_tools::wire_name(a.tool);
        // The way to load a tool the request hides is the provider's search where it has one, and
        // `tools_find` where it has none: the other is not offered, so it is not named.
        let loads = match self.defer.mechanism(tc.session_id) {
            Some(crate::tools_defer::Mechanism::Search) => crate::tools_defer::SEARCH_TOOL,
            _ => "tools_find",
        };
        let text = format!(
            "Not run: this is `{wire}`: use it. `theseus {}` in a shell is terminal text; the \
             tool gives {} as a result, and `{loads}` loads it if it is not in your tools.",
            a.verb,
            gives(a.tool, &a.verb)
        );
        let r = ResultNode {
            meta: json!({"answered_with": a.tool, "command": format!("theseus {}", a.verb)}),
            ..ResultNode::new(&call.id, tool, ResultStatus::Declined, text)
        };
        Ok(Some(CallOutcome::Done {
            status: self.answer(tc, r)?,
        }))
    }
}

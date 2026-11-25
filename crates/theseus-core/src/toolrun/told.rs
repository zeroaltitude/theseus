//! Two acts that ran unasked and are told to the owner instead (issue 57; the owner, 2026-11-25:
//! the approve is for what is truly dangerous and cannot be undone, and most of the rest should go
//! to notifications):
//!
//! - **the owner's command that names `theseusd`**, moved to the sandbox
//!   (`rsi::own_tree::moved_for_owner`), where Theseus's store, settings file and socket are not
//!   in the view and every write is scratch;
//! - **a fetch of a literal loopback or LAN address** (`web::net::told_host`). A link-local
//!   address, the cloud metadata services among them, and any name, keep their card.
//!
//! Each is told once, when it runs and no one was asked: an FYI-headed note to the operator
//! (`owner_note`), naming the conversation by its short id and quoting none of the call's own
//! text, so no secret and no outside text. A call that waited for a yes is not told again: the
//! owner answered it. The tool's own `[policy.tools]` line still makes either ask.
//!
//! A shared place keeps its card for a told address ([`in_shared`]): the page would join a
//! conversation others can read, which is the shared place's own rule (`places::private_fetch`).

use serde_json::{json, Value};
use theseus_protocol::PlaceClass;
use theseus_tools::{Plan, Tool};

use super::{Gated, ToolRuntime, TurnCtx};
use crate::owner_note::{note, Kind};
use crate::policy::{Decision, Posture};

/// A told address in a shared place waits for approval as every private address did, with the
/// card's words unchanged: the policy's reason, then the shared place's clause. Read after every
/// layer that may lower a decision (a routine's grant among them), so none lowers it. Any other
/// decision is returned as it is.
pub(crate) fn in_shared(
    policy: &crate::policy::ToolPolicy,
    class: PlaceClass,
    name: &str,
    plan: &Plan,
    d: Decision,
) -> Decision {
    if class == PlaceClass::Private || d.posture == Posture::Approve {
        return d;
    }
    let Some(why) = policy.told_address(plan) else {
        return d;
    };
    let asks = Decision::new(
        Posture::Approve,
        format!(
            "{}: {name} — approve ({why}, and a private address waits for approval)",
            plan.summary
        ),
    );
    crate::places::private_fetch(class, plan, asks)
}

/// The note for a call that runs unasked and is told, or None for any other call.
pub(crate) fn words(
    rt: &ToolRuntime,
    session: &str,
    tool: &dyn Tool,
    input: &Value,
    g: &Gated,
) -> Option<String> {
    if g.decision.posture == Posture::Approve {
        return None;
    }
    let who = format!(
        "conversation `{}` ran it, and you were not asked",
        crate::task::short(session)
    );
    if g.class.l1()
        && crate::rsi::own_tree::moved_for_owner(rt, Some(session), tool, &g.plan, input)
    {
        return Some(note(
            Kind::Fyi,
            "A command that names theseusd ran in the sandbox",
            &who,
            "Theseus's store, settings file and socket are not in the sandbox, and what the \
             command wrote there is gone, so there is nothing to undo. To be asked first, set \
             `[policy.tools] \"proc.run\" = \"approve\"`.",
        ));
    }
    let why = rt.policy.told_address(&g.plan)?;
    Some(note(
        Kind::Fyi,
        "A fetch of an address on this machine or your network ran",
        &who,
        &format!(
            "{why}. A fetch only reads, so Theseus has nothing to undo; a service there that acts \
             on a request has acted. A link-local address, such as a cloud's metadata service, \
             still asks. To be asked first, set `[policy.tools] \"http.fetch\" = \"approve\"`."
        ),
    ))
}

impl ToolRuntime {
    /// Tell the owner of a call that runs unasked ([`words`]): one notice to the operator. A
    /// note that cannot be written is logged, and the call runs all the same.
    pub(super) fn tell_owner(&self, tc: &TurnCtx<'_>, tool: &dyn Tool, input: &Value, g: &Gated) {
        let Some(text) = words(self, tc.session_id, tool, input, g) else {
            return;
        };
        let body = json!({"kind": "notice", "text": text});
        if let Err(e) = tc.outbox.to_operator(Some(tc.session_id), body) {
            tracing::warn!(tool = tool.name(), error = %format!("{e:#}"), "the owner's note of a told call was not written");
        }
    }
}

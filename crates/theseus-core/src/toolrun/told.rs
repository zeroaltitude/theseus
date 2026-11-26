//! A fetch that runs unasked and is told to the owner instead (issue 57; the owner, 2026-11-25:
//! the approve is for what is truly dangerous and cannot be undone, and most of the rest should go
//! to notifications): a fetch of a literal loopback or LAN address (`web::net::told_host`).
//!
//! A told fetch is a GET, with no header of the caller's and no body: `http.fetch` takes neither.
//! A URL that carries credentials (a user name or a password before the host) is not told and
//! keeps its card, since the client would send them as an `Authorization` header. A link-local
//! address, the cloud metadata services among them, and `localhost` keep their card; a name that
//! resolves to a private address is refused by the checked resolver, as before. A service there
//! that acts on a GET may act, and that cannot be undone: that is the cost the owner accepted.
//!
//! It belongs to any private place, not to who started the turn: a turn a wake, a routine's run or
//! a task starts in a private place gets it as a turn the owner typed does. It is told once, as it
//! starts and before anyone knows how it ends, so the note says what is true then: that Theseus is
//! fetching without asking. An FYI-headed note to the operator (`owner_note`), naming the
//! conversation by its short id and quoting none of the call's own text (no URL path, query or
//! credentials), so no secret and no outside text. A call that waited for a yes is not told again:
//! the owner answered it. The tool's own `[policy.tools]` line still makes it ask.
//!
//! A shared place keeps its card for a told address ([`in_shared`]): the page would join a
//! conversation others can read, which is the shared place's own rule (`places::private_fetch`).

use serde_json::json;
use theseus_protocol::PlaceClass;
use theseus_tools::{Plan, Tool};

use super::{Gated, ToolRuntime, TurnCtx};
use crate::owner_note::{note, Kind};
use crate::policy::{Decision, Posture};

/// A told address in a shared place waits for approval as every private address did, and its card
/// says what it said: the private address's reason, which came before the tool's posture, then the
/// shared place's clause. So it is the same decision whatever the posture, an asking one too. Read
/// after every layer that may lower a decision (a routine's grant among them), so none lowers it.
/// Any other decision is returned as it is.
pub(crate) fn in_shared(
    policy: &crate::policy::ToolPolicy,
    class: PlaceClass,
    name: &str,
    plan: &Plan,
    d: Decision,
) -> Decision {
    if class == PlaceClass::Private {
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
pub(crate) fn words(rt: &ToolRuntime, session: &str, g: &Gated) -> Option<String> {
    if g.decision.posture == Posture::Approve {
        return None;
    }
    let why = rt.policy.told_address(&g.plan)?;
    Some(note(
        Kind::Fyi,
        "Theseus is fetching an address on this machine or your network",
        &format!(
            "conversation `{}` asked for it, and you were not asked",
            crate::task::short(session)
        ),
        &format!(
            "{why}. A fetch only reads, so Theseus will have nothing to undo, but a service there \
             that acts on a request may act on this one. A link-local address, such as a cloud's \
             metadata service, still asks. To be asked first, set \
             `[policy.tools] \"http.fetch\" = \"approve\"`."
        ),
    ))
}

impl ToolRuntime {
    /// Tell the owner of a call that runs unasked ([`words`]): one notice to the operator, staged
    /// as the call starts. A note that cannot be written is logged, and the call runs all the same.
    pub(super) fn tell_owner(&self, tc: &TurnCtx<'_>, tool: &dyn Tool, g: &Gated) {
        let Some(text) = words(self, tc.session_id, g) else {
            return;
        };
        let posted = tc
            .outbox
            .to_operator(Some(tc.session_id), json!({"kind": "notice", "text": text}));
        if let Err(e) = posted {
            tracing::warn!(tool = tool.name(), error = %format!("{e:#}"), "the owner's note of a told call was not written");
        }
    }
}

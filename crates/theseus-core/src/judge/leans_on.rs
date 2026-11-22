//! `leans_on.v1` (issue 44, the design's step 3b; `docs/design/release-card.md`):
//! the trigger, typed. For the owner's own message in a place where he is
//! NOT alone, *does it lean on something that was not said in this place?*
//!
//! - **Who is asked of.** Only a message the owner's mark proves is his
//!   ([`crate::exchange::OwnerMark::person`] is the owner), at a turn whose
//!   presence the gate made a private channel or shared ([`asked_of`]). Another
//!   person's message, an agent's process, a message with no mark, and his
//!   message where he is alone are never asked.
//! - **What it reads.** The message and the last lines of the turn's own
//!   session, as that place already shows them, and nothing else: no other
//!   place, no memory, no recall. The state is built in the spawned task.
//! - **Off the turn's path, in shadow.** [`JudgeService::at_leans_on`] returns
//!   at once; the session's read, the state, its blob, the reservation from
//!   the shadow budget, the call and the record are the spawned task's, as
//!   `memory.v1`'s are. Nothing acts on the answer: the pack is not in
//!   `WIRED`, so its line is shadow until the owner promotes it
//!   (`theseus packs promote leans_on.v1`), and the company turn is the same
//!   turn, byte for byte, whatever it answers. Its row is the sink's
//!   `judge.call`, scoped `judge:leans_on`.

use std::sync::{Arc, Weak};

use serde_json::json;
use theseus_judge::builders::{HereLine, LeansOnInput, HERE};
use theseus_judge::{Ask, DecisionPoint, Input, Judge, Outcome, Pack, Urgency};
use theseus_memory::gate::Present;

use super::{spend, JudgeService, Prepared, ScrubWith};
use crate::exchange::{OwnerMark, OWNER};
use crate::node::{Body, Node, Origin};

/// The pack.
pub const PACK: &str = "leans_on.v1";

/// Whether the trigger is asked of a message of `mark`, at a turn whose
/// presence is `present`: the owner's own message, where he is not alone.
pub fn asked_of(mark: &OwnerMark, present: Present) -> bool {
    let company = matches!(present, Present::PrivateChannel | Present::Shared);
    company && mark.person().as_deref() == Some(OWNER)
}

/// The pack's input: the message `node` of the session's `nodes` (oldest
/// first), and the lines said before it there, as the place shows them.
pub fn input(nodes: &[Node], node: &str) -> Option<LeansOnInput> {
    let at = nodes.iter().position(|n| n.id == node)?;
    let Body::UserMessage { text, .. } = &nodes[at].body else {
        return None;
    };
    let mut here: Vec<HereLine> = nodes[..at]
        .iter()
        .rev()
        .filter_map(line_of)
        .take(HERE)
        .collect();
    here.reverse();
    Some(LeansOnInput {
        message: text.clone(),
        here,
    })
}

/// One line the place shows: a person's message, or the assistant's reply's
/// text. Tool calls, results and notes are not lines of the conversation.
fn line_of(n: &Node) -> Option<HereLine> {
    match &n.body {
        Body::UserMessage { text, .. } if n.origin == Origin::Operator => Some(HereLine {
            who: "person".into(),
            text: text.clone(),
        }),
        Body::AssistantMessage { blocks, .. } => {
            let text = crate::provider::text_of(blocks);
            (!text.is_empty()).then(|| HereLine {
                who: "assistant".into(),
                text,
            })
        }
        _ => None,
    }
}

impl JudgeService {
    /// The trigger, in shadow, in a task of its own, for the message `node`
    /// that opened `turn_id` of `session`. Returns at once, whatever Jev does.
    pub fn at_leans_on(&self, session: &str, turn_id: &str, node: &str) {
        if !self.pack_on(PACK) {
            return;
        }
        let Some(pack) = theseus_judge::pack::by_name(PACK) else {
            return;
        };
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let at = (session.to_string(), turn_id.to_string(), node.to_string());
        rt.spawn(judge_leans_on(self.me.clone(), pack, at));
    }

    /// The blocking half: the session's read, the state, its blob, and the
    /// reservation.
    fn prepare_leans_on(&self, pack: Arc<Pack>, at: &(String, String, String)) -> Option<Prepared> {
        let (session, turn, node) = at;
        let nodes: Vec<Node> = self
            .store
            .session_nodes(session)
            .ok()?
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        let input = input(&nodes, node)?;
        let lines = input.here.len();
        let scrub = ScrubWith(self.scrubber.clone());
        let state = theseus_judge::prepare(&pack, &Input::LeansOn(input), &scrub).ok()?;
        let blob = self
            .store
            .blobs()
            .put(state.state.json.as_bytes())
            .map_err(|e| tracing::warn!(error = %e, "judge: the state's blob was not written; not judged"))
            .ok()?;
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let mut context = json!({"session": session, "turn": turn, "node": node, "lines": lines,
                                 "baseline": "none", "class": "leans_on", "on_path_ms": 0,
                                 "blob": blob});
        let mode = self.ask_mode(&pack.name(), &mut context);
        let ask = Ask::new(pack, &state, mode, context);
        let need = built
            .judge
            .inner()
            .reserve_micros(std::slice::from_ref(&ask))
            .unwrap_or(0);
        Some(Prepared { built, ask, need })
    }
}

/// One trigger's judgment, in its own task; the service is held only around
/// the blocking half, never across the call.
async fn judge_leans_on(me: Weak<JudgeService>, pack: Arc<Pack>, at: (String, String, String)) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let prepared = tokio::task::spawn_blocking(move || svc.prepare_leans_on(pack, &at))
        .await
        .ok()
        .flatten();
    let Some(Prepared { built, ask, need }) = prepared else {
        return;
    };
    if !super::reserve_between(&me, &today, need, spend::Class::of(ask.mode)).await {
        return;
    }
    let judgments = built
        .judge
        .judge(DecisionPoint {
            asks: vec![ask],
            urgency: Urgency::Shadow,
        })
        .await;
    let Some(svc) = me.upgrade() else { return };
    for j in &judgments {
        let (called, failed, unknown) = match &j.outcome {
            Outcome::Answered => (true, false, false),
            Outcome::Failed { usage_unknown, .. } => (true, true, *usage_unknown),
            Outcome::Skipped { .. } => (false, false, false),
        };
        let spent = j.cost_micros.unwrap_or(if unknown { need } else { 0 });
        svc.budget.settle(&today, need, spent, called, failed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::Peer;

    fn mark(in_person: bool, owner_handle: Option<bool>, peer: Peer) -> OwnerMark {
        OwnerMark {
            node: "n1".into(),
            in_person,
            owner_handle,
            peer,
            agent: None,
            surface: "discord".into(),
            place: "discord:channel:1".into(),
            arrived_session: "s".into(),
            ran_session: "s".into(),
            author: Some("discord:42".into()),
            at_ms: 0,
        }
    }

    /// The owner's message in a channel or a shared place is asked of; his
    /// message where he is alone, another person's, an agent's, and any
    /// message where nobody's presence is known, are not.
    #[test]
    fn only_the_owners_message_in_company_is_asked_of() {
        let owner = mark(false, Some(true), Peer::Transport);
        assert!(asked_of(&owner, Present::PrivateChannel));
        assert!(asked_of(&owner, Present::Shared));
        assert!(!asked_of(&owner, Present::OwnerAlone));
        assert!(!asked_of(&owner, Present::Unknown));
        let guest = mark(false, Some(false), Peer::Transport);
        assert!(!asked_of(&guest, Present::Shared));
        assert!(!asked_of(&guest, Present::PrivateChannel));
        let agent = mark(false, None, Peer::Agent);
        assert!(!asked_of(&agent, Present::PrivateChannel));
    }
}

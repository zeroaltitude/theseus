//! A message that arrives while the place's turn runs (theseus-qu1lc): it
//! goes to the daemon at once, not after the turn. The daemon's running turn
//! hears it, ends its waits, and takes it in at its next loop; if the turn
//! ends first, the daemon queues it as the next turn, as this binding did.
//! So the place keeps no queue of its own while the daemon steers
//! (`[kernel] steer_queued`, on by default): with the switch off the message
//! waits here for the turn to end and goes with the rest, as before.
//!
//! The steered request is not the place's turn in flight (`inflight` is the
//! turn that was running), so its answer, which is that turn's result, only
//! says if it failed.

use theseus_protocol::{TurnSubmitParams, TurnSubmitResult};

use super::{CallError, Inbound, LaneMsg, Pending, Place, PlaceMsg};

impl Place {
    /// Whether a message that comes while a turn runs goes to the daemon now.
    pub(super) fn steers(&self) -> bool {
        self.shared.core.cfg.kernel.steer_queued
    }

    /// Send `m` to the session now, beside the turn in flight.
    pub(super) fn steer(&mut self, m: Inbound) {
        let author = format!("discord:{}", m.author);
        let anchor = m.message;
        // What the running turn says from here on answers this message.
        let _ = self.lane.send(LaneMsg::Anchor(anchor.get()));
        let pending: Vec<Pending> = m.files.into_iter().collect();
        let input = m.text;
        let (rpc, tx, sid) = (
            self.shared.rpc.clone(),
            self.tx.clone(),
            self.session_id.clone(),
        );
        tokio::spawn(async move {
            let mut attachments = Vec::new();
            for p in pending {
                attachments.extend(p.wait().await);
            }
            let r = rpc
                .call::<_, TurnSubmitResult>(
                    theseus_protocol::method::TURN_SUBMIT,
                    TurnSubmitParams {
                        dir: None,
                        carried: false,
                        prompt: None,
                        session_id: Some(sid),
                        input,
                        profile: None,
                        provider: None,
                        model: None,
                        author: Some(author),
                        attachments,
                        reply_to: Some(anchor.to_string()),
                        opened_from: None,
                        thread: None,
                        speaking: None,
                    },
                )
                .await
                .map(|_| ());
            let _ = tx.send(PlaceMsg::Steered(r));
        });
    }

    /// A steered request ended: only a failure is said, once.
    pub(super) fn steered_done(&mut self, r: Result<(), CallError>) {
        if let Err(e) = r {
            if !self.saw_failure {
                self.say(&format!("⚠️ {}", e.message), None);
            }
        }
    }
}

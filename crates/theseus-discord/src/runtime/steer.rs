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
//! says if it failed. It is counted all the same ([`Steering`]): when the turn
//! ends first, the daemon runs the message as a turn of its own, and until
//! that request is answered the place is busy ([`Place::turning`]). A `/stop`
//! that comes while one is out ends it, and its answer says nothing more
//! (theseus-07m8b).

use theseus_protocol::{Attachment, TurnSubmitParams, TurnSubmitResult};
use twilight_model::id::{marker::MessageMarker, Id};

use super::{CallError, Inbound, LaneMsg, Pending, Place, PlaceMsg};

/// The place's steered requests since its last `/stop`: how many wait for
/// their answer, and how many stops have come, which tells an answer a stop
/// ended from any other.
#[derive(Default)]
pub(super) struct Steering {
    out: u32,
    stops: u32,
}

impl Steering {
    /// A `/stop` came: the requests out now end with it, and are not counted.
    pub(super) fn stopped(&mut self) {
        self.stops = self.stops.wrapping_add(1);
        self.out = 0;
    }
}

/// `turn.submit`'s params for a person's message on the place's session: its
/// first reply answers `anchor`, and a Discord reply keeps the turn it
/// replied to, so the router can address its thread (theseus-rha2). One shape
/// for a turn of the place's own and a steer: the steer built its own, and
/// left that turn out.
pub(super) fn submit_params(
    sid: String,
    author: String,
    input: String,
    attachments: Vec<Attachment>,
    anchor: Option<Id<MessageMarker>>,
    reply_turn: Option<String>,
) -> TurnSubmitParams {
    TurnSubmitParams {
        session_id: Some(sid),
        input,
        author: Some(author),
        attachments,
        reply_to: anchor.map(|a| a.to_string()),
        in_reply_to: reply_turn,
        ..Default::default()
    }
}

/// What a steer sends of `m`, but for its files, which are added when they
/// are fetched.
pub(super) fn steer_params(sid: &str, m: &Inbound) -> TurnSubmitParams {
    submit_params(
        sid.to_string(),
        format!("discord:{}", m.author),
        m.text.clone(),
        Vec::new(),
        Some(m.message),
        m.reply_turn.clone(),
    )
}

impl Place {
    /// Whether a message that comes while a turn runs goes to the daemon now.
    pub(super) fn steers(&self) -> bool {
        self.shared.core.cfg.kernel.steer_queued
    }

    /// Whether a request of the place's is out: its turn's submit, or a
    /// steered one that waits for its answer.
    pub(super) fn turning(&self) -> bool {
        self.inflight || self.steering.out > 0
    }

    /// Send `m` to the session now, beside the turn in flight.
    pub(super) fn steer(&mut self, m: Inbound) {
        // What the running turn says from here on answers this message.
        let _ = self.lane.send(LaneMsg::Anchor(m.message.get()));
        let mut params = steer_params(&self.session_id, &m);
        let pending: Vec<Pending> = m.files.into_iter().collect();
        self.steering.out += 1;
        let stops = self.steering.stops;
        let (rpc, tx) = (self.shared.rpc.clone(), self.tx.clone());
        tokio::spawn(async move {
            for p in pending {
                params.attachments.extend(p.wait().await);
            }
            let r = rpc
                .call::<_, TurnSubmitResult>(theseus_protocol::method::TURN_SUBMIT, params)
                .await
                .map(|_| ());
            let _ = tx.send(PlaceMsg::Steered(r, stops));
        });
    }

    /// A steered request ended: only a failure is said, once, and not one a
    /// `/stop` since it went ended (`stops` counts those at its sending).
    pub(super) fn steered_done(&mut self, r: Result<(), CallError>, stops: u32) {
        if stops != self.steering.stops {
            return;
        }
        self.steering.out = self.steering.out.saturating_sub(1);
        if let Err(e) = r {
            if !self.saw_failure {
                self.say(&format!("⚠️ {}", e.message), None);
            }
        }
    }
}

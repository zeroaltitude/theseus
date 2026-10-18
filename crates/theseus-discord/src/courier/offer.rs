//! A late task offer (theseus-ypnxt.5). A child of `courier`.
//!
//! The reply was written before the judge's batch landed, so it went out
//! without `➕ Make this a task`. The core's `offer` post (within two minutes
//! of the reply) names the turn and its brief: the lane keeps the brief, as the
//! reply's own post does, and edits the reply's last message once to add the
//! button beside its sources. The edit is an ordinary write, so it follows the
//! lane's pace and pings no one. Nothing is done when the lane no longer holds
//! the reply (a restart, or it left the held turns), the place is not private,
//! or the reply carries the button already.

use serde_json::Value;

use super::{Lane, Plan, Write};
use crate::policy::Event;
use crate::render::Buttons;

impl Lane {
    pub(super) fn offer_post(&mut self, body: &Value) -> Plan {
        let (Some(turn), Some(brief)) = (body["turn_id"].as_str(), body["brief"].as_str()) else {
            return Plan::nothing("an offer without a turn or a brief");
        };
        if !self.shared.is_private(&self.target) {
            return Plan::nothing("not a private place");
        }
        let offers = &self.shared.tasks.offers;
        let store = &self.shared.core.store;
        if offers.brief(store, turn).is_some() {
            return Plan::nothing("the reply carries the offer already");
        }
        let Some((key, session, sources)) = offers.reply_of(turn) else {
            return Plan::nothing("the reply is not held by this lane");
        };
        let (Some(&(channel, _)), Some(content)) = (self.msgs.get(&key), self.sent.get(&key))
        else {
            return Plan::nothing("the reply's last message is not held by this lane");
        };
        offers.keep(store, turn, brief);
        Plan {
            writes: vec![Write {
                key,
                channel,
                content: content.clone(),
                buttons: Buttons::Sources(session, sources, Some(turn.to_string())),
                reply_to: None,
                message: None,
                mentions: vec![],
                ping: self.pings(Event::ReplyPart),
            }],
            extra: serde_json::json!({"turn_id": turn}),
        }
    }
}

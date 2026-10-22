//! A reply that may keep an open run going (theseus-a1wvq.15): after a turn
//! writes its final reply in a session whose open run ends just before it, the
//! reply is queued for the data question (`judge/reply_data.rs`), off the
//! turn's path. Nothing else is asked, so a plain session pays one cheap test.

use super::*;
use crate::data::reply::ReplyAsk;

impl TurnRunner {
    /// Queue the turn's final reply, when it continues an open run: a China-origin
    /// profile is configured, the session is untainted, the reply carries text
    /// and no call, and the node before it is the end of the run (the stored
    /// one, else the one this turn's input was admitted with) which the turn
    /// has not itself already extended through the reply. A reply that names
    /// one of the operator's sensitive names is never asked: it ends the run.
    pub(super) fn reply_data_ask(&self, t: &Turn<'_>, session: &SessionRecord) {
        if self.cfg.routing.classes.trivial.cn_profiles.is_empty() {
            return;
        }
        if let Some(ask) = self.reply_to_ask(t, session) {
            self.judge.ask_reply_data(ask);
        }
    }

    fn reply_to_ask(&self, t: &Turn<'_>, session: &SessionRecord) -> Option<ReplyAsk> {
        let data = session.data.as_deref();
        if data.is_some_and(|d| d.taint.is_some()) {
            return None;
        }
        let nodes = t.tc.store.transcript(t.tc.session_id).ok()?;
        let [.., (_, before), (_, reply)] = nodes.as_slice() else {
            return None;
        };
        let Body::AssistantMessage { blocks, .. } = &reply.body else {
            return None;
        };
        if reply.turn_id.as_deref() != Some(t.tc.turn_id)
            || !crate::provider::tool_uses_in(blocks).is_empty()
        {
            return None;
        }
        let text = crate::provider::text_of(blocks);
        let expect = data.and_then(|d| d.run.clone());
        if text.trim().is_empty() || expect.as_ref().is_some_and(|r| r.through == reply.id) {
            return None;
        }
        let admitted = t.route.admitted.0.as_ref().ok();
        let base = expect
            .iter()
            .chain(admitted)
            .find(|r| r.through == before.id)?
            .clone();
        let names = self.cfg.routing.data.names(&self.cfg.privacy);
        if crate::judge::inbound::names_hit(&names, &text) {
            return None;
        }
        Some(ReplyAsk {
            session: t.tc.session_id.to_string(),
            node: reply.id.clone(),
            text,
            base,
            expect,
            names,
        })
    }
}

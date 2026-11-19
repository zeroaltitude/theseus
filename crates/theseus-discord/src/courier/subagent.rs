//! A subagent's answer in the place (issue 28): another profile's words, posted as Theseus (the
//! bot every reply comes from), with a footer that names the model that answered and says it is a
//! subagent's. A child of `courier`.
//!
//! It is an outbox post (`kind: "subagent"`), so it is delivered once, in order with the turn's
//! other posts: the turn staged it before its reply, and the lane sends a target's posts in the
//! order they were written. In a thread's turn spoken in the owner's DM it carries the thread's
//! tag, as the reply does.

use serde_json::{json, Value};
use theseus_protocol::SubagentAnswer;

use super::{render, Buttons, Event, Lane, Plan, SendErr, Write};

impl Lane {
    /// The post's writes: the answer's parts under the call's keys.
    pub(super) async fn subagent_post(&mut self, body: &Value) -> Result<Plan, SendErr> {
        let channel = self.place_channel().await?;
        let answer: SubagentAnswer = serde_json::from_value(body["answer"].clone())
            .map_err(|e| SendErr::refused(format!("a subagent post without its answer: {e}")))?;
        let turn_id = body["turn_id"].as_str().unwrap_or("").to_string();
        let mut parts = render::subagent_parts(&answer);
        if let Some((tag, _)) = self.thread_tag(body, &turn_id) {
            render::thread_tag(&mut parts, &tag);
        }
        let ping = self.pings(Event::ReplyPart);
        let writes = parts
            .into_iter()
            .map(|(key, content)| Write {
                key,
                channel,
                content,
                buttons: Buttons::Keep,
                reply_to: None,
                message: None,
                mentions: vec![],
                ping,
            })
            .collect();
        Ok(Plan {
            writes,
            extra: json!({"subagent": answer.call}),
        })
    }
}

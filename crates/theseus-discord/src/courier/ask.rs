//! The needs-you post (theseus-j7cje): an open question that waited past its
//! line in `[notify]`, in the owner's DM, which pings. A child of `courier`.
//!
//! - **Always pings** (`Event::NeedsYou` is in no ping category): it exists
//!   because nothing else reached the owner.
//! - **Options as buttons** for a question with options; an approval's post
//!   says how to answer, since its card is where the buttons live.
//! - **Settled, not deleted**: the core indexes it as the question's card, so
//!   when the question closes anywhere the core's settle edits it (its
//!   `line` is what the settle repeats), no ping.

use serde_json::{json, Value};

use super::{Lane, Plan, SendErr, Write};
use crate::policy::Event;
use crate::render::Buttons;

/// The buttons of a confirm card: an `owner_ask`'s options, a layer-1
/// change's Accept and Decline, Approve with a third button to trust a
/// session that read external text, else Approve and Decline.
pub(super) fn card_buttons(req: &theseus_protocol::ConfirmRequest, q: &str) -> Buttons {
    if req.tool == theseus_protocol::work::ask::OWNER_ASK {
        return Buttons::Ask(
            q.to_string(),
            theseus_protocol::work::ask::choices_of(&req.input),
        );
    }
    if req.change.is_some() {
        Buttons::Accept(q.to_string())
    } else if req.external_text.is_some() {
        Buttons::ConfirmTrust(q.to_string())
    } else {
        Buttons::Confirm(q.to_string())
    }
}

impl Lane {
    /// The post: to the owner's DM alone, never a shared place.
    pub(super) async fn needs_you_post(&mut self, body: &Value) -> Result<Plan, SendErr> {
        let (channel, place) = self.owner_dm().await?;
        // Several questions due together, in one post: a nudge, no buttons.
        if body["questions"].is_array() {
            return Ok(Plan {
                writes: vec![Write {
                    key: format!("note:{}", body["questions"][0].as_str().unwrap_or("")),
                    channel,
                    content: body["text"].as_str().unwrap_or("").to_string(),
                    buttons: Buttons::Keep,
                    reply_to: None,
                    message: None,
                    mentions: vec![],
                    ping: self.pings(Event::NeedsYou),
                }],
                extra: json!({"place": place, "settled": "a batch has no card"}),
            });
        }
        let q = body["question"].as_str().unwrap_or("").to_string();
        let choices: Vec<String> = body["choices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c.as_str().map(str::to_string))
            .collect();
        let buttons = match choices.is_empty() {
            true => Buttons::Keep,
            false => Buttons::Ask(q.clone(), choices),
        };
        let content = crate::render::needs_you(body);
        let line = body["prompt"].as_str().unwrap_or("").to_string();
        Ok(Plan {
            writes: vec![Write {
                key: format!("confirm:{q}"),
                channel,
                content,
                buttons,
                reply_to: None,
                message: None,
                mentions: vec![],
                ping: self.pings(Event::NeedsYou),
            }],
            extra: json!({"place": place, "question": q, "line": line}),
        })
    }
}

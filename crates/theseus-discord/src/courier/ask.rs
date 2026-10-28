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
use crate::runtime::reply::answer;

/// The buttons of a confirm card: an `owner_ask`'s options, a layer-1
/// change's Accept and Decline, else Approve and Decline.
pub(super) fn card_buttons(req: &theseus_protocol::ConfirmRequest, q: &str) -> Buttons {
    if req.tool == theseus_protocol::work::ask::OWNER_ASK {
        return Buttons::Ask(
            q.to_string(),
            theseus_protocol::work::ask::choices_of(&req.input),
        );
    }
    if req.change.is_some() {
        Buttons::Accept(q.to_string())
    } else if let Some(o) = &req.grant_offer {
        // "Approve + this command for the conversation" (theseus-drjs).
        Buttons::ConfirmGrant(q.to_string(), o.clone())
    } else {
        Buttons::Confirm(q.to_string())
    }
}

/// The line a card's settle repeats: a card in plain words repeats all of it
/// (who, what, risk, the model's reason), so an Expired card is still the
/// question it was (theseus-cudar); any other post repeats its prompt.
fn line_of(body: &Value) -> String {
    body["card"]["line"]
        .as_str()
        .filter(|l| !l.is_empty())
        .or(body["prompt"].as_str())
        .unwrap_or("")
        .to_string()
}

impl Lane {
    /// The post: to the owner's DM alone, never a shared place. A post with a
    /// `card` (`[notify] dm_cards`, theseus-suvpe) is a card of its own with
    /// Approve, Decline and Details (an ask's options and Details); a batch's
    /// header pings and its cards post silently. Without a `card`, the post
    /// of before: a nudge for a batch, options as buttons for an ask.
    pub(super) async fn needs_you_post(&mut self, body: &Value) -> Result<Plan, SendErr> {
        let (channel, place) = self.owner_dm().await?;
        let questions: Vec<String> = body["questions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|q| q.as_str().map(str::to_string))
            .collect();
        // Several questions due together: the header names them, in the cards' numbers.
        if !questions.is_empty() {
            let key = format!("note:{}", questions[0]);
            if body["header"] == true {
                let entries = questions.iter().enumerate().map(|(i, q)| answer::Entry {
                    question: q.clone(),
                    choices: body["choices"][i]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|c| c.as_str().map(str::to_string))
                        .collect(),
                });
                self.shared.replies.asked().plan(&key, entries.collect());
            }
            let extra = match body["header"] == true {
                true => json!({"place": place}),
                false => json!({"place": place, "settled": "a batch has no card"}),
            };
            return Ok(Plan {
                writes: vec![Write {
                    key,
                    channel,
                    content: body["text"].as_str().unwrap_or("").to_string(),
                    buttons: Buttons::Keep,
                    reply_to: None,
                    message: None,
                    mentions: vec![],
                    ping: self.pings(Event::NeedsYou),
                }],
                extra,
            });
        }
        let q = body["question"].as_str().unwrap_or("").to_string();
        let choices: Vec<String> = body["choices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c.as_str().map(str::to_string))
            .collect();
        let card = body["card"].is_object();
        let buttons = match (card, choices.is_empty()) {
            (true, _) => Buttons::Card(q.clone(), choices.clone()),
            (false, true) => Buttons::Keep,
            (false, false) => Buttons::Ask(q.clone(), choices.clone()),
        };
        let key = format!("confirm:{q}");
        if card {
            let entry = answer::Entry {
                question: q.clone(),
                choices,
            };
            self.shared.replies.asked().plan(&key, vec![entry]);
        }
        let content = crate::render::needs_you(body);
        let line = line_of(body);
        let ping = body["silent"] != true && self.pings(Event::NeedsYou);
        Ok(Plan {
            writes: vec![Write {
                key,
                channel,
                content,
                buttons,
                reply_to: None,
                message: None,
                mentions: vec![],
                ping,
            }],
            extra: json!({"place": place, "question": q, "line": line}),
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// The settle's line is the card's whole question, else the prompt.
    #[test]
    fn a_settled_card_repeats_the_whole_question() {
        let card = json!({"prompt": "write `out.txt`",
            "card": {"line": "write out.txt wanted to write `out.txt` · writes"}});
        assert_eq!(
            super::line_of(&card),
            "write out.txt wanted to write `out.txt` · writes"
        );
        let old = json!({"prompt": "Ship it?"});
        assert_eq!(super::line_of(&old), "Ship it?");
        assert_eq!(
            super::line_of(&json!({"prompt": "p", "card": {"line": ""}})),
            "p"
        );
    }
}

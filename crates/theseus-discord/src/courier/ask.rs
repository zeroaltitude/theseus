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
/// change's Accept and Decline, a decline alone when the daemon offers no yes
/// (a secret to keep takes a value, and no button carries one), else Approve and
/// Decline.
pub(crate) fn card_buttons(req: &theseus_protocol::ConfirmRequest, q: &str) -> Buttons {
    if req.tool == theseus_protocol::work::ask::OWNER_ASK {
        return Buttons::Ask(
            q.to_string(),
            theseus_protocol::work::ask::choices_of(&req.input),
        );
    }
    // A secret to keep, in the owner's direct messages: a decline and the form (issue 29).
    if crate::runtime::keep_form::has_form(&req.options) {
        return Buttons::KeepForm(q.to_string());
    }
    // A settings question the daemon offers no yes to takes a value no button carries.
    if req.tool == theseus_core::config_change::ACK
        && !req.options.is_empty()
        && !req
            .options
            .iter()
            .any(|o| theseus_protocol::answer::approves(&o.id))
    {
        return Buttons::Decline(q.to_string());
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
    /// The card as this place draws it. A question that needs the owner's second proof says,
    /// before the press, whether the place's earlier proof covers it or a code is asked. A secret
    /// to keep, in the owner's direct messages with `[vault] keep_by_discord` on, says the form
    /// exists and what it costs, and draws its button (issue 29).
    pub(super) fn prepare(
        &self,
        req: &mut theseus_protocol::ConfirmRequest,
        route: &crate::render::Route,
    ) {
        let place = self.press_place(route);
        if let (true, Some(place)) = (req.needs_proof, &place) {
            req.proof = Some(
                self.shared
                    .core
                    .proof_standing(place, req.needs_fresh_proof),
            );
        }
        let on = self.shared.core.cfg.vault.keep_by_discord;
        crate::runtime::keep_form::offer(req, place.as_deref() == Some("discord:dm"), on);
    }

    /// A settings question's needs-you post (`render::settings::needs_you`): the
    /// daemon's words whole, the earlier messages silent and plain, the last
    /// with the buttons; none for any other post.
    fn settings_post(&self, body: &Value, q: &str, channel: u64, place: &str) -> Option<Plan> {
        // A secret to keep, in the owner's direct messages with the setting on, draws the form (issue 29).
        let form = self
            .shared
            .core
            .cfg
            .vault
            .keep_by_discord
            .then(|| crate::runtime::keep_form::needs_you_body(body))
            .flatten();
        let body = form.as_ref().unwrap_or(body);
        let (before, last) = crate::render::settings::needs_you(body)?;
        let key = format!("confirm:{q}");
        self.shared.replies.asked().plan(
            &key,
            vec![answer::Entry {
                question: q.to_string(),
                choices: Vec::new(),
            }],
        );
        let mut writes: Vec<Write> = before
            .into_iter()
            .enumerate()
            .map(|(i, part)| Write {
                key: format!("{}{q}:{i}", crate::render::settings::PART_KEY),
                channel,
                content: part,
                buttons: Buttons::Keep,
                reply_to: None,
                message: None,
                mentions: vec![],
                ping: false,
            })
            .collect();
        // A question the daemon offers no yes to (a secret to keep) has a Decline alone.
        let no_yes = body["settings"]["no_yes"] == true;
        writes.push(Write {
            key,
            channel,
            content: last,
            buttons: match (no_yes, form.is_some()) {
                (true, true) => Buttons::KeepForm(q.to_string()),
                (true, false) => Buttons::Decline(q.to_string()),
                (false, _) => Buttons::Card(q.to_string(), Vec::new()),
            },
            reply_to: None,
            message: None,
            mentions: vec![],
            ping: body["silent"] != true && self.pings(Event::NeedsYou),
        });
        Some(Plan {
            writes,
            // Escaped as the card's settled line is: a `*` in a value is drawn.
            extra: json!({"place": place, "question": q, "line": crate::render::settings::md(&line_of(body))}),
        })
    }

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
        // A settings question's post: the daemon's words whole, as several
        // messages when longer than one, the buttons on the last.
        if let Some(plan) = self.settings_post(body, &q, channel, &place) {
            return Ok(plan);
        }
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

    /// Only a secret to keep gets a decline alone: an `owner_ask` keeps its choices and an
    /// ordinary approval keeps Approve and Decline, whatever their options are.
    #[test]
    fn only_a_secret_to_keep_is_drawn_with_a_decline_alone() {
        use theseus_protocol::work::QuestionKind;
        use theseus_protocol::ConfirmRequest;
        let mut asks = ConfirmRequest {
            tool: theseus_protocol::work::ask::OWNER_ASK.into(),
            input: json!({"options": [{"label": "Careful"}, {"label": "Ship it"}]}),
            ..Default::default()
        };
        asks.options = asks.question(QuestionKind::Ask, true).options;
        assert!(matches!(
            super::card_buttons(&asks, "act_1"),
            crate::render::Buttons::Ask(..)
        ));
        let mut call = ConfirmRequest {
            tool: "fs.write".into(),
            ..Default::default()
        };
        call.options = call.question(QuestionKind::Approval, true).options;
        assert_eq!(
            super::card_buttons(&call, "act_1"),
            crate::render::Buttons::Confirm("act_1".into())
        );
    }

    /// A secret to keep takes a value, which no button carries: the daemon offers
    /// only its decline, and the card says first where the value is typed. Drawn
    /// in each of Discord's forms (the card, the needs-you post, the V2 buttons),
    /// there is no Approve, and no sentence about one (issue 29).
    #[test]
    fn a_secret_to_keep_is_drawn_with_a_decline_alone_and_says_where_it_is_typed() {
        use theseus_core::config_change::vault_keep::target::{State, Target};
        use theseus_core::config_change::{vault_keep, words};
        use theseus_protocol::work::QuestionKind;
        use theseus_protocol::ConfirmRequest;

        let t = Target {
            vault: "Harbor".into(),
            vault_id: "vaultid00000000000000000001".into(),
            title: "Lantern new".into(),
            item_id: None,
            field: "credential".into(),
            field_id: None,
            state: State::New,
            protected: false,
        };
        let q = vault_keep::question(&t, false, false, "the lantern provider's key");
        let args = json!({"question": q, "settled": vault_keep::settled(&t), "keep": t.args()});
        let mut req = ConfirmRequest {
            correlation_id: "act_0000feed".into(),
            session_id: "ses_00000000harbour".into(),
            tool: "config.ack".into(),
            reason: q.clone(),
            plain: words::plain_of(&args),
            input: args,
            needs_proof: true,
            ..Default::default()
        };
        // What the daemon offers: the decline alone.
        req.options = req
            .question(QuestionKind::Approval, true)
            .options
            .into_iter()
            .filter(|o| o.id == "decline")
            .collect();
        assert_eq!(req.options.len(), 1);

        // The card: where the value is typed comes first, and nothing offers a yes.
        let card = crate::render::card(&req, &crate::render::Route::Here, "");
        assert!(
            card.content
                .starts_with("Type the value in your own terminal"),
            "{}",
            card.content
        );
        assert!(!card.content.contains("**Approve**"), "{}", card.content);
        assert!(!card.content.contains("opens a form"), "{}", card.content);
        assert!(!card.content.contains("goes through"), "{}", card.content);
        // Its buttons: a decline alone.
        assert_eq!(
            super::card_buttons(&req, "act_0000feed"),
            crate::render::Buttons::Decline("act_0000feed".into())
        );
        let text = format!("{:?}", crate::runtime::decline_buttons("act_0000feed"));
        assert!(text.contains("confirm:decline:act_0000feed"), "{text}");
        assert!(!text.contains("approve"), "{text}");
        // The needs-you post says the same.
        let body = json!({"settings": {"detail": q, "needs_proof": true, "short": "feed", "no_yes": true}});
        let (before, last) = crate::render::settings::needs_you(&body).unwrap();
        let all = format!("{}{last}", before.concat());
        assert!(all.starts_with("Type the value"), "{all}");
        assert!(all.contains("tap Decline"), "{all}");
        assert!(!all.contains("tap Approve"), "{all}");
    }
}

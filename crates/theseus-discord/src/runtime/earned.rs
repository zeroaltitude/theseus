//! Earned autonomy's card (theseus-uttu): the owner's DM gets the week's
//! candidates, each with **Yes, with a notice**, **Not this one** and
//! **Show the record**. A yes or a no is `policy.earned.answer` as the
//! presser, with the place it came from, so the core judges it as it judges
//! an answer: the owner, from a private place (a yes loosens). The record
//! comes back as a reply only the presser sees. A child of `runtime`, apart
//! from it for the shape budget's file ceiling.

use serde_json::Value;
use theseus_protocol::earned::{
    PolicyEarnedAnswerParams, PolicyEarnedAnswerResult, PolicyEarnedParams, PolicyEarnedResult,
};
use theseus_protocol::DiscordOrigin;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::http::interaction::InteractionResponseType;

use super::Shared;

/// A card button's custom id: `earned:<yes|no|show>:<group id>`.
const PREFIX: &str = "earned";

/// A button's label: Discord's cap is 80 characters.
fn label(words: &str, what: &str) -> String {
    let mut l = format!("{words}: {what}");
    if l.chars().count() > 80 {
        l = l.chars().take(79).collect::<String>() + "…";
    }
    l
}

/// A card's buttons: a row for each candidate (`(id, label)`), three buttons
/// in it.
pub(crate) fn buttons(candidates: &[(String, String)]) -> Vec<Component> {
    candidates
        .iter()
        .take(5)
        .map(|(id, what)| {
            let b = |verb: &str, words: &str, style: ButtonStyle| {
                Component::Button(Button {
                    id: None,
                    custom_id: Some(format!("{PREFIX}:{verb}:{id}")),
                    disabled: false,
                    emoji: None,
                    label: Some(label(words, what)),
                    style,
                    url: None,
                    sku_id: None,
                })
            };
            Component::ActionRow(ActionRow {
                id: None,
                components: vec![
                    b("yes", "Yes, with a notice", ButtonStyle::Success),
                    b("no", "Not this one", ButtonStyle::Secondary),
                    b("show", "Show the record", ButtonStyle::Primary),
                ],
            })
        })
        .collect()
}

/// A press: its verb and the group's id.
pub(crate) fn parse(custom_id: &str) -> Option<(String, String)> {
    let rest = custom_id.strip_prefix(PREFIX)?.strip_prefix(':')?;
    let (verb, id) = rest.split_once(':')?;
    (matches!(verb, "yes" | "no" | "show") && id.starts_with("ern_"))
        .then(|| (verb.to_string(), id.to_string()))
}

/// The card's candidates `(id, label)`, from the core's `earned_card` post.
pub(crate) fn candidates(body: &Value) -> Vec<(String, String)> {
    body["card"]["candidates"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some((
                c["id"].as_str()?.to_string(),
                c["label"].as_str()?.to_string(),
            ))
        })
        .collect()
}

impl Shared {
    /// A press on one of the owner's DM notices: Jev's label, or an earned
    /// card's button. Whether it was one of them.
    pub(super) async fn notice_press(
        &self,
        i: &Interaction,
        custom_id: &str,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) -> bool {
        if let Some(press) = super::jev::parse(custom_id) {
            self.jev_press(i, press, who, discord).await;
        } else if let Some(press) = parse(custom_id) {
            self.earned_press(i, press, who, discord).await;
        } else if let Some(press) = super::capture_card::parse(custom_id) {
            self.standing_press(i, press, who, discord).await;
        } else if let Some(press) = super::card_question::parse(custom_id) {
            self.card_question_press(i, press, who, discord).await;
        } else if let Some(press) = super::remind::parse(custom_id) {
            self.remind_press(i, press, who).await;
        } else if let Some(press) = super::seal::parse(custom_id) {
            self.seal_press(i, press, who, discord).await;
        } else if let Some(press) = super::seal::link::parse(custom_id) {
            self.link_press(i, press, who, discord).await;
        } else if let Some(write) = super::onto_card::parse(custom_id) {
            self.onto_press(i, write, who, discord).await;
        } else {
            return self.notice_press_more(i, custom_id, who, discord).await;
        }
        true
    }

    /// The rest of `notice_press`'s chain, apart from it for clippy's complexity limit.
    async fn notice_press_more(
        &self,
        i: &Interaction,
        custom_id: &str,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) -> bool {
        if let Some(press) = super::persona_soul::parse(custom_id) {
            self.persona_soul_press(i, press, who).await;
        } else if let Some(press) = super::content::parse(custom_id) {
            self.content_press(i, press, who, discord).await;
        } else if let Some(press) = super::self_note::parse(custom_id) {
            self.self_note_press(i, press, who).await;
        } else if let Some(press) = super::thread_ask::parse(custom_id) {
            self.thread_ask_press(i, press, who, discord).await;
        } else if let Some(press) = super::ask::parse(custom_id) {
            self.ask_press(i, press, who, discord).await;
        } else if let Some(q) = super::ask::parse_again(custom_id) {
            self.ask_again_press(i, q, who, discord).await;
        } else if let Some(q) = super::ask::parse_details(custom_id) {
            self.ask_details(i, &q, discord).await;
        } else {
            return false;
        }
        true
    }

    /// A card button: yes and no go to the core as the presser, from where
    /// they pressed; the record is read. The reply is the presser's alone.
    pub(super) async fn earned_press(
        &self,
        i: &Interaction,
        (verb, id): (String, String),
        who: &str,
        discord: Option<DiscordOrigin>,
    ) {
        self.respond(
            i,
            InteractionResponseType::DeferredChannelMessageWithSource,
            None,
            true,
        )
        .await;
        let text = match verb.as_str() {
            "show" => {
                let r = self
                    .rpc
                    .call::<_, PolicyEarnedResult>(
                        theseus_protocol::method::POLICY_EARNED,
                        PolicyEarnedParams {
                            show: Some(id.clone()),
                            brief: true,
                            ..Default::default()
                        },
                    )
                    .await;
                match r {
                    Ok(r) => r.shown.map_or_else(
                        || format!("{id}: no record."),
                        |g| {
                            let x = &g.record;
                            let mut t = format!(
                                "`{}`: {} approvals, {} session(s), never declined, no \"should have asked\", no floor.",
                                g.label, x.approvals, x.sessions
                            );
                            for s in &x.samples {
                                t.push_str(&format!("\ne.g. `{s}`"));
                            }
                            t
                        },
                    ),
                    Err(e) => format!("⚠️ The record was not read: {}", e.message),
                }
            }
            _ => {
                let accept = verb == "yes";
                let r = self
                    .rpc
                    .call::<_, PolicyEarnedAnswerResult>(
                        theseus_protocol::method::POLICY_EARNED_ANSWER,
                        PolicyEarnedAnswerParams {
                            id: id.clone(),
                            accept,
                            author: Some(who.to_string()),
                            discord,
                        },
                    )
                    .await;
                match r {
                    Ok(r) if r.accepted => format!(
                        "✅ `{}` runs with a notice from now on. `theseus policy revoke {id}` ends it.",
                        r.grant.map_or(id.clone(), |g| g.group.label)
                    ),
                    Ok(_) => format!("Not proposed again for 90 days: {id}."),
                    Err(e) => match e.code == theseus_protocol::error_code::REFUSED {
                        true => format!(
                            "🔐 Your answer did not count: {}.",
                            e.data.get("why").and_then(Value::as_str).unwrap_or(&e.message)
                        ),
                        false => format!("⚠️ The answer was not written: {}", e.message),
                    },
                }
            }
        };
        self.followup(i, &text).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A card's buttons: a row a candidate (five at most), three buttons in
    /// each, ids `earned:<verb>:<id>` that `parse` reads back, labels within
    /// Discord's 80 characters.
    #[test]
    fn a_card_has_three_buttons_a_candidate_and_each_press_parses() {
        let long = "cargo ".repeat(30);
        let c: Vec<(String, String)> = (0..7)
            .map(|i| {
                (
                    format!("ern_0000000{i}"),
                    if i == 0 {
                        long.clone()
                    } else {
                        format!("cmd {i}")
                    },
                )
            })
            .collect();
        let rows = buttons(&c);
        assert_eq!(rows.len(), 5, "five rows at most");
        for (n, row) in rows.iter().enumerate() {
            let Component::ActionRow(r) = row else {
                panic!("{row:?}")
            };
            let ids: Vec<String> = r
                .components
                .iter()
                .map(|b| match b {
                    Component::Button(b) => b.custom_id.clone().unwrap(),
                    other => panic!("{other:?}"),
                })
                .collect();
            let verbs: Vec<&str> = ids.iter().map(|i| i.split(':').nth(1).unwrap()).collect();
            assert_eq!(verbs, ["yes", "no", "show"]);
            for (id, verb) in ids.iter().zip(verbs) {
                assert_eq!(
                    parse(id),
                    Some((verb.to_string(), format!("ern_0000000{n}")))
                );
            }
            for b in &r.components {
                let Component::Button(b) = b else {
                    unreachable!()
                };
                assert!(b.label.as_ref().unwrap().chars().count() <= 80);
            }
        }
        assert_eq!(parse("earned:maybe:ern_00000001"), None);
        assert_eq!(parse("earned:yes:other"), None);
        assert_eq!(parse("jev:right:jdg_1"), None);
    }

    /// The card's candidates are read from the core's post, and a post
    /// without them has none.
    #[test]
    fn the_candidates_come_from_the_posts_card() {
        let body = json!({"kind": "earned_card", "text": "x",
            "card": {"candidates": [{"id": "ern_00000001", "label": "cargo fmt --all"},
                                    {"id": "ern_00000002", "label": "git status"}]}});
        assert_eq!(
            candidates(&body),
            [
                ("ern_00000001".to_string(), "cargo fmt --all".to_string()),
                ("ern_00000002".to_string(), "git status".to_string())
            ]
        );
        assert!(candidates(&json!({"kind": "earned_card"})).is_empty());
    }
}

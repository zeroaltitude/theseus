//! The one message a content label adds to the Discord flow (theseus-9ymz0,
//! design §6.3). When the core raises a label on a session it posts a
//! `content_label` message in that session's place, with one **Remove** button
//! (`content:remove:<session>:<flag>`). A press is `content.unlabel` as the
//! presser, with the place it came from, so the core judges it as it judges a
//! card's press or `/trust`: the owner, from a private place, and anyone
//! else's is refused and told alone. A counted press settles the message (its
//! button goes, and it says who removed the label); a refused one changes
//! nothing on it.
//!
//! **Nothing else in the DM task flow moves.** The owner locked that flow on
//! 2026-10-22 (`tests_dm_flow_golden.rs`); this is the one approved addition,
//! and the golden differs from before by this one message and nothing else.
//!
//! A child of `runtime`, apart from it for the shape budget's file ceiling.

use theseus_protocol::{method, ContentUnlabelParams, DiscordOrigin, UnlabelResult};
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::http::interaction::InteractionResponseType;

use super::Shared;

/// A button's custom id: `content:remove:<session>:<flag>`.
const PREFIX: &str = "content:remove:";

/// The flags' words, as the core takes them.
const FLAGS: [&str; 4] = ["injection", "secret", "confidential", "personal"];

/// A press of the Remove button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Press {
    pub session: String,
    pub flag: String,
}

/// A session id as a custom id may carry it.
fn session_ok(s: &str) -> bool {
    s.starts_with("ses_")
        && s.len() <= 48
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(crate) fn parse(custom_id: &str) -> Option<Press> {
    let rest = custom_id.strip_prefix(PREFIX)?;
    let (session, flag) = rest.rsplit_once(':')?;
    (session_ok(session) && FLAGS.contains(&flag)).then(|| Press {
        session: session.into(),
        flag: flag.into(),
    })
}

/// The message's one button.
pub(crate) fn buttons(session: &str, flag: &str) -> Vec<Component> {
    if !session_ok(session) || !FLAGS.contains(&flag) {
        return vec![];
    }
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: vec![Component::Button(Button {
            id: None,
            custom_id: Some(format!("{PREFIX}{session}:{flag}")),
            disabled: false,
            emoji: None,
            label: Some("Remove".into()),
            style: ButtonStyle::Secondary,
            url: None,
            sku_id: None,
        })],
    })]
}

/// The message as it settles after a counted press.
pub(crate) fn settled(text: &str, who: &str) -> String {
    format!("{text}\n✅ Removed by {who}.")
}

/// What the presser is told of a refusal or a failure.
fn failed(e: &crate::rpc_client::CallError) -> String {
    match e.code == theseus_protocol::error_code::REFUSED {
        true => format!(
            "🔐 Your removal did not count: {}. The label stays.",
            e.data
                .get("why")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&e.message)
        ),
        false => format!("⚠️ {}", e.message),
    }
}

impl Shared {
    /// A press of the Remove button: `content.unlabel` as the presser.
    pub(super) async fn content_press(
        &self,
        i: &Interaction,
        press: Press,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) {
        self.respond(
            i,
            InteractionResponseType::DeferredUpdateMessage,
            None,
            false,
        )
        .await;
        let r = self
            .rpc
            .call::<_, UnlabelResult>(
                method::CONTENT_UNLABEL,
                ContentUnlabelParams {
                    session_id: press.session,
                    flag: press.flag,
                    author: Some(who.to_string()),
                    discord,
                },
            )
            .await;
        match r {
            Ok(_) => {
                let text = i.message.as_ref().map(|m| settled(&m.content, who));
                let client = self.interact.interaction(i.application_id);
                let mut update = client.update_response(&i.token).components(Some(&[]));
                if let Some(t) = &text {
                    update = update.content(Some(t));
                }
                if let Err(e) = update.await {
                    self.board.error("content label", None, e);
                }
            }
            Err(e) => self.followup(i, &failed(&e)).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SES: &str = "ses_0123456789abcdef0123456789abcdef";

    /// The button's id parses back to its session and flag, and anything that
    /// is not the button is none.
    #[test]
    fn the_buttons_id_parses_back_and_a_stranger_is_none() {
        let id = format!("content:remove:{SES}:injection");
        assert_eq!(
            parse(&id),
            Some(Press {
                session: SES.into(),
                flag: "injection".into()
            })
        );
        for f in FLAGS {
            assert!(parse(&format!("{PREFIX}{SES}:{f}")).is_some(), "{f}");
        }
        for bad in [
            "content:remove:ses_1:open",
            "content:remove:tsk_1:injection",
            "content:remove:ses_1",
            "content:remove:",
            "content:keep:ses_1:secret",
            "confirm:approve:act_1",
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
        assert!(
            format!("{PREFIX}{SES}:confidential").len() <= 100,
            "within Discord's limit on a custom id"
        );
    }

    /// One button, labelled Remove, whose id is the press.
    #[test]
    fn the_message_has_one_button() {
        let c = buttons(SES, "secret");
        assert_eq!(c.len(), 1);
        let Component::ActionRow(row) = &c[0] else {
            panic!("a row")
        };
        assert_eq!(row.components.len(), 1);
        let Component::Button(b) = &row.components[0] else {
            panic!("a button")
        };
        assert_eq!(b.label.as_deref(), Some("Remove"));
        assert_eq!(
            b.custom_id.as_deref(),
            Some(format!("content:remove:{SES}:secret").as_str())
        );
        assert!(buttons("not-a-session", "secret").is_empty());
        assert!(buttons(SES, "open").is_empty());
    }
}

//! A proposed seal's card (theseus-f2g0; the owner, 2026-10-09: Jev
//! recommends, the owner seals): the owner's DM gets "🔒 This looks embargoed:
//! <reason>" with **Seal this thread** and **Not needed**. Each press is
//! `thread.seal` or `thread.seal_dismiss` as the presser, with the place it
//! came from, so the core judges it as it judges any thread act: the owner,
//! from a private place. A seal's answer says what had already crossed into
//! other threads. A child of `runtime`, apart from it for the shape budget's
//! file ceiling.

use serde_json::Value;
use theseus_protocol::threads::{
    crossed_words, ThreadNameParams, ThreadRow, ThreadSealParams, ThreadSealResult,
};
use theseus_protocol::DiscordOrigin;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::http::interaction::InteractionResponseType;

pub(crate) mod link;

use super::Shared;

/// A card button's custom id: `seal:<yes|no>:<slug>`.
const PREFIX: &str = "seal";

/// The card's buttons: Seal this thread, Not needed.
pub(crate) fn buttons(slug: &str) -> Vec<Component> {
    let b = |verb: &str, label: &str, style: ButtonStyle| {
        Component::Button(Button {
            id: None,
            custom_id: Some(format!("{PREFIX}:{verb}:{slug}")),
            disabled: false,
            emoji: None,
            label: Some(label.to_string()),
            style,
            url: None,
            sku_id: None,
        })
    };
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: vec![
            b("yes", "Seal this thread", ButtonStyle::Danger),
            b("no", "Not needed", ButtonStyle::Secondary),
        ],
    })]
}

/// A press: its verb and the thread's slug.
pub(crate) fn parse(custom_id: &str) -> Option<(String, String)> {
    let rest = custom_id.strip_prefix(PREFIX)?.strip_prefix(':')?;
    let (verb, slug) = rest.split_once(':')?;
    (matches!(verb, "yes" | "no") && theseus_protocol::threads::slug_ok(slug))
        .then(|| (verb.to_string(), slug.to_string()))
}

/// The card's slug, from the core's `seal_card` post.
pub(crate) fn slug_of(body: &Value) -> String {
    body["slug"].as_str().unwrap_or("").to_string()
}

impl Shared {
    /// A card button: the seal or the dismissal goes to the core as the
    /// presser, from where they pressed. The reply is the presser's alone.
    pub(super) async fn seal_press(
        &self,
        i: &Interaction,
        (verb, slug): (String, String),
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
        let text = if verb == "yes" {
            let r = self
                .rpc
                .call::<_, ThreadSealResult>(
                    theseus_protocol::method::THREAD_SEAL,
                    ThreadSealParams {
                        slug: slug.clone(),
                        author: Some(who.to_string()),
                        discord,
                        ..Default::default()
                    },
                )
                .await;
            match r {
                Ok(r) => {
                    let mut t = format!("🔒 `{slug}` is sealed.");
                    for w in crossed_words(&r.crossed) {
                        t.push_str(&format!("\n{w}."));
                    }
                    if !r.crossed.is_empty() {
                        t.push_str(&format!(
                            "\n`theseus thread seal {slug} --redirect` leaves a note in each, withdrawing them."
                        ));
                    }
                    t
                }
                Err(e) => failed(&e, "The seal was not written"),
            }
        } else {
            let r = self
                .rpc
                .call::<_, ThreadRow>(
                    theseus_protocol::method::THREAD_SEAL_DISMISS,
                    ThreadNameParams {
                        slug: slug.clone(),
                        author: Some(who.to_string()),
                        discord,
                    },
                )
                .await;
            match r {
                Ok(_) => {
                    format!("Not needed: `{slug}` is not proposed again until its content changes.")
                }
                Err(e) => failed(&e, "The answer was not written"),
            }
        };
        self.followup(i, &text).await;
    }
}

fn failed(e: &crate::rpc_client::CallError, what: &str) -> String {
    match e.code == theseus_protocol::error_code::REFUSED {
        true => format!(
            "🔐 Your answer did not count: {}.",
            e.data
                .get("why")
                .and_then(Value::as_str)
                .unwrap_or(&e.message)
        ),
        false => format!("⚠️ {what}: {}", e.message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The card has the two buttons, and each press parses back to its verb
    /// and slug; anything else is not a seal press.
    #[test]
    fn a_card_has_two_buttons_and_each_press_parses() {
        let c = buttons("security-fix");
        let Component::ActionRow(row) = &c[0] else {
            panic!("a row")
        };
        let ids: Vec<&str> = row
            .components
            .iter()
            .map(|b| match b {
                Component::Button(b) => b.custom_id.as_deref().unwrap(),
                _ => panic!("a button"),
            })
            .collect();
        assert_eq!(ids, ["seal:yes:security-fix", "seal:no:security-fix"]);
        for id in ids {
            let (verb, slug) = parse(id).unwrap();
            assert!(matches!(verb.as_str(), "yes" | "no") && slug == "security-fix");
        }
        for bad in [
            "seal:maybe:x",
            "seal:yes:Bad Slug",
            "other:yes:x",
            "seal:yes",
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
        assert_eq!(
            slug_of(&json!({"kind": "seal_card", "slug": "tides"})),
            "tides"
        );
    }
}

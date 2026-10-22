//! A proposed link's card (theseus-98bz.12): the owner's DM gets "🔗 Notes of «stats» answered
//! questions in «pipeline» twice." with **Link these threads** and **Not needed**. A press is
//! `thread.link` as the presser, with the place it came from, so the core judges it as it judges
//! `theseus thread link`: the owner, from a private place. **Not needed** writes nothing: the core
//! marked the pair asked when it posted the card.

use serde_json::Value;
use theseus_protocol::threads::{slug_ok, ThreadLinkParams, ThreadRow};
use theseus_protocol::DiscordOrigin;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::http::interaction::InteractionResponseType;

use super::failed;
use crate::runtime::Shared;

/// A card button's custom id: `link:<yes|no>:<here>:<other>`.
const PREFIX: &str = "link";

/// The card's buttons: Link these threads, Not needed.
pub(crate) fn buttons(here: &str, other: &str) -> Vec<Component> {
    let b = |verb: &str, label: &str, style: ButtonStyle| {
        Component::Button(Button {
            id: None,
            custom_id: Some(format!("{PREFIX}:{verb}:{here}:{other}")),
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
            b("yes", "Link these threads", ButtonStyle::Primary),
            b("no", "Not needed", ButtonStyle::Secondary),
        ],
    })]
}

/// A press: its verb and the pair's slugs.
pub(crate) fn parse(custom_id: &str) -> Option<(String, String, String)> {
    let rest = custom_id.strip_prefix(PREFIX)?.strip_prefix(':')?;
    let (verb, pair) = rest.split_once(':')?;
    let (here, other) = pair.split_once(':')?;
    (matches!(verb, "yes" | "no") && slug_ok(here) && slug_ok(other))
        .then(|| (verb.to_string(), here.to_string(), other.to_string()))
}

/// The card's pair, from the core's `link_card` post.
pub(crate) fn pair_of(body: &Value) -> (String, String) {
    let s = |k: &str| body[k].as_str().unwrap_or("").to_string();
    (s("here"), s("other"))
}

impl Shared {
    /// A card button: the link goes to the core as the presser, from where
    /// they pressed. The reply is the presser's alone.
    pub(in crate::runtime) async fn link_press(
        &self,
        i: &Interaction,
        (verb, here, other): (String, String, String),
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
            let p = ThreadLinkParams {
                slug: here.clone(),
                other: other.clone(),
                author: Some(who.to_string()),
                discord,
            };
            match self
                .rpc
                .call::<_, ThreadRow>(theseus_protocol::method::THREAD_LINK, p)
                .await
            {
                Ok(_) => {
                    format!("🔗 `{here}` and `{other}` are linked: their notes pass between them.")
                }
                Err(e) => failed(&e, "Nothing was linked"),
            }
        } else {
            "Not linked. Jev does not ask about this pair again.".to_string()
        };
        self.followup(i, &text).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use theseus_protocol::threads::SLUG_MAX;

    fn ids(c: &[Component]) -> Vec<String> {
        let Component::ActionRow(row) = &c[0] else {
            panic!("a row")
        };
        row.components
            .iter()
            .map(|b| match b {
                Component::Button(b) => b.custom_id.clone().unwrap(),
                _ => panic!("a button"),
            })
            .collect()
    }

    /// The buttons give the pair back through `parse`; bad verbs, bad slugs and a missing part are
    /// not a link press; and the longest pair a slug allows fits Discord's 100-character id.
    #[test]
    fn link_card_ids_round_trip() {
        let got = ids(&buttons("pipeline", "stats"));
        assert_eq!(got, ["link:yes:pipeline:stats", "link:no:pipeline:stats"]);
        for id in &got {
            let (verb, here, other) = parse(id).unwrap();
            assert!(
                matches!(verb.as_str(), "yes" | "no")
                    && (here.as_str(), other.as_str()) == ("pipeline", "stats")
            );
        }
        for bad in [
            "link:maybe:a:b",
            "link:yes:Bad Slug:b",
            "link:yes:a:B",
            "link:yes:a",
            "link:yes",
            "seal:yes:a:b",
            "link:yes:a:b:c",
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
        let long = "x".repeat(SLUG_MAX);
        for id in ids(&buttons(&long, &long)) {
            assert!(id.len() <= 100, "{} > 100", id.len());
            assert!(parse(&id).is_some());
        }
        assert_eq!(
            pair_of(&json!({"kind": "link_card", "here": "a", "other": "b"})),
            ("a".into(), "b".into())
        );
    }
}

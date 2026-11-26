//! The owner's consent by a press and a form (issue 59), written once. A child of `runtime`.
//!
//! What a sign-in from Discord needs, whatever it signs in to (an MCP server, Google's one-time
//! consent): the presser read off the press, which the daemon judges (the owner's own press, in a
//! private place); an address shown to its presser alone (an ephemeral answer, and the address kept
//! here only for the user whose press began it); a form with one field, whose value goes straight
//! into the daemon's request and nowhere else; and a fixed-word answer. Each user of this module
//! names its own custom-id prefix, its words and its daemon calls; nothing here logs, keeps or says
//! a pasted value.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde_json::{json, Value};
use theseus_protocol::DiscordOrigin;
use twilight_model::application::interaction::modal::ModalInteractionData;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::channel::message::AllowedMentions;

use super::persona_soul::field_of;
use super::{origin_of, Shared};
use crate::render::Buttons;

/// The form's one field.
pub(crate) const FIELD: &str = "address";
/// Discord's cap on a text input.
const MAX: usize = 4000;

/// A word a custom id may carry: a server's name, or a sign-in's id.
pub(crate) fn word_ok(w: &str) -> bool {
    !w.is_empty()
        && w.len() <= 64
        && w.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

pub(crate) fn button(id: String, label: &str, style: ButtonStyle) -> Component {
    Component::Button(Button {
        id: None,
        custom_id: Some(id),
        disabled: false,
        emoji: None,
        label: Some(label.to_string()),
        style,
        url: None,
        sku_id: None,
    })
}

pub(crate) fn row(components: Vec<Component>) -> Vec<Component> {
    vec![Component::ActionRow(ActionRow {
        id: None,
        components,
    })]
}

/// **Paste the address**: `<prefix>:paste:<id>:<agreed>`.
pub(crate) fn paste_button(prefix: &str, id: &str, agreed: bool) -> Vec<Component> {
    let flag = if agreed { "1" } else { "0" };
    row(vec![button(
        format!("{prefix}:paste:{id}:{flag}"),
        "Paste the address",
        ButtonStyle::Primary,
    )])
}

/// The form **Paste the address** opens, `<prefix>:m:<id>:<agreed>`: one field.
pub(crate) fn modal(prefix: &str, id: &str, agreed: bool, title: &str) -> Value {
    form(
        &format!("{prefix}:m:{id}"),
        agreed,
        title,
        "The address your browser ended on",
        "The whole address, from the bar. It goes to Theseus and nowhere else.",
    )
}

/// A form of one field, `<head>:<agreed>`, whose value is read by `pasted` alone.
pub(crate) fn form(head: &str, agreed: bool, title: &str, label: &str, about: &str) -> Value {
    let flag = if agreed { "1" } else { "0" };
    json!({"type": 9, "data": {
        "custom_id": format!("{head}:{flag}"),
        "title": title,
        "components": [{
            "type": 18,
            "label": label,
            "description": about,
            "component": {"type": 4, "custom_id": FIELD, "style": 2,
                          "required": true, "max_length": MAX},
        }],
    }})
}

/// A card a turn offered on this pattern, as the courier posts it: its text, its key, its
/// buttons (`Buttons::Consent`, by its kind).
pub(crate) fn card(kind: &str, body: &Value, correlation: &str) -> (String, String, Buttons) {
    let key = match kind {
        "aws_alerts" => "awsc",
        _ => "gsi",
    };
    (
        body["text"].as_str().unwrap_or("").to_string(),
        format!("{key}:{correlation}"),
        Buttons::Consent(kind.to_string()),
    )
}

/// The buttons of a card `card` made.
pub(crate) fn card_buttons(kind: &str) -> Vec<Component> {
    match kind {
        "aws_alerts" => super::aws_alerts::buttons(),
        _ => super::google_sign_in::buttons(),
    }
}

/// The sign-in a form's custom id names, and whether its places were agreed to.
pub(crate) fn submitted(m: &ModalInteractionData, prefix: &str) -> Option<(String, bool)> {
    let rest = m.custom_id.strip_prefix(prefix)?.strip_prefix(":m:")?;
    let (id, flag) = rest.split_once(':')?;
    word_ok(id).then(|| (id.to_string(), flag == "1"))
}

/// The form's value: read here, and handed straight to the daemon's request.
pub(crate) fn pasted(m: &ModalInteractionData) -> String {
    field_of(m, FIELD)
}

/// The address each open sign-in shows, and the user whose press began it, kept between that press
/// and a later press of his: it holds no secret (the daemon's own words), only that user is shown it,
/// and a restart forgets it, as the daemon forgets the sign-in.
static ADDRESSES: Mutex<BTreeMap<String, (String, u64)>> = Mutex::new(BTreeMap::new());

pub(crate) fn keep_address(id: &str, url: &str, user: u64) {
    let mut all = ADDRESSES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if all.len() >= 16 {
        all.clear();
    }
    all.insert(id.to_string(), (url.to_string(), user));
}

/// The address of sign-in `id`, for the user whose press began it alone.
pub(crate) fn address_of(id: &str, user: Option<u64>) -> Option<String> {
    ADDRESSES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(id)
        .filter(|(_, by)| Some(*by) == user)
        .map(|(url, _)| url.clone())
}

/// The address to open, as a link, and what to do after.
pub(crate) fn address_words(url: &str) -> String {
    format!(
        "🔐 [Open this address to sign in](<{url}>). After you sign in, the page may fail to load: \
         copy the whole address your browser ends on, then press **Paste the address**."
    )
}

/// The presser, as the daemon judges it: read off the interaction.
pub(crate) fn presser(i: &Interaction) -> Option<DiscordOrigin> {
    origin_of(
        i.author().map(|u| u.id.get()),
        i.channel.as_ref().map(|c| c.id.get()),
        i.guild_id,
    )
}

/// The user whose press this is.
pub(crate) fn user_of(i: &Interaction) -> Option<u64> {
    i.author().map(|u| u.id.get())
}

impl Shared {
    /// An ephemeral answer with controls, to the presser alone; `what` names it in the board's error.
    pub(super) async fn consent_reply(
        &self,
        i: &Interaction,
        what: &str,
        text: &str,
        components: Vec<Component>,
    ) {
        use twilight_model::channel::message::MessageFlags;
        use twilight_model::http::interaction::{
            InteractionResponse, InteractionResponseData, InteractionResponseType,
        };
        let data = InteractionResponseData {
            content: Some(super::command_reply::leaving(&self.core.exits, text)),
            flags: Some(MessageFlags::EPHEMERAL),
            allowed_mentions: Some(AllowedMentions::default()),
            components: Some(components),
            ..Default::default()
        };
        let r = InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: Some(data),
        };
        if let Err(e) = self
            .interact
            .interaction(i.application_id)
            .create_response(i.id, &i.token, &r)
            .await
        {
            self.board.error(what, None, e);
        }
    }

    /// Edit the acknowledged answer to `text`, with `components`.
    pub(super) async fn consent_finish(
        &self,
        i: &Interaction,
        what: &str,
        text: &str,
        components: &[Component],
    ) {
        let content = super::command_reply::leaving(&self.core.exits, text);
        if let Err(e) = self
            .interact
            .interaction(i.application_id)
            .update_response(&i.token)
            .content(Some(&content))
            .components(Some(components))
            .allowed_mentions(Some(&AllowedMentions::default()))
            .await
        {
            self.board.error(what, None, e);
        }
    }
}

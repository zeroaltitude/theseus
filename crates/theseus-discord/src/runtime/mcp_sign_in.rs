//! Signing in to an MCP server from Discord, with no one at the machine (issue 59). A child of `runtime`.
//!
//! A turn can only **offer** the control: `mcp.sign_in_offer` posts one `mcp_sign_in` card to the
//! owner's direct messages, with **Sign in** and **Sign out** for the named server. Every step after
//! that is the owner's own press, which the daemon judges (`rpc/mcp_login.rs`: an owner, in a private
//! place, read off the press as every press is):
//!
//! - **Sign in** (`mcpin:in:<server>`) is `mcp.login.begin` with `manual` (the paste, as `theseus mcp
//!   login --manual` takes it). The presser alone is told the host he signs in at and every other
//!   place the sign-in goes through; when there is one, an **I agree** press (`mcpin:agree:<id>`) comes
//!   first, exactly where the CLI asks for its typed yes. Then the address to open, as a link, and
//!   **Paste the address** (`mcpin:paste:<id>:<agreed>`).
//! - **Paste the address** opens a modal with one field: the whole address the browser ended on. Its
//!   submit is `mcp.login.finish { pasted, agreed }`, the daemon's own manual finish.
//! - **Sign out** (`mcpin:out:<server>`) is `mcp.logout`.
//!
//! **The pasted value goes from the form to the daemon and nowhere else.** It is read off the modal
//! into the finish's params and handed to the request; it is in no message, no log line, no row and no
//! reply. Every answer is fixed words: signed in and how many tools, or one failure (expired, not the
//! address of this sign-in, refused, the server said no) with no part of what was pasted.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde_json::{json, Value};
use theseus_protocol::mcp::{
    McpLoginBeginParams, McpLoginBeginResult, McpLoginFinishParams, McpLoginFinishResult,
    McpLogoutParams, McpLogoutResult,
};
use theseus_protocol::{error_code, method, DiscordOrigin};
use twilight_model::application::interaction::modal::ModalInteractionData;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::channel::message::AllowedMentions;

use super::persona_soul::field_of;
use super::{origin_of, Shared};
use crate::rpc_client::CallError;

const PREFIX: &str = "mcpin";
/// The modal's custom id: `mcpin:m:<login id>:<agreed>`.
pub(crate) const MODAL: &str = "mcpin:m:";
/// The modal's one field.
const FIELD: &str = "address";
/// Discord's cap on a text input.
const MAX: usize = 4000;

/// The fixed words a press is answered with.
pub(crate) const NOT_HERE: &str =
    "🔐 Only you can sign in to an MCP server, and only in a private place: \
     your direct messages with Theseus, or a channel bound private. Nothing began.";
pub(crate) const EXPIRED: &str =
    "🔐 That sign-in is no longer open: it ran out, a newer one began, or \
     Theseus restarted. Press **Sign in** again.";
pub(crate) const NOT_THIS: &str =
    "🔐 That is not the address of this sign-in. Press **Sign in** again, \
     and paste the whole address your browser ends on.";
pub(crate) const REFUSED: &str = "🔐 The sign-in was refused, and nothing was kept.";
pub(crate) const SERVER_NO: &str =
    "🔐 The server said no, and nothing was kept. Press **Sign in** to try again.";
pub(crate) const NO_BEGIN: &str = "🔐 The sign-in could not begin. Nothing was kept.";
pub(crate) const SIGNED_OUT: &str = "🔐 Signed out: its sign-in is forgotten here.";
pub(crate) const NOTHING_KEPT: &str = "🔐 It had no sign-in kept.";
pub(crate) const NO_SIGN_OUT: &str = "🔐 The sign-out could not be done.";

/// A press of a control of this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Press {
    In(String),
    Out(String),
    Agree(String),
    Paste(String, bool),
}

/// A server's name, or a sign-in's id, as a custom id may carry it.
fn word_ok(w: &str) -> bool {
    !w.is_empty()
        && w.len() <= 64
        && w.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

pub(crate) fn parse(custom_id: &str) -> Option<Press> {
    let rest = custom_id.strip_prefix(PREFIX)?.strip_prefix(':')?;
    let (verb, word) = rest.split_once(':')?;
    let press = match verb {
        "in" => Press::In(word.into()),
        "out" => Press::Out(word.into()),
        "agree" => Press::Agree(word.into()),
        "paste" => {
            let (id, agreed) = word.split_once(':')?;
            Press::Paste(id.into(), agreed == "1")
        }
        _ => return None,
    };
    let w = match &press {
        Press::In(w) | Press::Out(w) | Press::Agree(w) | Press::Paste(w, _) => w,
    };
    word_ok(w).then_some(press)
}

/// The server the core's `mcp_sign_in` card is for.
pub(crate) fn server_of(body: &Value) -> String {
    body["server"].as_str().unwrap_or_default().to_string()
}

fn button(id: String, label: &str, style: ButtonStyle) -> Component {
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

fn row(components: Vec<Component>) -> Vec<Component> {
    vec![Component::ActionRow(ActionRow {
        id: None,
        components,
    })]
}

/// The card's two buttons.
pub(crate) fn buttons(server: &str) -> Vec<Component> {
    if !word_ok(server) {
        return vec![];
    }
    row(vec![
        button(
            format!("{PREFIX}:in:{server}"),
            "Sign in",
            ButtonStyle::Primary,
        ),
        button(
            format!("{PREFIX}:out:{server}"),
            "Sign out",
            ButtonStyle::Secondary,
        ),
    ])
}

fn paste_button(id: &str, agreed: bool) -> Vec<Component> {
    let flag = if agreed { "1" } else { "0" };
    row(vec![button(
        format!("{PREFIX}:paste:{id}:{flag}"),
        "Paste the address",
        ButtonStyle::Primary,
    )])
}

/// The modal "Paste the address" opens: one field.
pub(crate) fn modal(id: &str, agreed: bool) -> Value {
    let flag = if agreed { "1" } else { "0" };
    json!({"type": 9, "data": {
        "custom_id": format!("{MODAL}{id}:{flag}"),
        "title": "Finish the sign-in",
        "components": [{
            "type": 18,
            "label": "The address your browser ended on",
            "description": "The whole address, from the bar. It goes to Theseus and nowhere else.",
            "component": {"type": 4, "custom_id": FIELD, "style": 2,
                          "required": true, "max_length": MAX},
        }],
    }})
}

/// The sign-in a modal's custom id names, and whether its places were agreed to.
fn submitted(m: &ModalInteractionData) -> Option<(String, bool)> {
    let rest = m.custom_id.strip_prefix(MODAL)?;
    let (id, flag) = rest.split_once(':')?;
    word_ok(id).then(|| (id.to_string(), flag == "1"))
}

/// The address each open sign-in shows, and the user whose press began it, kept between that press
/// and the agree press: it holds no secret (the daemon's own words), only that user is shown it, and a
/// restart forgets it, as the daemon forgets the sign-in.
static ADDRESSES: Mutex<BTreeMap<String, (String, u64)>> = Mutex::new(BTreeMap::new());

fn keep_address(id: &str, url: &str, user: u64) {
    let mut all = ADDRESSES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if all.len() >= 16 {
        all.clear();
    }
    all.insert(id.to_string(), (url.to_string(), user));
}

/// The address of sign-in `id`, for the user whose press began it alone.
fn address_of(id: &str, user: Option<u64>) -> Option<String> {
    ADDRESSES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(id)
        .filter(|(_, by)| Some(*by) == user)
        .map(|(url, _)| url.clone())
}

/// What the presser is shown first: the host, and every other place it goes through.
pub(crate) fn host_words(b: &McpLoginBeginResult) -> String {
    let mut t = format!("🔐 You will sign in at `{}`.", b.host);
    for h in &b.elsewhere {
        t.push_str(&format!(
            "\nPart of the sign-in goes through `{h}`, which is not its issuer's own address."
        ));
    }
    if !b.elsewhere.is_empty() {
        t.push_str("\nPress **I agree** to go on; nothing is sent until you do.");
    }
    t
}

/// The address to open, as a link, and what to do after.
pub(crate) fn address_words(url: &str) -> String {
    format!(
        "🔐 [Open this address to sign in](<{url}>). After you sign in, the page may fail to load: \
         copy the whole address your browser ends on, then press **Paste the address**."
    )
}

/// A failure, in fixed words: never any part of what the daemon or the presser said.
pub(crate) fn failure(e: &CallError) -> &'static str {
    if e.code == error_code::REFUSED {
        return NOT_HERE;
    }
    let m = e.message.as_str();
    if m.starts_with("no sign-in is open") || m.contains("the redirect was taken") {
        EXPIRED
    } else if m.contains("not this sign-in's")
        || m.contains("the address")
        || m.contains("redirect")
    {
        NOT_THIS
    } else if m.starts_with("the sign-in was not completed") {
        SERVER_NO
    } else {
        REFUSED
    }
}

impl Shared {
    /// A press of this module's controls.
    pub(super) async fn mcp_sign_in_press(&self, i: &Interaction, press: Press, who: &str) {
        let discord = origin_of(
            i.author().map(|u| u.id.get()),
            i.channel.as_ref().map(|c| c.id.get()),
            i.guild_id,
        );
        match press {
            Press::In(server) => self.mcp_begin(i, &server, who, discord).await,
            Press::Agree(id) => match address_of(&id, i.author().map(|u| u.id.get())) {
                Some(url) => {
                    self.mcp_reply(i, &address_words(&url), paste_button(&id, true))
                        .await
                }
                None => self.tell(i, EXPIRED.into()).await,
            },
            Press::Paste(id, agreed) => self.respond_modal(i, &modal(&id, agreed)).await,
            Press::Out(server) => self.mcp_logout(i, &server, who, discord).await,
        }
    }

    /// An ephemeral answer with controls, to the presser alone.
    async fn mcp_reply(&self, i: &Interaction, text: &str, components: Vec<Component>) {
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
            self.board.error("mcp sign-in reply", None, e);
        }
    }

    /// Edit the acknowledged answer to `text`, with `components`.
    async fn mcp_finish(&self, i: &Interaction, text: &str, components: &[Component]) {
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
            self.board.error("mcp sign-in reply", None, e);
        }
    }

    /// **Sign in**: the daemon's manual begin, as the presser.
    async fn mcp_begin(
        &self,
        i: &Interaction,
        server: &str,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) {
        self.ack_ephemeral(i).await;
        let params = McpLoginBeginParams {
            name: server.to_string(),
            manual: true,
            author: Some(who.to_string()),
            discord,
        };
        let r = self
            .rpc
            .call::<_, McpLoginBeginResult>(method::MCP_LOGIN_BEGIN, params)
            .await;
        let b = match r {
            Ok(b) => b,
            Err(e) => {
                let words = match e.code == error_code::REFUSED {
                    true => NOT_HERE,
                    false => NO_BEGIN,
                };
                return self.mcp_finish(i, words, &[]).await;
            }
        };
        keep_address(&b.login_id, &b.url, i.author().map_or(0, |u| u.id.get()));
        match b.elsewhere.is_empty() {
            true => {
                let text = format!("{}\n{}", host_words(&b), address_words(&b.url));
                self.mcp_finish(i, &text, &paste_button(&b.login_id, false))
                    .await
            }
            false => {
                let agree = row(vec![button(
                    format!("{PREFIX}:agree:{}", b.login_id),
                    "I agree",
                    ButtonStyle::Primary,
                )]);
                self.mcp_finish(i, &host_words(&b), &agree).await
            }
        }
    }

    /// The form's submit: the daemon's manual finish, as the presser. The field goes into the
    /// request and nowhere else.
    pub(super) async fn mcp_sign_in_modal(
        &self,
        i: &Interaction,
        m: &ModalInteractionData,
        who: &str,
    ) {
        let Some((login_id, agreed)) = submitted(m) else {
            return self.tell(i, EXPIRED.into()).await;
        };
        self.ack_ephemeral(i).await;
        let discord = origin_of(
            i.author().map(|u| u.id.get()),
            i.channel.as_ref().map(|c| c.id.get()),
            i.guild_id,
        );
        let params = McpLoginFinishParams {
            login_id,
            pasted: Some(field_of(m, FIELD)),
            wait_secs: 0,
            agreed,
            author: Some(who.to_string()),
            discord,
        };
        let r = self
            .rpc
            .call::<_, McpLoginFinishResult>(method::MCP_LOGIN_FINISH, params)
            .await;
        let (text, controls) = match r {
            Ok(done) if done.state == "done" => (
                format!(
                    "✅ Signed in to `{}`: {} {} found.",
                    done.name,
                    done.tools,
                    if done.tools == 1 { "tool" } else { "tools" }
                ),
                buttons(&done.name),
            ),
            Ok(_) => (EXPIRED.to_string(), vec![]),
            Err(e) => (failure(&e).to_string(), vec![]),
        };
        self.mcp_finish(i, &text, &controls).await;
    }

    /// **Sign out**: the daemon's logout, as the presser.
    async fn mcp_logout(
        &self,
        i: &Interaction,
        server: &str,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) {
        self.ack_ephemeral(i).await;
        let params = McpLogoutParams {
            name: server.to_string(),
            author: Some(who.to_string()),
            discord,
        };
        let r = self
            .rpc
            .call::<_, McpLogoutResult>(method::MCP_LOGOUT, params)
            .await;
        let text = match r {
            Ok(out) if out.forgot => SIGNED_OUT,
            Ok(_) => NOTHING_KEPT,
            Err(e) if e.code == error_code::REFUSED => NOT_HERE,
            Err(_) => NO_SIGN_OUT,
        };
        self.mcp_finish(i, text, &[]).await;
    }
}

#[cfg(test)]
mod tests;

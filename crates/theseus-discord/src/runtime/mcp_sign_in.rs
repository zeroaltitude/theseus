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

use serde_json::Value;
use theseus_protocol::mcp::{
    McpLoginBeginParams, McpLoginBeginResult, McpLoginFinishParams, McpLoginFinishResult,
    McpLogoutParams, McpLogoutResult,
};
use theseus_protocol::{error_code, method, DiscordOrigin};
use twilight_model::application::interaction::modal::ModalInteractionData;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ButtonStyle, Component};

pub(crate) use super::consent::address_words;
#[cfg(test)]
use super::consent::FIELD;
use super::consent::{self, address_of, button, keep_address, row, word_ok};
use super::Shared;
use crate::rpc_client::CallError;

const PREFIX: &str = "mcpin";
/// The modal's custom id: `mcpin:m:<login id>:<agreed>`.
pub(crate) const MODAL: &str = "mcpin:m:";

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
    consent::paste_button(PREFIX, id, agreed)
}

/// The modal "Paste the address" opens: one field.
pub(crate) fn modal(id: &str, agreed: bool) -> Value {
    consent::modal(PREFIX, id, agreed, "Finish the sign-in")
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
        let discord = consent::presser(i);
        match press {
            Press::In(server) => self.mcp_begin(i, &server, who, discord).await,
            Press::Agree(id) => match address_of(&id, consent::user_of(i)) {
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
        self.consent_reply(i, "mcp sign-in reply", text, components)
            .await;
    }

    /// Edit the acknowledged answer to `text`, with `components`.
    async fn mcp_finish(&self, i: &Interaction, text: &str, components: &[Component]) {
        self.consent_finish(i, "mcp sign-in reply", text, components)
            .await;
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
        keep_address(&b.login_id, &b.url, consent::user_of(i).unwrap_or(0));
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
        let Some((login_id, agreed)) = consent::submitted(m, PREFIX) else {
            return self.tell(i, EXPIRED.into()).await;
        };
        self.ack_ephemeral(i).await;
        let discord = consent::presser(i);
        let params = McpLoginFinishParams {
            login_id,
            pasted: Some(consent::pasted(m)),
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
